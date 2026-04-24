use super::{
    core::TursoSessionStore,
    mapping::{
        collect_string_column, escape_sql_text, llm_audit_requested_range_where, opt_i64_sql,
        opt_sql_i64, opt_sql_text, opt_string_sql, row_to_approval, row_to_llm_audit,
        row_to_llm_audit_summary, row_to_llm_usage, row_to_llm_usage_summary,
        row_to_pending_question, row_to_session_index, row_to_tool_audit, row_to_webhook_agent,
        row_to_webhook_event, tool_audit_started_range_where, value_to_i64, value_to_opt_string,
        value_to_string,
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
use std::path::PathBuf;

#[async_trait]
impl SessionStorage for TursoSessionStore {
    async fn touch_session(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let jsonl_path = self.session_jsonl_path(session_key);
        let jsonl_path_str = relative_or_absolute_jsonl(&self.paths.root_dir, &jsonl_path);
        let sql = format!(
            "INSERT INTO sessions (
                session_key, chat_id, channel, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             ) VALUES ('{}', '{}', '{}', NULL, NULL, 0, NULL, 0, NULL, {}, {}, {}, 0, '{}')
             ON CONFLICT(session_key) DO UPDATE SET
                chat_id=excluded.chat_id,
                channel=excluded.channel,
                updated_at_ms=excluded.updated_at_ms,
                last_message_at_ms=excluded.last_message_at_ms,
                jsonl_path=excluded.jsonl_path",
            escape_sql_text(session_key),
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            now,
            now,
            now,
            escape_sql_text(&jsonl_path_str)
        );
        {
            let conn = self.connection().await?;
            conn.execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
        }
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
        let update_sql = format!(
            "UPDATE sessions
             SET
                chat_id = '{}',
                channel = '{}',
                updated_at_ms = {},
                last_message_at_ms = {},
                turn_count = turn_count + 1,
                jsonl_path = '{}'
             WHERE session_key = '{}'",
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            now,
            now,
            escape_sql_text(&jsonl_path_str),
            escape_sql_text(session_key)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&update_sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                let insert_sql = format!(
                    "INSERT INTO sessions (
                        session_key, chat_id, channel, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
                    ) VALUES ('{}', '{}', '{}', NULL, NULL, 0, NULL, 0, NULL, {}, {}, {}, 1, '{}')",
                    escape_sql_text(session_key),
                    escape_sql_text(chat_id),
                    escape_sql_text(channel),
                    now,
                    now,
                    now,
                    escape_sql_text(&jsonl_path_str)
                );
                conn.execute(&insert_sql, ())
                    .await
                    .map_err(StorageError::backend)?;
            }
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
        let sql = format!(
            "SELECT session_key, chat_id, channel, title, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             FROM sessions
             WHERE session_key = '{}'
             LIMIT 1",
            escape_sql_text(session_key)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("session not found"))?;
        row_to_session_index(&row)
    }

    async fn set_session_title(
        &self,
        session_key: &str,
        title: Option<&str>,
    ) -> Result<SessionIndex, StorageError> {
        let title_sql = title
            .map(|value| format!("'{}'", escape_sql_text(value)))
            .unwrap_or_else(|| "NULL".to_string());
        let sql = format!(
            "UPDATE sessions
             SET title = {title_sql}
             WHERE session_key = '{}'",
            escape_sql_text(session_key)
        );
        let affected = {
            let conn = self.connection().await?;
            conn.execute(&sql, ())
                .await
                .map_err(StorageError::backend)?
        };
        if affected == 0 {
            return Err(StorageError::backend(format!(
                "session '{session_key}' not found when setting title"
            )));
        }
        self.get_session(session_key).await
    }

    async fn delete_session(&self, session_key: &str) -> Result<bool, StorageError> {
        let sql = format!(
            "DELETE FROM sessions WHERE session_key = '{}'",
            escape_sql_text(session_key)
        );
        let deleted = {
            let conn = self.connection().await?;
            conn.execute(&sql, ())
                .await
                .map_err(StorageError::backend)?
                > 0
        };
        if deleted {
            jsonl::delete_chat_records(&self.paths, session_key).await?;
        }
        Ok(deleted)
    }

    async fn get_session_by_active_session_key(
        &self,
        active_session_key: &str,
    ) -> Result<SessionIndex, StorageError> {
        let sql = format!(
            "SELECT session_key, chat_id, channel, title, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             FROM sessions
             WHERE active_session_key = '{}'
             ORDER BY CASE WHEN session_key = active_session_key THEN 1 ELSE 0 END, updated_at_ms DESC
             LIMIT 1",
            escape_sql_text(active_session_key)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("session not found"))?;
        row_to_session_index(&row)
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
        let sql = format!(
            "INSERT INTO sessions (
                session_key, chat_id, channel, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             ) VALUES ('{}', '{}', '{}', '{}', NULL, 0, NULL, 0, NULL, {}, {}, {}, 0, '{}')
             ON CONFLICT(session_key) DO UPDATE SET
                chat_id=excluded.chat_id,
                channel=excluded.channel,
                updated_at_ms=excluded.updated_at_ms,
                active_session_key=COALESCE(sessions.active_session_key, excluded.active_session_key),
                jsonl_path=excluded.jsonl_path",
            escape_sql_text(session_key),
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            escape_sql_text(session_key),
            now,
            now,
            now,
            escape_sql_text(&jsonl_path_str)
        );
        {
            let conn = self.connection().await?;
            conn.execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
        }
        self.get_session(session_key).await
    }

    async fn set_active_session(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        active_session_key: &str,
    ) -> Result<SessionIndex, StorageError> {
        let sql = format!(
            "UPDATE sessions
             SET chat_id = '{}',
                 channel = '{}',
                 updated_at_ms = {},
                 active_session_key = '{}'
             WHERE session_key = '{}'",
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            now_ms(),
            escape_sql_text(active_session_key),
            escape_sql_text(session_key)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                return Err(StorageError::backend(format!(
                    "session '{session_key}' not found when setting active_session_key"
                )));
            }
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
        let sql = format!(
            "UPDATE sessions
             SET chat_id = '{}',
                 channel = '{}',
                 updated_at_ms = {},
                 model_provider = '{}',
                 model_provider_explicit = 1,
                 model = '{}',
                 model_explicit = 1
             WHERE session_key = '{}'",
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            now_ms(),
            escape_sql_text(model_provider),
            escape_sql_text(model),
            escape_sql_text(session_key)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                return Err(StorageError::backend(format!(
                    "session '{session_key}' not found when setting model_provider"
                )));
            }
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
        let sql = format!(
            "UPDATE sessions
             SET chat_id = '{}',
                 channel = '{}',
                 updated_at_ms = {},
                 model = '{}',
                 model_explicit = 0
             WHERE session_key = '{}'",
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            now_ms(),
            escape_sql_text(model),
            escape_sql_text(session_key)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                return Err(StorageError::backend(format!(
                    "session '{session_key}' not found when setting model"
                )));
            }
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
        let sql = format!(
            "UPDATE sessions
             SET chat_id = '{}',
                 channel = '{}',
                 updated_at_ms = {},
                 delivery_metadata_json = {}
             WHERE session_key = '{}'",
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            now_ms(),
            opt_sql_text(delivery_metadata_json),
            escape_sql_text(session_key)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                return Err(StorageError::backend(format!(
                    "session '{session_key}' not found when setting delivery_metadata"
                )));
            }
        }
        self.get_session(session_key).await
    }

    async fn clear_model_routing_override(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, StorageError> {
        let sql = format!(
            "UPDATE sessions
             SET chat_id = '{}',
                 channel = '{}',
                 updated_at_ms = {},
                 model_provider = NULL,
                 model_provider_explicit = 0,
                 model = NULL,
                 model_explicit = 0
             WHERE session_key = '{}'",
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            now_ms(),
            escape_sql_text(session_key)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                return Err(StorageError::backend(format!(
                    "session '{session_key}' not found when clearing model routing override"
                )));
            }
        }
        self.get_session(session_key).await
    }

    async fn get_session_compression_state(
        &self,
        session_key: &str,
    ) -> Result<Option<SessionCompressionState>, StorageError> {
        let sql = format!(
            "SELECT compression_last_len, compression_summary_json
             FROM sessions
             WHERE session_key = '{}'",
            escape_sql_text(session_key)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let Some(row) = rows.next().await.map_err(StorageError::backend)? else {
            return Ok(None);
        };
        Ok(Some(SessionCompressionState {
            last_compressed_len: value_to_i64(row.get_value(0).map_err(StorageError::backend)?)?,
            summary_json: value_to_opt_string(row.get_value(1).map_err(StorageError::backend)?),
        }))
    }

    async fn set_session_compression_state(
        &self,
        session_key: &str,
        state: &SessionCompressionState,
    ) -> Result<(), StorageError> {
        let summary_sql = match state.summary_json.as_ref() {
            Some(value) => format!("'{}'", escape_sql_text(value)),
            None => "NULL".to_string(),
        };
        let sql = format!(
            "UPDATE sessions
             SET compression_last_len = {},
                 compression_summary_json = {},
                 updated_at_ms = {}
             WHERE session_key = '{}'",
            state.last_compressed_len,
            summary_sql,
            now_ms(),
            escape_sql_text(session_key)
        );
        let conn = self.connection().await?;
        let affected = conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        if affected == 0 {
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
        let mut sql = String::from(
            "SELECT session_key, chat_id, channel, title, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             FROM sessions WHERE 1=1",
        );
        if let Some(from) = updated_from_ms {
            sql.push_str(&format!(" AND updated_at_ms >= {}", from));
        }
        if let Some(to) = updated_to_ms {
            sql.push_str(&format!(" AND updated_at_ms <= {}", to));
        }
        if let Some(channel) = channel {
            sql.push_str(&format!(" AND channel = '{}'", escape_sql_text(channel)));
        }
        if let Some(prefix) = session_key_prefix {
            sql.push_str(&format!(
                " AND session_key LIKE '{}%'",
                escape_sql_text(prefix)
            ));
        }
        sql.push_str(&format!(" ORDER BY {}", sort_order.sql_order_by()));
        if let Some(limit) = limit {
            sql.push_str(&format!(" LIMIT {} OFFSET {}", limit.max(1), offset.max(0)));
        }
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_session_index(&row)?);
        }
        Ok(out)
    }

    async fn list_session_channels(&self) -> Result<Vec<String>, StorageError> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT DISTINCT channel
                 FROM sessions
                 ORDER BY channel ASC",
                (),
            )
            .await
            .map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(value_to_string(
                row.get_value(0).map_err(StorageError::backend)?,
            )?);
        }
        Ok(out)
    }

    async fn append_llm_usage(
        &self,
        input: &NewLlmUsageRecord,
    ) -> Result<LlmUsageRecord, StorageError> {
        let now = now_ms();
        let sql = format!(
            "INSERT INTO llm_usage (
                id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                input_tokens, output_tokens, total_tokens, cached_input_tokens, reasoning_tokens,
                source, provider_request_id, provider_response_id, created_at_ms
            ) VALUES ('{}', '{}', '{}', {}, {}, '{}', '{}', '{}', {}, {}, {}, {}, {}, '{}', {}, {}, {})",
            escape_sql_text(&input.id),
            escape_sql_text(&input.session_key),
            escape_sql_text(&input.chat_id),
            input.turn_index,
            input.request_seq,
            escape_sql_text(&input.provider),
            escape_sql_text(&input.model),
            escape_sql_text(&input.wire_api),
            input.input_tokens,
            input.output_tokens,
            input.total_tokens,
            opt_i64_sql(input.cached_input_tokens),
            opt_i64_sql(input.reasoning_tokens),
            input.source.as_str(),
            opt_string_sql(input.provider_request_id.as_deref()),
            opt_string_sql(input.provider_response_id.as_deref()),
            now
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        let query_sql = format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    input_tokens, output_tokens, total_tokens, cached_input_tokens, reasoning_tokens,
                    source, provider_request_id, provider_response_id, created_at_ms
             FROM llm_usage
             WHERE id = '{}'
             LIMIT 1",
            escape_sql_text(&input.id)
        );
        let mut rows = conn
            .query(&query_sql, ())
            .await
            .map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("llm usage not found"))?;
        row_to_llm_usage(&row)
    }

    async fn list_llm_usage(
        &self,
        session_key: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<LlmUsageRecord>, StorageError> {
        let sql = format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    input_tokens, output_tokens, total_tokens, cached_input_tokens, reasoning_tokens,
                    source, provider_request_id, provider_response_id, created_at_ms
             FROM llm_usage
             WHERE session_key = '{}'
             ORDER BY turn_index DESC, request_seq DESC, created_at_ms DESC
             LIMIT {} OFFSET {}",
            escape_sql_text(session_key),
            limit.max(1),
            offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_llm_usage(&row)?);
        }
        Ok(out)
    }

    async fn sum_llm_usage_by_session(
        &self,
        session_key: &str,
    ) -> Result<LlmUsageSummary, StorageError> {
        let sql = format!(
            "SELECT
                COUNT(*) as request_count,
                COALESCE(SUM(input_tokens), 0) as input_tokens,
                COALESCE(SUM(output_tokens), 0) as output_tokens,
                COALESCE(SUM(total_tokens), 0) as total_tokens,
                COALESCE(SUM(cached_input_tokens), 0) as cached_input_tokens,
                COALESCE(SUM(reasoning_tokens), 0) as reasoning_tokens
             FROM llm_usage
             WHERE session_key = '{}'",
            escape_sql_text(session_key)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("llm usage summary not found"))?;
        row_to_llm_usage_summary(&row)
    }

    async fn sum_llm_usage_by_turn(
        &self,
        session_key: &str,
        turn_index: i64,
    ) -> Result<LlmUsageSummary, StorageError> {
        let sql = format!(
            "SELECT
                COUNT(*) as request_count,
                COALESCE(SUM(input_tokens), 0) as input_tokens,
                COALESCE(SUM(output_tokens), 0) as output_tokens,
                COALESCE(SUM(total_tokens), 0) as total_tokens,
                COALESCE(SUM(cached_input_tokens), 0) as cached_input_tokens,
                COALESCE(SUM(reasoning_tokens), 0) as reasoning_tokens
             FROM llm_usage
             WHERE session_key = '{}' AND turn_index = {}",
            escape_sql_text(session_key),
            turn_index
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("llm usage summary not found"))?;
        row_to_llm_usage_summary(&row)
    }

    async fn append_llm_audit(
        &self,
        input: &NewLlmAuditRecord,
    ) -> Result<LlmAuditRecord, StorageError> {
        let now = now_ms();
        let sql = format!(
            "INSERT INTO llm_audit (
                id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                status, error_code, error_message, provider_request_id, provider_response_id,
                request_body_json, response_body_json, metadata_json, requested_at_ms, responded_at_ms, created_at_ms
            ) VALUES ('{}', '{}', '{}', {}, {}, '{}', '{}', '{}', '{}', {}, {}, {}, {}, '{}', {}, {}, {}, {}, {})",
            escape_sql_text(&input.id),
            escape_sql_text(&input.session_key),
            escape_sql_text(&input.chat_id),
            input.turn_index,
            input.request_seq,
            escape_sql_text(&input.provider),
            escape_sql_text(&input.model),
            escape_sql_text(&input.wire_api),
            input.status.as_str(),
            opt_string_sql(input.error_code.as_deref()),
            opt_string_sql(input.error_message.as_deref()),
            opt_string_sql(input.provider_request_id.as_deref()),
            opt_string_sql(input.provider_response_id.as_deref()),
            escape_sql_text(&input.request_body_json),
            opt_string_sql(input.response_body_json.as_deref()),
            opt_sql_text(input.metadata_json.as_deref()),
            input.requested_at_ms,
            opt_i64_sql(input.responded_at_ms),
            now
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        let query_sql = format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    status, error_code, error_message, provider_request_id, provider_response_id,
                    request_body_json, response_body_json, metadata_json, requested_at_ms, responded_at_ms, created_at_ms
             FROM llm_audit
             WHERE id = '{}'
             LIMIT 1",
            escape_sql_text(&input.id)
        );
        let mut rows = conn
            .query(&query_sql, ())
            .await
            .map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("llm audit not found"))?;
        row_to_llm_audit(&row)
    }

    async fn list_llm_audit(
        &self,
        query: &LlmAuditQuery,
    ) -> Result<Vec<LlmAuditRecord>, StorageError> {
        let sort_order = match query.sort_order {
            LlmAuditSortOrder::RequestedAtAsc => "requested_at_ms ASC, created_at_ms ASC",
            LlmAuditSortOrder::RequestedAtDesc => "requested_at_ms DESC, created_at_ms DESC",
        };
        let mut conditions = Vec::new();
        if let Some(session_key) = query
            .session_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            conditions.push(format!("session_key = '{}'", escape_sql_text(session_key)));
        }
        if let Some(provider) = query
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            conditions.push(format!("provider = '{}'", escape_sql_text(provider)));
        }
        if let Some(from_ms) = query.requested_from_ms {
            conditions.push(format!("requested_at_ms >= {from_ms}"));
        }
        if let Some(to_ms) = query.requested_to_ms {
            conditions.push(format!("requested_at_ms <= {to_ms}"));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let sql = format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    status, error_code, error_message, provider_request_id, provider_response_id,
                    request_body_json, response_body_json, metadata_json, requested_at_ms, responded_at_ms, created_at_ms
             FROM llm_audit
             {where_clause}
             ORDER BY {sort_order}
             LIMIT {} OFFSET {}",
            query.limit.max(1),
            query.offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_llm_audit(&row)?);
        }
        Ok(out)
    }

    async fn get_llm_audit(&self, audit_id: &str) -> Result<LlmAuditRecord, StorageError> {
        let sql = format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    status, error_code, error_message, provider_request_id, provider_response_id,
                    request_body_json, response_body_json, metadata_json, requested_at_ms, responded_at_ms, created_at_ms
             FROM llm_audit
             WHERE id = '{}'
             LIMIT 1",
            escape_sql_text(audit_id)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("llm audit not found"))?;
        row_to_llm_audit(&row)
    }

    async fn list_llm_audit_summaries(
        &self,
        query: &LlmAuditQuery,
    ) -> Result<Vec<LlmAuditSummaryRecord>, StorageError> {
        let sort_order = match query.sort_order {
            LlmAuditSortOrder::RequestedAtAsc => "requested_at_ms ASC, created_at_ms ASC",
            LlmAuditSortOrder::RequestedAtDesc => "requested_at_ms DESC, created_at_ms DESC",
        };
        let mut conditions = Vec::new();
        if let Some(session_key) = query
            .session_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            conditions.push(format!("session_key = '{}'", escape_sql_text(session_key)));
        }
        if let Some(provider) = query
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            conditions.push(format!("provider = '{}'", escape_sql_text(provider)));
        }
        if let Some(from_ms) = query.requested_from_ms {
            conditions.push(format!("requested_at_ms >= {from_ms}"));
        }
        if let Some(to_ms) = query.requested_to_ms {
            conditions.push(format!("requested_at_ms <= {to_ms}"));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let sql = format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    status, error_code, error_message, provider_request_id, provider_response_id,
                    requested_at_ms, responded_at_ms, created_at_ms
             FROM llm_audit
             {where_clause}
             ORDER BY {sort_order}
             LIMIT {} OFFSET {}",
            query.limit.max(1),
            query.offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_llm_audit_summary(&row)?);
        }
        Ok(out)
    }

    async fn list_llm_audit_filter_options(
        &self,
        query: &LlmAuditFilterOptionsQuery,
    ) -> Result<LlmAuditFilterOptions, StorageError> {
        let where_clause =
            llm_audit_requested_range_where(query.requested_from_ms, query.requested_to_ms);
        let conn = self.connection().await?;
        let mut session_rows = conn
            .query(
                &format!(
                    "SELECT DISTINCT session_key
                     FROM llm_audit
                     {where_clause}
                     ORDER BY session_key ASC"
                ),
                (),
            )
            .await
            .map_err(StorageError::backend)?;
        let mut session_keys = Vec::new();
        while let Some(row) = session_rows.next().await.map_err(StorageError::backend)? {
            let value = row.get_value(0).map_err(StorageError::backend)?;
            session_keys.push(value_to_string(value)?);
        }

        let mut provider_rows = conn
            .query(
                &format!(
                    "SELECT DISTINCT provider
                     FROM llm_audit
                     {where_clause}
                     ORDER BY provider ASC"
                ),
                (),
            )
            .await
            .map_err(StorageError::backend)?;
        let mut providers = Vec::new();
        while let Some(row) = provider_rows.next().await.map_err(StorageError::backend)? {
            let value = row.get_value(0).map_err(StorageError::backend)?;
            providers.push(value_to_string(value)?);
        }

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
        let sql = format!(
            "INSERT INTO tool_audit (
                id, session_key, chat_id, turn_index, request_seq, tool_call_seq, tool_name,
                status, error_code, error_message, retryable, approval_required, arguments_json,
                result_content, error_details_json, signals_json, metadata_json, started_at_ms,
                finished_at_ms, created_at_ms
            ) VALUES ('{}', '{}', '{}', {}, {}, {}, '{}', '{}', {}, {}, {}, {}, '{}', '{}', {}, {}, {}, {}, {}, {})",
            escape_sql_text(&input.id),
            escape_sql_text(&input.session_key),
            escape_sql_text(&input.chat_id),
            input.turn_index,
            input.request_seq,
            input.tool_call_seq,
            escape_sql_text(&input.tool_name),
            input.status.as_str(),
            opt_string_sql(input.error_code.as_deref()),
            opt_string_sql(input.error_message.as_deref()),
            opt_i64_sql(input.retryable.map(|flag| if flag { 1 } else { 0 })),
            if input.approval_required { 1 } else { 0 },
            escape_sql_text(&input.arguments_json),
            escape_sql_text(&input.result_content),
            opt_sql_text(input.error_details_json.as_deref()),
            opt_sql_text(input.signals_json.as_deref()),
            opt_sql_text(input.metadata_json.as_deref()),
            input.started_at_ms,
            input.finished_at_ms,
            now
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        let query_sql = format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, tool_call_seq, tool_name,
                    status, error_code, error_message, retryable, approval_required,
                    arguments_json, result_content, error_details_json, signals_json,
                    metadata_json, started_at_ms, finished_at_ms, created_at_ms
             FROM tool_audit
             WHERE id = '{}'
             LIMIT 1",
            escape_sql_text(&input.id)
        );
        let mut rows = conn
            .query(&query_sql, ())
            .await
            .map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("tool audit not found"))?;
        row_to_tool_audit(&row)
    }

    async fn list_tool_audit(
        &self,
        query: &ToolAuditQuery,
    ) -> Result<Vec<ToolAuditRecord>, StorageError> {
        let mut conditions = Vec::new();
        if let Some(session_key) = query
            .session_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            conditions.push(format!("session_key = '{}'", escape_sql_text(session_key)));
        }
        if let Some(tool_name) = query
            .tool_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            conditions.push(format!("tool_name = '{}'", escape_sql_text(tool_name)));
        }
        if let Some(from_ms) = query.started_from_ms {
            conditions.push(format!("started_at_ms >= {from_ms}"));
        }
        if let Some(to_ms) = query.started_to_ms {
            conditions.push(format!("started_at_ms <= {to_ms}"));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let sql = format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, tool_call_seq, tool_name,
                    status, error_code, error_message, retryable, approval_required,
                    arguments_json, result_content, error_details_json, signals_json,
                    metadata_json, started_at_ms, finished_at_ms, created_at_ms
             FROM tool_audit
             {where_clause}
             ORDER BY {}
             LIMIT {}
             OFFSET {}",
            query.sort_order.sql_order_by(),
            query.limit.max(1),
            query.offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_tool_audit(&row)?);
        }
        Ok(out)
    }

    async fn list_tool_audit_filter_options(
        &self,
        query: &ToolAuditFilterOptionsQuery,
    ) -> Result<ToolAuditFilterOptions, StorageError> {
        let where_clause =
            tool_audit_started_range_where(query.started_from_ms, query.started_to_ms);
        let session_sql = format!(
            "SELECT DISTINCT session_key
             FROM tool_audit
             {where_clause}
             ORDER BY session_key ASC"
        );
        let tool_sql = format!(
            "SELECT DISTINCT tool_name
             FROM tool_audit
             {where_clause}
             ORDER BY tool_name ASC"
        );
        let conn = self.connection().await?;
        let session_keys = collect_string_column(&conn, &session_sql).await?;
        let tool_names = collect_string_column(&conn, &tool_sql).await?;
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
        let sql = format!(
            "INSERT INTO webhook_events (
                id, source, event_type, session_key, chat_id, sender_id, content,
                payload_json, metadata_json, status, error_message, response_summary,
                received_at_ms, processed_at_ms, remote_addr, created_at_ms
            ) VALUES (
                '{}', '{}', '{}', '{}', '{}', '{}', '{}',
                {}, {}, '{}', {}, {}, {}, {}, {}, {}
            )",
            escape_sql_text(&input.id),
            escape_sql_text(&input.source),
            escape_sql_text(&input.event_type),
            escape_sql_text(&input.session_key),
            escape_sql_text(&input.chat_id),
            escape_sql_text(&input.sender_id),
            escape_sql_text(&input.content),
            opt_sql_text(input.payload_json.as_deref()),
            opt_sql_text(input.metadata_json.as_deref()),
            input.status.as_str(),
            opt_sql_text(input.error_message.as_deref()),
            opt_sql_text(input.response_summary.as_deref()),
            input.received_at_ms,
            opt_sql_i64(input.processed_at_ms),
            opt_sql_text(input.remote_addr.as_deref()),
            now
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;

        let mut rows = conn
            .query(
                &format!(
                    "SELECT id, source, event_type, session_key, chat_id, sender_id, content,
                            payload_json, metadata_json, status, error_message, response_summary,
                            received_at_ms, processed_at_ms, remote_addr, created_at_ms
                     FROM webhook_events
                     WHERE id = '{}'
                     LIMIT 1",
                    escape_sql_text(&input.id)
                ),
                (),
            )
            .await
            .map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("webhook event not found"))?;
        row_to_webhook_event(&row)
    }

    async fn update_webhook_event_status(
        &self,
        event_id: &str,
        update: &UpdateWebhookEventResult,
    ) -> Result<WebhookEventRecord, StorageError> {
        let sql = format!(
            "UPDATE webhook_events
             SET status = '{}', error_message = {}, response_summary = {}, processed_at_ms = {}
             WHERE id = '{}'",
            update.status.as_str(),
            opt_sql_text(update.error_message.as_deref()),
            opt_sql_text(update.response_summary.as_deref()),
            opt_sql_i64(update.processed_at_ms),
            escape_sql_text(event_id)
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;

        let mut rows = conn
            .query(
                &format!(
                    "SELECT id, source, event_type, session_key, chat_id, sender_id, content,
                            payload_json, metadata_json, status, error_message, response_summary,
                            received_at_ms, processed_at_ms, remote_addr, created_at_ms
                     FROM webhook_events
                     WHERE id = '{}'
                     LIMIT 1",
                    escape_sql_text(event_id)
                ),
                (),
            )
            .await
            .map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("webhook event not found"))?;
        row_to_webhook_event(&row)
    }

    async fn list_webhook_events(
        &self,
        query: &WebhookEventQuery,
    ) -> Result<Vec<WebhookEventRecord>, StorageError> {
        let sort_order = match query.sort_order {
            WebhookEventSortOrder::ReceivedAtAsc => "received_at_ms ASC, created_at_ms ASC",
            WebhookEventSortOrder::ReceivedAtDesc => "received_at_ms DESC, created_at_ms DESC",
        };
        let mut conditions = Vec::new();
        if let Some(source) = query
            .source
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            conditions.push(format!("source = '{}'", escape_sql_text(source)));
        }
        if let Some(event_type) = query
            .event_type
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            conditions.push(format!("event_type = '{}'", escape_sql_text(event_type)));
        }
        if let Some(session_key) = query
            .session_key
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            conditions.push(format!("session_key = '{}'", escape_sql_text(session_key)));
        }
        if let Some(status) = query.status {
            conditions.push(format!("status = '{}'", status.as_str()));
        }
        if let Some(from_ms) = query.received_from_ms {
            conditions.push(format!("received_at_ms >= {from_ms}"));
        }
        if let Some(to_ms) = query.received_to_ms {
            conditions.push(format!("received_at_ms <= {to_ms}"));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let sql = format!(
            "SELECT id, source, event_type, session_key, chat_id, sender_id, content,
                    payload_json, metadata_json, status, error_message, response_summary,
                    received_at_ms, processed_at_ms, remote_addr, created_at_ms
             FROM webhook_events
             {where_clause}
             ORDER BY {sort_order}
             LIMIT {} OFFSET {}",
            query.limit.max(1),
            query.offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_webhook_event(&row)?);
        }
        Ok(out)
    }

    async fn append_webhook_agent(
        &self,
        input: &NewWebhookAgentRecord,
    ) -> Result<WebhookAgentRecord, StorageError> {
        let now = now_ms();
        let sql = format!(
            "INSERT INTO webhook_agents (
                id, hook_id, session_key, chat_id, sender_id, content,
                payload_json, metadata_json, status, error_message, response_summary,
                received_at_ms, processed_at_ms, remote_addr, created_at_ms
            ) VALUES (
                '{}', '{}', '{}', '{}', '{}', '{}',
                {}, {}, '{}', {}, {}, {}, {}, {}, {}
            )",
            escape_sql_text(&input.id),
            escape_sql_text(&input.hook_id),
            escape_sql_text(&input.session_key),
            escape_sql_text(&input.chat_id),
            escape_sql_text(&input.sender_id),
            escape_sql_text(&input.content),
            opt_sql_text(input.payload_json.as_deref()),
            opt_sql_text(input.metadata_json.as_deref()),
            input.status.as_str(),
            opt_sql_text(input.error_message.as_deref()),
            opt_sql_text(input.response_summary.as_deref()),
            input.received_at_ms,
            opt_sql_i64(input.processed_at_ms),
            opt_sql_text(input.remote_addr.as_deref()),
            now
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;

        let mut rows = conn
            .query(
                &format!(
                    "SELECT id, hook_id, session_key, chat_id, sender_id, content,
                            payload_json, metadata_json, status, error_message, response_summary,
                            received_at_ms, processed_at_ms, remote_addr, created_at_ms
                     FROM webhook_agents
                     WHERE id = '{}'
                     LIMIT 1",
                    escape_sql_text(&input.id)
                ),
                (),
            )
            .await
            .map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("webhook agent not found"))?;
        row_to_webhook_agent(&row)
    }

    async fn update_webhook_agent_status(
        &self,
        event_id: &str,
        update: &UpdateWebhookAgentResult,
    ) -> Result<WebhookAgentRecord, StorageError> {
        let sql = format!(
            "UPDATE webhook_agents
             SET status = '{}', error_message = {}, response_summary = {}, processed_at_ms = {}
             WHERE id = '{}'",
            update.status.as_str(),
            opt_sql_text(update.error_message.as_deref()),
            opt_sql_text(update.response_summary.as_deref()),
            opt_sql_i64(update.processed_at_ms),
            escape_sql_text(event_id)
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;

        let mut rows = conn
            .query(
                &format!(
                    "SELECT id, hook_id, session_key, chat_id, sender_id, content,
                            payload_json, metadata_json, status, error_message, response_summary,
                            received_at_ms, processed_at_ms, remote_addr, created_at_ms
                     FROM webhook_agents
                     WHERE id = '{}'
                     LIMIT 1",
                    escape_sql_text(event_id)
                ),
                (),
            )
            .await
            .map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("webhook agent not found"))?;
        row_to_webhook_agent(&row)
    }

    async fn list_webhook_agents(
        &self,
        query: &WebhookAgentQuery,
    ) -> Result<Vec<WebhookAgentRecord>, StorageError> {
        let sort_order = match query.sort_order {
            WebhookEventSortOrder::ReceivedAtAsc => "received_at_ms ASC, created_at_ms ASC",
            WebhookEventSortOrder::ReceivedAtDesc => "received_at_ms DESC, created_at_ms DESC",
        };
        let mut conditions = Vec::new();
        if let Some(hook_id) = query
            .hook_id
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            conditions.push(format!("hook_id = '{}'", escape_sql_text(hook_id)));
        }
        if let Some(session_key) = query
            .session_key
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            conditions.push(format!("session_key = '{}'", escape_sql_text(session_key)));
        }
        if let Some(status) = query.status {
            conditions.push(format!("status = '{}'", status.as_str()));
        }
        if let Some(from_ms) = query.received_from_ms {
            conditions.push(format!("received_at_ms >= {from_ms}"));
        }
        if let Some(to_ms) = query.received_to_ms {
            conditions.push(format!("received_at_ms <= {to_ms}"));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let sql = format!(
            "SELECT id, hook_id, session_key, chat_id, sender_id, content,
                    payload_json, metadata_json, status, error_message, response_summary,
                    received_at_ms, processed_at_ms, remote_addr, created_at_ms
             FROM webhook_agents
             {where_clause}
             ORDER BY {sort_order}
             LIMIT {} OFFSET {}",
            query.limit.max(1),
            query.offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_webhook_agent(&row)?);
        }
        Ok(out)
    }

    async fn create_approval(
        &self,
        input: &NewApprovalRecord,
    ) -> Result<ApprovalRecord, StorageError> {
        let now = now_ms();
        let sql = format!(
            "INSERT INTO approvals (
                id, session_key, tool_name, command_hash, command_preview, command_text, risk_level, status,
                requested_by, approved_by, justification, expires_at_ms, created_at_ms, updated_at_ms, consumed_at_ms
            ) VALUES ('{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', NULL, {}, {}, {}, {}, NULL)",
            escape_sql_text(&input.id),
            escape_sql_text(&input.session_key),
            escape_sql_text(&input.tool_name),
            escape_sql_text(&input.command_hash),
            escape_sql_text(&input.command_preview),
            escape_sql_text(&input.command_text),
            escape_sql_text(&input.risk_level),
            ApprovalStatus::Pending.as_str(),
            escape_sql_text(&input.requested_by),
            input.justification
                .as_deref()
                .map(|value| format!("'{}'", escape_sql_text(value)))
                .unwrap_or_else(|| "NULL".to_string()),
            input.expires_at_ms,
            now,
            now
        );
        {
            let conn = self.connection().await?;
            conn.execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
        }
        self.get_approval(&input.id).await
    }

    async fn get_approval(&self, approval_id: &str) -> Result<ApprovalRecord, StorageError> {
        let sql = format!(
            "SELECT id, session_key, tool_name, command_hash, command_preview, risk_level, status,
                    command_text, requested_by, approved_by, justification, expires_at_ms, created_at_ms, updated_at_ms, consumed_at_ms
             FROM approvals
             WHERE id = '{}'
             LIMIT 1",
            escape_sql_text(approval_id)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("approval not found"))?;
        row_to_approval(&row)
    }

    async fn update_approval_status(
        &self,
        approval_id: &str,
        status: ApprovalStatus,
        approved_by: Option<&str>,
    ) -> Result<ApprovalRecord, StorageError> {
        let sql = format!(
            "UPDATE approvals
             SET status = '{}',
                 approved_by = {},
                 updated_at_ms = {}
             WHERE id = '{}'",
            status.as_str(),
            approved_by
                .map(|value| format!("'{}'", escape_sql_text(value)))
                .unwrap_or_else(|| "NULL".to_string()),
            now_ms(),
            escape_sql_text(approval_id)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                return Err(StorageError::backend(format!(
                    "approval '{approval_id}' not found when setting status"
                )));
            }
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
        let sql = format!(
            "UPDATE approvals
             SET status = '{}',
                 consumed_at_ms = {},
                 updated_at_ms = {}
             WHERE id = '{}'
               AND tool_name = '{}'
               AND session_key = '{}'
               AND command_hash = '{}'
               AND status = '{}'
               AND consumed_at_ms IS NULL
               AND expires_at_ms >= {}",
            ApprovalStatus::Consumed.as_str(),
            now_ms,
            now_ms,
            escape_sql_text(approval_id),
            escape_sql_text(tool_name),
            escape_sql_text(session_key),
            escape_sql_text(command_hash),
            ApprovalStatus::Approved.as_str(),
            now_ms
        );
        let conn = self.connection().await?;
        let affected = conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        Ok(affected > 0)
    }

    async fn consume_latest_approved_tool_command(
        &self,
        tool_name: &str,
        session_key: &str,
        command_hash: &str,
        now_ms: i64,
    ) -> Result<bool, StorageError> {
        let sql = format!(
            "SELECT id
             FROM approvals
             WHERE tool_name = '{}'
               AND session_key = '{}'
               AND command_hash = '{}'
               AND status = '{}'
               AND consumed_at_ms IS NULL
               AND expires_at_ms >= {}
             ORDER BY created_at_ms DESC
             LIMIT 1",
            escape_sql_text(tool_name),
            escape_sql_text(session_key),
            escape_sql_text(command_hash),
            ApprovalStatus::Approved.as_str(),
            now_ms
        );
        let approval_id = {
            let conn = self.connection().await?;
            let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
            let Some(row) = rows.next().await.map_err(StorageError::backend)? else {
                return Ok(false);
            };
            value_to_string(row.get_value(0).map_err(StorageError::backend)?)?
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
        let sql = format!(
            "INSERT INTO pending_questions (
                id, session_key, channel, chat_id, title, question_text, options_json, status,
                selected_option_id, answered_by, expires_at_ms, created_at_ms, updated_at_ms, answered_at_ms
            ) VALUES ('{}', '{}', '{}', '{}', {}, '{}', '{}', '{}', NULL, NULL, {}, {}, {}, NULL)",
            escape_sql_text(&input.id),
            escape_sql_text(&input.session_key),
            escape_sql_text(&input.channel),
            escape_sql_text(&input.chat_id),
            input.title
                .as_deref()
                .map(|value| format!("'{}'", escape_sql_text(value)))
                .unwrap_or_else(|| "NULL".to_string()),
            escape_sql_text(&input.question_text),
            escape_sql_text(&input.options_json),
            PendingQuestionStatus::Pending.as_str(),
            input.expires_at_ms,
            now,
            now
        );
        {
            let conn = self.connection().await?;
            conn.execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
        }
        self.get_pending_question(&input.id).await
    }

    async fn get_pending_question(
        &self,
        question_id: &str,
    ) -> Result<PendingQuestionRecord, StorageError> {
        let sql = format!(
            "SELECT id, session_key, channel, chat_id, title, question_text, options_json, status,
                    selected_option_id, answered_by, expires_at_ms, created_at_ms, updated_at_ms, answered_at_ms
             FROM pending_questions
             WHERE id = '{}'
             LIMIT 1",
            escape_sql_text(question_id)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("pending question not found"))?;
        row_to_pending_question(&row)
    }

    async fn update_pending_question_answer(
        &self,
        question_id: &str,
        status: PendingQuestionStatus,
        selected_option_id: Option<&str>,
        answered_by: Option<&str>,
        answered_at_ms: Option<i64>,
    ) -> Result<PendingQuestionRecord, StorageError> {
        let sql = format!(
            "UPDATE pending_questions
             SET status = '{}',
                 selected_option_id = {},
                 answered_by = {},
                 answered_at_ms = {},
                 updated_at_ms = {}
             WHERE id = '{}'",
            status.as_str(),
            selected_option_id
                .map(|value| format!("'{}'", escape_sql_text(value)))
                .unwrap_or_else(|| "NULL".to_string()),
            answered_by
                .map(|value| format!("'{}'", escape_sql_text(value)))
                .unwrap_or_else(|| "NULL".to_string()),
            answered_at_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "NULL".to_string()),
            now_ms(),
            escape_sql_text(question_id)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                return Err(StorageError::backend(format!(
                    "pending question '{question_id}' not found when updating answer"
                )));
            }
        }
        self.get_pending_question(question_id).await
    }

    fn session_jsonl_path(&self, session_key: &str) -> PathBuf {
        jsonl::session_jsonl_path(&self.paths, session_key)
    }
}
