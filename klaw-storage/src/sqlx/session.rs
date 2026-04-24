use super::{
    core::SqlxSessionStore,
    rows::{
        ApprovalRow, LlmAuditRow, LlmAuditSummaryRow, LlmUsageRow, LlmUsageSummaryRow,
        PendingQuestionRow, SessionIndexRow, ToolAuditRow, WebhookAgentRow, WebhookEventRow,
    },
};
use crate::{
    ApprovalRecord, ApprovalStatus, ChatRecord, LlmAuditFilterOptions, LlmAuditFilterOptionsQuery,
    LlmAuditQuery, LlmAuditRecord, LlmAuditSortOrder, LlmAuditSummaryRecord, LlmUsageRecord,
    LlmUsageSummary, NewApprovalRecord, NewLlmAuditRecord, NewLlmUsageRecord,
    NewPendingQuestionRecord, NewToolAuditRecord, NewWebhookAgentRecord, NewWebhookEventRecord,
    PendingQuestionRecord, PendingQuestionStatus, SessionCompressionState, SessionIndex,
    SessionSortOrder, SessionStorage, StorageError, ToolAuditFilterOptions,
    ToolAuditFilterOptionsQuery, ToolAuditQuery, ToolAuditRecord, UpdateWebhookAgentResult,
    UpdateWebhookEventResult, WebhookAgentQuery, WebhookAgentRecord, WebhookEventQuery,
    WebhookEventRecord, WebhookEventSortOrder, jsonl,
    util::{now_ms, relative_or_absolute_jsonl},
};
use async_trait::async_trait;
use sqlx::Row;
use std::path::PathBuf;

#[async_trait]
impl SessionStorage for SqlxSessionStore {
    async fn touch_session(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let jsonl_path = self.session_jsonl_path(session_key);
        let jsonl_path_str = relative_or_absolute_jsonl(&self.paths.root_dir, &jsonl_path);
        sqlx::query(
            "INSERT INTO sessions (
                session_key, chat_id, channel, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
            ) VALUES (?1, ?2, ?3, NULL, NULL, 0, NULL, 0, NULL, ?4, ?5, ?6, 0, ?7)
            ON CONFLICT(session_key) DO UPDATE SET
                chat_id=excluded.chat_id,
                channel=excluded.channel,
                updated_at_ms=excluded.updated_at_ms,
                last_message_at_ms=excluded.last_message_at_ms,
                jsonl_path=excluded.jsonl_path",
        )
        .bind(session_key)
        .bind(chat_id)
        .bind(channel)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(jsonl_path_str)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.get_session(session_key).await
    }

    async fn complete_turn(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let jsonl_path = self.session_jsonl_path(session_key);
        let jsonl_path_str = relative_or_absolute_jsonl(&self.paths.root_dir, &jsonl_path);
        let updated = sqlx::query(
            "UPDATE sessions
             SET
                chat_id = ?1,
                channel = ?2,
                updated_at_ms = ?3,
                last_message_at_ms = ?4,
                turn_count = turn_count + 1,
                jsonl_path = ?5
             WHERE session_key = ?6",
        )
        .bind(chat_id)
        .bind(channel)
        .bind(now)
        .bind(now)
        .bind(jsonl_path_str.clone())
        .bind(session_key)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;

        if updated.rows_affected() == 0 {
            sqlx::query(
                "INSERT INTO sessions (
                    session_key, chat_id, channel, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
                ) VALUES (?1, ?2, ?3, NULL, NULL, 0, NULL, 0, NULL, ?4, ?5, ?6, 1, ?7)",
            )
            .bind(session_key)
            .bind(chat_id)
            .bind(channel)
            .bind(now)
            .bind(now)
            .bind(now)
            .bind(jsonl_path_str)
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        }

        self.get_session(session_key).await
    }

    async fn append_chat_record(
        &self,
        session_key: &str,
        record: &ChatRecord,
    ) -> Result<(), StorageError> {
        jsonl::append_chat_record(&self.paths, session_key, record).await
    }

    async fn read_chat_records(&self, session_key: &str) -> Result<Vec<ChatRecord>, StorageError> {
        jsonl::read_chat_records(&self.paths, session_key).await
    }

    async fn read_chat_records_page(
        &self,
        session_key: &str,
        before_message_id: Option<&str>,
        limit: usize,
    ) -> Result<crate::ChatRecordPage, StorageError> {
        jsonl::read_chat_records_page(&self.paths, session_key, before_message_id, limit).await
    }

    async fn get_session(&self, session_key: &str) -> Result<SessionIndex, StorageError> {
        let row = sqlx::query_as::<_, SessionIndexRow>(
            "SELECT session_key, chat_id, channel, title, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             FROM sessions
             WHERE session_key = ?1",
        )
        .bind(session_key)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(row.into())
    }

    async fn set_session_title(
        &self,
        session_key: &str,
        title: Option<&str>,
    ) -> Result<SessionIndex, StorageError> {
        let updated = sqlx::query(
            "UPDATE sessions
             SET title = ?1
             WHERE session_key = ?2",
        )
        .bind(title)
        .bind(session_key)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;

        if updated.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "session '{session_key}' not found when setting title"
            )));
        }

        self.get_session(session_key).await
    }

    async fn delete_session(&self, session_key: &str) -> Result<bool, StorageError> {
        let deleted = sqlx::query("DELETE FROM sessions WHERE session_key = ?1")
            .bind(session_key)
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?
            .rows_affected()
            > 0;
        if deleted {
            jsonl::delete_chat_records(&self.paths, session_key).await?;
        }
        Ok(deleted)
    }

    async fn get_session_by_active_session_key(
        &self,
        active_session_key: &str,
    ) -> Result<SessionIndex, StorageError> {
        let row = sqlx::query_as::<_, SessionIndexRow>(
            "SELECT session_key, chat_id, channel, title, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             FROM sessions
             WHERE active_session_key = ?1
             ORDER BY CASE WHEN session_key = active_session_key THEN 1 ELSE 0 END, updated_at_ms DESC
             LIMIT 1",
        )
        .bind(active_session_key)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(row.into())
    }

    async fn get_or_create_session_state(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        _default_provider: &str,
        _default_model: &str,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let jsonl_path = self.session_jsonl_path(session_key);
        let jsonl_path_str = relative_or_absolute_jsonl(&self.paths.root_dir, &jsonl_path);
        sqlx::query(
            "INSERT INTO sessions (
                session_key, chat_id, channel, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
            ) VALUES (?1, ?2, ?3, ?4, NULL, 0, NULL, 0, NULL, ?5, ?6, ?7, 0, ?8)
            ON CONFLICT(session_key) DO UPDATE SET
                chat_id=excluded.chat_id,
                channel=excluded.channel,
                updated_at_ms=excluded.updated_at_ms,
                active_session_key=COALESCE(sessions.active_session_key, excluded.active_session_key),
                jsonl_path=excluded.jsonl_path",
        )
        .bind(session_key)
        .bind(chat_id)
        .bind(channel)
        .bind(session_key)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(jsonl_path_str)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.get_session(session_key).await
    }

    async fn set_active_session(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        active_session_key: &str,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let updated = sqlx::query(
            "UPDATE sessions
             SET chat_id = ?1,
                 channel = ?2,
                 updated_at_ms = ?3,
                 active_session_key = ?4
             WHERE session_key = ?5",
        )
        .bind(chat_id)
        .bind(channel)
        .bind(now)
        .bind(active_session_key)
        .bind(session_key)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        if updated.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "session '{session_key}' not found when setting active_session_key"
            )));
        }
        self.get_session(session_key).await
    }

    async fn set_model_provider(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        model_provider: &str,
        model: &str,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let updated = sqlx::query(
            "UPDATE sessions
             SET chat_id = ?1,
                 channel = ?2,
                 updated_at_ms = ?3,
                 model_provider = ?4,
                 model_provider_explicit = 1,
                 model = ?5,
                 model_explicit = 1
             WHERE session_key = ?6",
        )
        .bind(chat_id)
        .bind(channel)
        .bind(now)
        .bind(model_provider)
        .bind(model)
        .bind(session_key)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        if updated.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "session '{session_key}' not found when setting model_provider"
            )));
        }
        self.get_session(session_key).await
    }

    async fn set_model(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        model: &str,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let updated = sqlx::query(
            "UPDATE sessions
             SET chat_id = ?1,
                 channel = ?2,
                 updated_at_ms = ?3,
                 model = ?4,
                 model_explicit = 0
             WHERE session_key = ?5",
        )
        .bind(chat_id)
        .bind(channel)
        .bind(now)
        .bind(model)
        .bind(session_key)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        if updated.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "session '{session_key}' not found when setting model"
            )));
        }
        self.get_session(session_key).await
    }

    async fn set_delivery_metadata(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        delivery_metadata_json: Option<&str>,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let updated = sqlx::query(
            "UPDATE sessions
             SET chat_id = ?1,
                 channel = ?2,
                 updated_at_ms = ?3,
                 delivery_metadata_json = ?4
             WHERE session_key = ?5",
        )
        .bind(chat_id)
        .bind(channel)
        .bind(now)
        .bind(delivery_metadata_json)
        .bind(session_key)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        if updated.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "session '{session_key}' not found when setting delivery_metadata"
            )));
        }
        self.get_session(session_key).await
    }

    async fn clear_model_routing_override(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let updated = sqlx::query(
            "UPDATE sessions
             SET chat_id = ?1,
                 channel = ?2,
                 updated_at_ms = ?3,
                 model_provider = NULL,
                 model_provider_explicit = 0,
                 model = NULL,
                 model_explicit = 0
             WHERE session_key = ?4",
        )
        .bind(chat_id)
        .bind(channel)
        .bind(now)
        .bind(session_key)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        if updated.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "session '{session_key}' not found when clearing model routing override"
            )));
        }
        self.get_session(session_key).await
    }

    async fn get_session_compression_state(
        &self,
        session_key: &str,
    ) -> Result<Option<SessionCompressionState>, StorageError> {
        let row = sqlx::query(
            "SELECT compression_last_len, compression_summary_json
             FROM sessions
             WHERE session_key = ?1",
        )
        .bind(session_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(StorageError::backend)?;

        Ok(row.map(|value| SessionCompressionState {
            last_compressed_len: value.get::<i64, _>("compression_last_len"),
            summary_json: value.get::<Option<String>, _>("compression_summary_json"),
        }))
    }

    async fn set_session_compression_state(
        &self,
        session_key: &str,
        state: &SessionCompressionState,
    ) -> Result<(), StorageError> {
        let updated = sqlx::query(
            "UPDATE sessions
             SET compression_last_len = ?2,
                 compression_summary_json = ?3,
                 updated_at_ms = ?4
             WHERE session_key = ?1",
        )
        .bind(session_key)
        .bind(state.last_compressed_len)
        .bind(state.summary_json.as_deref())
        .bind(now_ms())
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;

        if updated.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "session '{session_key}' not found when setting compression state"
            )));
        }
        Ok(())
    }

    async fn list_sessions(
        &self,
        limit: Option<i64>,
        offset: i64,
        updated_from_ms: Option<i64>,
        updated_to_ms: Option<i64>,
        channel: Option<&str>,
        session_key_prefix: Option<&str>,
        sort_order: SessionSortOrder,
    ) -> Result<Vec<SessionIndex>, StorageError> {
        let mut query = String::from(
            "SELECT session_key, chat_id, channel, title, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             FROM sessions WHERE 1=1",
        );
        if updated_from_ms.is_some() {
            query.push_str(" AND updated_at_ms >= ?");
        }
        if updated_to_ms.is_some() {
            query.push_str(" AND updated_at_ms <= ?");
        }
        if channel.is_some() {
            query.push_str(" AND channel = ?");
        }
        if session_key_prefix.is_some() {
            query.push_str(" AND session_key LIKE ?");
        }
        query.push_str(&format!(" ORDER BY {}", sort_order.sql_order_by()));
        if limit.is_some() {
            query.push_str(" LIMIT ? OFFSET ?");
        }

        let mut q = sqlx::query_as::<_, SessionIndexRow>(&query);
        if let Some(from) = updated_from_ms {
            q = q.bind(from);
        }
        if let Some(to) = updated_to_ms {
            q = q.bind(to);
        }
        if let Some(channel) = channel {
            q = q.bind(channel);
        }
        if let Some(prefix) = session_key_prefix {
            q = q.bind(format!("{}%", prefix));
        }
        if let Some(limit) = limit {
            q = q.bind(limit.max(1)).bind(offset.max(0));
        }

        let rows = q
            .fetch_all(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_session_channels(&self) -> Result<Vec<String>, StorageError> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT channel
             FROM sessions
             ORDER BY channel ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(rows)
    }

    async fn append_llm_usage(
        &self,
        input: &NewLlmUsageRecord,
    ) -> Result<LlmUsageRecord, StorageError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO llm_usage (
                id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                input_tokens, output_tokens, total_tokens, cached_input_tokens, reasoning_tokens,
                source, provider_request_id, provider_response_id, created_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        )
        .bind(&input.id)
        .bind(&input.session_key)
        .bind(&input.chat_id)
        .bind(input.turn_index)
        .bind(input.request_seq)
        .bind(&input.provider)
        .bind(&input.model)
        .bind(&input.wire_api)
        .bind(input.input_tokens)
        .bind(input.output_tokens)
        .bind(input.total_tokens)
        .bind(input.cached_input_tokens)
        .bind(input.reasoning_tokens)
        .bind(input.source.as_str())
        .bind(&input.provider_request_id)
        .bind(&input.provider_response_id)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        let row = sqlx::query_as::<_, LlmUsageRow>(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    input_tokens, output_tokens, total_tokens, cached_input_tokens, reasoning_tokens,
                    source, provider_request_id, provider_response_id, created_at_ms
             FROM llm_usage
             WHERE id = ?1",
        )
        .bind(&input.id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        LlmUsageRecord::try_from(row)
    }

    async fn list_llm_usage(
        &self,
        session_key: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<LlmUsageRecord>, StorageError> {
        let rows = sqlx::query_as::<_, LlmUsageRow>(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    input_tokens, output_tokens, total_tokens, cached_input_tokens, reasoning_tokens,
                    source, provider_request_id, provider_response_id, created_at_ms
             FROM llm_usage
             WHERE session_key = ?1
             ORDER BY turn_index DESC, request_seq DESC, created_at_ms DESC
             LIMIT ?2 OFFSET ?3",
        )
        .bind(session_key)
        .bind(limit.max(1))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        rows.into_iter().map(LlmUsageRecord::try_from).collect()
    }

    async fn sum_llm_usage_by_session(
        &self,
        session_key: &str,
    ) -> Result<LlmUsageSummary, StorageError> {
        let row = sqlx::query_as::<_, LlmUsageSummaryRow>(
            "SELECT
                COUNT(*) as request_count,
                COALESCE(SUM(input_tokens), 0) as input_tokens,
                COALESCE(SUM(output_tokens), 0) as output_tokens,
                COALESCE(SUM(total_tokens), 0) as total_tokens,
                COALESCE(SUM(cached_input_tokens), 0) as cached_input_tokens,
                COALESCE(SUM(reasoning_tokens), 0) as reasoning_tokens
             FROM llm_usage
             WHERE session_key = ?1",
        )
        .bind(session_key)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(row.into())
    }

    async fn sum_llm_usage_by_turn(
        &self,
        session_key: &str,
        turn_index: i64,
    ) -> Result<LlmUsageSummary, StorageError> {
        let row = sqlx::query_as::<_, LlmUsageSummaryRow>(
            "SELECT
                COUNT(*) as request_count,
                COALESCE(SUM(input_tokens), 0) as input_tokens,
                COALESCE(SUM(output_tokens), 0) as output_tokens,
                COALESCE(SUM(total_tokens), 0) as total_tokens,
                COALESCE(SUM(cached_input_tokens), 0) as cached_input_tokens,
                COALESCE(SUM(reasoning_tokens), 0) as reasoning_tokens
             FROM llm_usage
             WHERE session_key = ?1 AND turn_index = ?2",
        )
        .bind(session_key)
        .bind(turn_index)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(row.into())
    }

    async fn append_llm_audit(
        &self,
        input: &NewLlmAuditRecord,
    ) -> Result<LlmAuditRecord, StorageError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO llm_audit (
                id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                status, error_code, error_message, provider_request_id, provider_response_id,
                request_body_json, response_body_json, metadata_json, requested_at_ms, responded_at_ms, created_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        )
        .bind(&input.id)
        .bind(&input.session_key)
        .bind(&input.chat_id)
        .bind(input.turn_index)
        .bind(input.request_seq)
        .bind(&input.provider)
        .bind(&input.model)
        .bind(&input.wire_api)
        .bind(input.status.as_str())
        .bind(&input.error_code)
        .bind(&input.error_message)
        .bind(&input.provider_request_id)
        .bind(&input.provider_response_id)
        .bind(&input.request_body_json)
        .bind(&input.response_body_json)
        .bind(&input.metadata_json)
        .bind(input.requested_at_ms)
        .bind(input.responded_at_ms)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        let row = sqlx::query_as::<_, LlmAuditRow>(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    status, error_code, error_message, provider_request_id, provider_response_id,
                    request_body_json, response_body_json, metadata_json, requested_at_ms, responded_at_ms, created_at_ms
             FROM llm_audit
             WHERE id = ?1",
        )
        .bind(&input.id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        LlmAuditRecord::try_from(row)
    }

    async fn list_llm_audit(
        &self,
        query: &LlmAuditQuery,
    ) -> Result<Vec<LlmAuditRecord>, StorageError> {
        let sort_order = match query.sort_order {
            LlmAuditSortOrder::RequestedAtAsc => "requested_at_ms ASC, created_at_ms ASC",
            LlmAuditSortOrder::RequestedAtDesc => "requested_at_ms DESC, created_at_ms DESC",
        };
        let rows = sqlx::query_as::<_, LlmAuditRow>(&format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    status, error_code, error_message, provider_request_id, provider_response_id,
                    request_body_json, response_body_json, metadata_json, requested_at_ms, responded_at_ms, created_at_ms
             FROM llm_audit
             WHERE (?1 IS NULL OR session_key = ?1)
               AND (?2 IS NULL OR provider = ?2)
               AND (?3 IS NULL OR requested_at_ms >= ?3)
               AND (?4 IS NULL OR requested_at_ms <= ?4)
             ORDER BY {sort_order}
             LIMIT ?5 OFFSET ?6"
        ))
        .bind(query.session_key.as_deref())
        .bind(query.provider.as_deref())
        .bind(query.requested_from_ms)
        .bind(query.requested_to_ms)
        .bind(query.limit.max(1))
        .bind(query.offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        rows.into_iter().map(LlmAuditRecord::try_from).collect()
    }

    async fn get_llm_audit(&self, audit_id: &str) -> Result<LlmAuditRecord, StorageError> {
        let row = sqlx::query_as::<_, LlmAuditRow>(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    status, error_code, error_message, provider_request_id, provider_response_id,
                    request_body_json, response_body_json, metadata_json, requested_at_ms, responded_at_ms, created_at_ms
             FROM llm_audit
             WHERE id = ?1",
        )
        .bind(audit_id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        LlmAuditRecord::try_from(row)
    }

    async fn list_llm_audit_summaries(
        &self,
        query: &LlmAuditQuery,
    ) -> Result<Vec<LlmAuditSummaryRecord>, StorageError> {
        let sort_order = match query.sort_order {
            LlmAuditSortOrder::RequestedAtAsc => "requested_at_ms ASC, created_at_ms ASC",
            LlmAuditSortOrder::RequestedAtDesc => "requested_at_ms DESC, created_at_ms DESC",
        };
        let rows = sqlx::query_as::<_, LlmAuditSummaryRow>(&format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    status, error_code, error_message, provider_request_id, provider_response_id,
                    requested_at_ms, responded_at_ms, created_at_ms
             FROM llm_audit
             WHERE (?1 IS NULL OR session_key = ?1)
               AND (?2 IS NULL OR provider = ?2)
               AND (?3 IS NULL OR requested_at_ms >= ?3)
               AND (?4 IS NULL OR requested_at_ms <= ?4)
             ORDER BY {sort_order}
             LIMIT ?5 OFFSET ?6"
        ))
        .bind(query.session_key.as_deref())
        .bind(query.provider.as_deref())
        .bind(query.requested_from_ms)
        .bind(query.requested_to_ms)
        .bind(query.limit.max(1))
        .bind(query.offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        rows.into_iter()
            .map(LlmAuditSummaryRecord::try_from)
            .collect()
    }

    async fn list_llm_audit_filter_options(
        &self,
        query: &LlmAuditFilterOptionsQuery,
    ) -> Result<LlmAuditFilterOptions, StorageError> {
        let session_keys = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT session_key
             FROM llm_audit
             WHERE (?1 IS NULL OR requested_at_ms >= ?1)
               AND (?2 IS NULL OR requested_at_ms <= ?2)
             ORDER BY session_key ASC",
        )
        .bind(query.requested_from_ms)
        .bind(query.requested_to_ms)
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        let providers = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT provider
             FROM llm_audit
             WHERE (?1 IS NULL OR requested_at_ms >= ?1)
               AND (?2 IS NULL OR requested_at_ms <= ?2)
             ORDER BY provider ASC",
        )
        .bind(query.requested_from_ms)
        .bind(query.requested_to_ms)
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(LlmAuditFilterOptions {
            session_keys,
            providers,
        })
    }

    async fn append_tool_audit(
        &self,
        input: &NewToolAuditRecord,
    ) -> Result<ToolAuditRecord, StorageError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO tool_audit (
                id, session_key, chat_id, turn_index, request_seq, tool_call_seq, tool_name,
                status, error_code, error_message, retryable, approval_required, arguments_json,
                result_content, error_details_json, signals_json, metadata_json, started_at_ms,
                finished_at_ms, created_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        )
        .bind(&input.id)
        .bind(&input.session_key)
        .bind(&input.chat_id)
        .bind(input.turn_index)
        .bind(input.request_seq)
        .bind(input.tool_call_seq)
        .bind(&input.tool_name)
        .bind(input.status.as_str())
        .bind(&input.error_code)
        .bind(&input.error_message)
        .bind(input.retryable.map(|flag| if flag { 1_i64 } else { 0_i64 }))
        .bind(if input.approval_required { 1_i64 } else { 0_i64 })
        .bind(&input.arguments_json)
        .bind(&input.result_content)
        .bind(&input.error_details_json)
        .bind(&input.signals_json)
        .bind(&input.metadata_json)
        .bind(input.started_at_ms)
        .bind(input.finished_at_ms)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        let row = sqlx::query_as::<_, ToolAuditRow>(
            "SELECT id, session_key, chat_id, turn_index, request_seq, tool_call_seq, tool_name,
                    status, error_code, error_message, retryable, approval_required,
                    arguments_json, result_content, error_details_json, signals_json,
                    metadata_json, started_at_ms, finished_at_ms, created_at_ms
             FROM tool_audit
             WHERE id = ?1",
        )
        .bind(&input.id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        ToolAuditRecord::try_from(row)
    }

    async fn list_tool_audit(
        &self,
        query: &ToolAuditQuery,
    ) -> Result<Vec<ToolAuditRecord>, StorageError> {
        let rows = sqlx::query_as::<_, ToolAuditRow>(&format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, tool_call_seq, tool_name,
                    status, error_code, error_message, retryable, approval_required,
                    arguments_json, result_content, error_details_json, signals_json,
                    metadata_json, started_at_ms, finished_at_ms, created_at_ms
             FROM tool_audit
             WHERE (?1 IS NULL OR session_key = ?1)
               AND (?2 IS NULL OR tool_name = ?2)
               AND (?3 IS NULL OR started_at_ms >= ?3)
               AND (?4 IS NULL OR started_at_ms <= ?4)
             ORDER BY {}
             LIMIT ?5 OFFSET ?6",
            query.sort_order.sql_order_by()
        ))
        .bind(query.session_key.as_deref())
        .bind(query.tool_name.as_deref())
        .bind(query.started_from_ms)
        .bind(query.started_to_ms)
        .bind(query.limit.max(1))
        .bind(query.offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        rows.into_iter().map(ToolAuditRecord::try_from).collect()
    }

    async fn list_tool_audit_filter_options(
        &self,
        query: &ToolAuditFilterOptionsQuery,
    ) -> Result<ToolAuditFilterOptions, StorageError> {
        let session_keys = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT session_key
             FROM tool_audit
             WHERE (?1 IS NULL OR started_at_ms >= ?1)
               AND (?2 IS NULL OR started_at_ms <= ?2)
             ORDER BY session_key ASC",
        )
        .bind(query.started_from_ms)
        .bind(query.started_to_ms)
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        let tool_names = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT tool_name
             FROM tool_audit
             WHERE (?1 IS NULL OR started_at_ms >= ?1)
               AND (?2 IS NULL OR started_at_ms <= ?2)
             ORDER BY tool_name ASC",
        )
        .bind(query.started_from_ms)
        .bind(query.started_to_ms)
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(ToolAuditFilterOptions {
            session_keys,
            tool_names,
        })
    }

    async fn append_webhook_event(
        &self,
        input: &NewWebhookEventRecord,
    ) -> Result<WebhookEventRecord, StorageError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO webhook_events (
                id, source, event_type, session_key, chat_id, sender_id, content,
                payload_json, metadata_json, status, error_message, response_summary,
                received_at_ms, processed_at_ms, remote_addr, created_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        )
        .bind(&input.id)
        .bind(&input.source)
        .bind(&input.event_type)
        .bind(&input.session_key)
        .bind(&input.chat_id)
        .bind(&input.sender_id)
        .bind(&input.content)
        .bind(&input.payload_json)
        .bind(&input.metadata_json)
        .bind(input.status.as_str())
        .bind(&input.error_message)
        .bind(&input.response_summary)
        .bind(input.received_at_ms)
        .bind(input.processed_at_ms)
        .bind(&input.remote_addr)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;

        let row = sqlx::query_as::<_, WebhookEventRow>(
            "SELECT id, source, event_type, session_key, chat_id, sender_id, content,
                    payload_json, metadata_json, status, error_message, response_summary,
                    received_at_ms, processed_at_ms, remote_addr, created_at_ms
             FROM webhook_events
             WHERE id = ?1",
        )
        .bind(&input.id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        WebhookEventRecord::try_from(row)
    }

    async fn update_webhook_event_status(
        &self,
        event_id: &str,
        update: &UpdateWebhookEventResult,
    ) -> Result<WebhookEventRecord, StorageError> {
        sqlx::query(
            "UPDATE webhook_events
             SET status = ?2, error_message = ?3, response_summary = ?4, processed_at_ms = ?5
             WHERE id = ?1",
        )
        .bind(event_id)
        .bind(update.status.as_str())
        .bind(&update.error_message)
        .bind(&update.response_summary)
        .bind(update.processed_at_ms)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;

        let row = sqlx::query_as::<_, WebhookEventRow>(
            "SELECT id, source, event_type, session_key, chat_id, sender_id, content,
                    payload_json, metadata_json, status, error_message, response_summary,
                    received_at_ms, processed_at_ms, remote_addr, created_at_ms
             FROM webhook_events
             WHERE id = ?1",
        )
        .bind(event_id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        WebhookEventRecord::try_from(row)
    }

    async fn list_webhook_events(
        &self,
        query: &WebhookEventQuery,
    ) -> Result<Vec<WebhookEventRecord>, StorageError> {
        let sort_order = match query.sort_order {
            WebhookEventSortOrder::ReceivedAtAsc => "received_at_ms ASC, created_at_ms ASC",
            WebhookEventSortOrder::ReceivedAtDesc => "received_at_ms DESC, created_at_ms DESC",
        };
        let rows = sqlx::query_as::<_, WebhookEventRow>(&format!(
            "SELECT id, source, event_type, session_key, chat_id, sender_id, content,
                    payload_json, metadata_json, status, error_message, response_summary,
                    received_at_ms, processed_at_ms, remote_addr, created_at_ms
             FROM webhook_events
             WHERE (?1 IS NULL OR source = ?1)
               AND (?2 IS NULL OR event_type = ?2)
               AND (?3 IS NULL OR session_key = ?3)
               AND (?4 IS NULL OR status = ?4)
               AND (?5 IS NULL OR received_at_ms >= ?5)
               AND (?6 IS NULL OR received_at_ms <= ?6)
             ORDER BY {sort_order}
             LIMIT ?7 OFFSET ?8"
        ))
        .bind(query.source.as_deref())
        .bind(query.event_type.as_deref())
        .bind(query.session_key.as_deref())
        .bind(query.status.map(|status| status.as_str()))
        .bind(query.received_from_ms)
        .bind(query.received_to_ms)
        .bind(query.limit.max(1))
        .bind(query.offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        rows.into_iter().map(WebhookEventRecord::try_from).collect()
    }

    async fn append_webhook_agent(
        &self,
        input: &NewWebhookAgentRecord,
    ) -> Result<WebhookAgentRecord, StorageError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO webhook_agents (
                id, hook_id, session_key, chat_id, sender_id, content,
                payload_json, metadata_json, status, error_message, response_summary,
                received_at_ms, processed_at_ms, remote_addr, created_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        )
        .bind(&input.id)
        .bind(&input.hook_id)
        .bind(&input.session_key)
        .bind(&input.chat_id)
        .bind(&input.sender_id)
        .bind(&input.content)
        .bind(&input.payload_json)
        .bind(&input.metadata_json)
        .bind(input.status.as_str())
        .bind(&input.error_message)
        .bind(&input.response_summary)
        .bind(input.received_at_ms)
        .bind(input.processed_at_ms)
        .bind(&input.remote_addr)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;

        let row = sqlx::query_as::<_, WebhookAgentRow>(
            "SELECT id, hook_id, session_key, chat_id, sender_id, content,
                    payload_json, metadata_json, status, error_message, response_summary,
                    received_at_ms, processed_at_ms, remote_addr, created_at_ms
             FROM webhook_agents
             WHERE id = ?1",
        )
        .bind(&input.id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        WebhookAgentRecord::try_from(row)
    }

    async fn update_webhook_agent_status(
        &self,
        event_id: &str,
        update: &UpdateWebhookAgentResult,
    ) -> Result<WebhookAgentRecord, StorageError> {
        sqlx::query(
            "UPDATE webhook_agents
             SET status = ?2, error_message = ?3, response_summary = ?4, processed_at_ms = ?5
             WHERE id = ?1",
        )
        .bind(event_id)
        .bind(update.status.as_str())
        .bind(&update.error_message)
        .bind(&update.response_summary)
        .bind(update.processed_at_ms)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;

        let row = sqlx::query_as::<_, WebhookAgentRow>(
            "SELECT id, hook_id, session_key, chat_id, sender_id, content,
                    payload_json, metadata_json, status, error_message, response_summary,
                    received_at_ms, processed_at_ms, remote_addr, created_at_ms
             FROM webhook_agents
             WHERE id = ?1",
        )
        .bind(event_id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        WebhookAgentRecord::try_from(row)
    }

    async fn list_webhook_agents(
        &self,
        query: &WebhookAgentQuery,
    ) -> Result<Vec<WebhookAgentRecord>, StorageError> {
        let sort_order = match query.sort_order {
            WebhookEventSortOrder::ReceivedAtAsc => "received_at_ms ASC, created_at_ms ASC",
            WebhookEventSortOrder::ReceivedAtDesc => "received_at_ms DESC, created_at_ms DESC",
        };
        let rows = sqlx::query_as::<_, WebhookAgentRow>(&format!(
            "SELECT id, hook_id, session_key, chat_id, sender_id, content,
                    payload_json, metadata_json, status, error_message, response_summary,
                    received_at_ms, processed_at_ms, remote_addr, created_at_ms
             FROM webhook_agents
             WHERE (?1 IS NULL OR hook_id = ?1)
               AND (?2 IS NULL OR session_key = ?2)
               AND (?3 IS NULL OR status = ?3)
               AND (?4 IS NULL OR received_at_ms >= ?4)
               AND (?5 IS NULL OR received_at_ms <= ?5)
             ORDER BY {sort_order}
             LIMIT ?6 OFFSET ?7"
        ))
        .bind(query.hook_id.as_deref())
        .bind(query.session_key.as_deref())
        .bind(query.status.map(|status| status.as_str()))
        .bind(query.received_from_ms)
        .bind(query.received_to_ms)
        .bind(query.limit.max(1))
        .bind(query.offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        rows.into_iter().map(WebhookAgentRecord::try_from).collect()
    }

    async fn create_approval(
        &self,
        input: &NewApprovalRecord,
    ) -> Result<ApprovalRecord, StorageError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO approvals (
                id, session_key, tool_name, command_hash, command_preview, command_text, risk_level, status,
                requested_by, approved_by, justification, expires_at_ms, created_at_ms, updated_at_ms, consumed_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10, ?11, ?12, ?13, NULL)",
        )
        .bind(&input.id)
        .bind(&input.session_key)
        .bind(&input.tool_name)
        .bind(&input.command_hash)
        .bind(&input.command_preview)
        .bind(&input.command_text)
        .bind(&input.risk_level)
        .bind(ApprovalStatus::Pending.as_str())
        .bind(&input.requested_by)
        .bind(&input.justification)
        .bind(input.expires_at_ms)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.get_approval(&input.id).await
    }

    async fn get_approval(&self, approval_id: &str) -> Result<ApprovalRecord, StorageError> {
        let row = sqlx::query_as::<_, ApprovalRow>(
            "SELECT id, session_key, tool_name, command_hash, command_preview, command_text, risk_level, status,
                    requested_by, approved_by, justification, expires_at_ms, created_at_ms, updated_at_ms, consumed_at_ms
             FROM approvals
             WHERE id = ?1",
        )
        .bind(approval_id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        ApprovalRecord::try_from(row)
    }

    async fn update_approval_status(
        &self,
        approval_id: &str,
        status: ApprovalStatus,
        approved_by: Option<&str>,
    ) -> Result<ApprovalRecord, StorageError> {
        let now = now_ms();
        let updated = sqlx::query(
            "UPDATE approvals
             SET status = ?1,
                 approved_by = ?2,
                 updated_at_ms = ?3
             WHERE id = ?4",
        )
        .bind(status.as_str())
        .bind(approved_by)
        .bind(now)
        .bind(approval_id)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        if updated.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "approval '{approval_id}' not found when setting status"
            )));
        }
        self.get_approval(approval_id).await
    }

    async fn consume_approved_tool_command(
        &self,
        approval_id: &str,
        tool_name: &str,
        session_key: &str,
        command_hash: &str,
        now_ms: i64,
    ) -> Result<bool, StorageError> {
        let updated = sqlx::query(
            "UPDATE approvals
             SET status = ?1,
                 consumed_at_ms = ?2,
                 updated_at_ms = ?2
             WHERE id = ?3
               AND tool_name = ?4
               AND session_key = ?5
               AND command_hash = ?6
               AND status = ?7
               AND consumed_at_ms IS NULL
               AND expires_at_ms >= ?2",
        )
        .bind(ApprovalStatus::Consumed.as_str())
        .bind(now_ms)
        .bind(approval_id)
        .bind(tool_name)
        .bind(session_key)
        .bind(command_hash)
        .bind(ApprovalStatus::Approved.as_str())
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(updated.rows_affected() > 0)
    }

    async fn consume_latest_approved_tool_command(
        &self,
        tool_name: &str,
        session_key: &str,
        command_hash: &str,
        now_ms: i64,
    ) -> Result<bool, StorageError> {
        let approval_id = sqlx::query_scalar::<_, String>(
            "SELECT id
             FROM approvals
             WHERE tool_name = ?1
               AND session_key = ?2
               AND command_hash = ?3
               AND status = ?4
               AND consumed_at_ms IS NULL
               AND expires_at_ms >= ?5
             ORDER BY created_at_ms DESC
             LIMIT 1",
        )
        .bind(tool_name)
        .bind(session_key)
        .bind(command_hash)
        .bind(ApprovalStatus::Approved.as_str())
        .bind(now_ms)
        .fetch_optional(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        let Some(approval_id) = approval_id else {
            return Ok(false);
        };
        self.consume_approved_tool_command(
            &approval_id,
            tool_name,
            session_key,
            command_hash,
            now_ms,
        )
        .await
    }

    async fn create_pending_question(
        &self,
        input: &NewPendingQuestionRecord,
    ) -> Result<PendingQuestionRecord, StorageError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO pending_questions (
                id, session_key, channel, chat_id, title, question_text, options_json, status,
                selected_option_id, answered_by, expires_at_ms, created_at_ms, updated_at_ms, answered_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, ?9, ?10, ?11, NULL)",
        )
        .bind(&input.id)
        .bind(&input.session_key)
        .bind(&input.channel)
        .bind(&input.chat_id)
        .bind(&input.title)
        .bind(&input.question_text)
        .bind(&input.options_json)
        .bind(PendingQuestionStatus::Pending.as_str())
        .bind(input.expires_at_ms)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.get_pending_question(&input.id).await
    }

    async fn get_pending_question(
        &self,
        question_id: &str,
    ) -> Result<PendingQuestionRecord, StorageError> {
        let row = sqlx::query_as::<_, PendingQuestionRow>(
            "SELECT id, session_key, channel, chat_id, title, question_text, options_json, status,
                    selected_option_id, answered_by, expires_at_ms, created_at_ms, updated_at_ms, answered_at_ms
             FROM pending_questions
             WHERE id = ?1",
        )
        .bind(question_id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        PendingQuestionRecord::try_from(row)
    }

    async fn update_pending_question_answer(
        &self,
        question_id: &str,
        status: PendingQuestionStatus,
        selected_option_id: Option<&str>,
        answered_by: Option<&str>,
        answered_at_ms: Option<i64>,
    ) -> Result<PendingQuestionRecord, StorageError> {
        let now = now_ms();
        let updated = sqlx::query(
            "UPDATE pending_questions
             SET status = ?1,
                 selected_option_id = ?2,
                 answered_by = ?3,
                 answered_at_ms = ?4,
                 updated_at_ms = ?5
             WHERE id = ?6",
        )
        .bind(status.as_str())
        .bind(selected_option_id)
        .bind(answered_by)
        .bind(answered_at_ms)
        .bind(now)
        .bind(question_id)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        if updated.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "pending question '{question_id}' not found when updating answer"
            )));
        }
        self.get_pending_question(question_id).await
    }

    fn session_jsonl_path(&self, session_key: &str) -> PathBuf {
        jsonl::session_jsonl_path(&self.paths, session_key)
    }
}
