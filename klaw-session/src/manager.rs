use crate::SessionError;
use async_trait::async_trait;
use klaw_storage::{
    ChatRecord, DefaultSessionStore, LlmAuditFilterOptions, LlmAuditFilterOptionsQuery,
    LlmAuditQuery, LlmAuditRecord, LlmAuditSummaryRecord, LlmUsageRecord, LlmUsageSummary,
    NewLlmAuditRecord, NewLlmUsageRecord, NewToolAuditRecord, NewWebhookAgentRecord,
    NewWebhookEventRecord, SessionCompressionState, SessionIndex, SessionSortOrder, SessionStorage,
    ToolAuditFilterOptions, ToolAuditFilterOptionsQuery, ToolAuditQuery, ToolAuditRecord,
    UpdateWebhookAgentResult, UpdateWebhookEventResult, WebhookAgentQuery, WebhookAgentRecord,
    WebhookEventQuery, WebhookEventRecord, open_default_store,
};

#[derive(Debug, Clone)]
pub struct SessionListQuery {
    pub limit: i64,
    pub offset: i64,
    pub updated_from_ms: Option<i64>,
    pub updated_to_ms: Option<i64>,
    pub channel: Option<String>,
    pub sort_order: SessionSortOrder,
}

impl Default for SessionListQuery {
    fn default() -> Self {
        Self {
            limit: 100,
            offset: 0,
            updated_from_ms: None,
            updated_to_ms: None,
            channel: None,
            sort_order: SessionSortOrder::UpdatedAtDesc,
        }
    }
}

#[async_trait]
pub trait SessionManager: Send + Sync {
    async fn touch_session(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, SessionError>;

    async fn complete_turn(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, SessionError>;

    async fn append_chat_record(
        &self,
        session_key: &str,
        record: &ChatRecord,
    ) -> Result<(), SessionError>;

    async fn read_chat_records(&self, session_key: &str) -> Result<Vec<ChatRecord>, SessionError>;

    async fn get_session(&self, session_key: &str) -> Result<SessionIndex, SessionError>;

    async fn set_session_title(
        &self,
        session_key: &str,
        title: Option<&str>,
    ) -> Result<SessionIndex, SessionError>;

    async fn delete_session(&self, session_key: &str) -> Result<bool, SessionError>;

    async fn get_or_create_session_state(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        default_provider: &str,
        default_model: &str,
    ) -> Result<SessionIndex, SessionError>;

    async fn set_active_session(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        active_session_key: &str,
    ) -> Result<SessionIndex, SessionError>;

    async fn set_model_provider(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        model_provider: &str,
        model: &str,
    ) -> Result<SessionIndex, SessionError>;

    async fn set_model(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        model: &str,
    ) -> Result<SessionIndex, SessionError>;

    async fn set_delivery_metadata(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        delivery_metadata_json: Option<&str>,
    ) -> Result<SessionIndex, SessionError>;

    async fn clear_model_routing_override(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, SessionError>;

    async fn get_session_compression_state(
        &self,
        session_key: &str,
    ) -> Result<Option<SessionCompressionState>, SessionError>;

    async fn set_session_compression_state(
        &self,
        session_key: &str,
        state: &SessionCompressionState,
    ) -> Result<(), SessionError>;

    async fn list_sessions(
        &self,
        query: SessionListQuery,
    ) -> Result<Vec<SessionIndex>, SessionError>;

    async fn list_session_channels(&self) -> Result<Vec<String>, SessionError>;

    async fn append_llm_usage(
        &self,
        input: &NewLlmUsageRecord,
    ) -> Result<LlmUsageRecord, SessionError>;

    async fn list_llm_usage(
        &self,
        session_key: &str,
        query: SessionListQuery,
    ) -> Result<Vec<LlmUsageRecord>, SessionError>;

    async fn sum_llm_usage_by_session(
        &self,
        session_key: &str,
    ) -> Result<LlmUsageSummary, SessionError>;

    async fn sum_llm_usage_by_turn(
        &self,
        session_key: &str,
        turn_index: i64,
    ) -> Result<LlmUsageSummary, SessionError>;

    async fn append_llm_audit(
        &self,
        input: &NewLlmAuditRecord,
    ) -> Result<LlmAuditRecord, SessionError>;

    async fn list_llm_audit(
        &self,
        query: &LlmAuditQuery,
    ) -> Result<Vec<LlmAuditRecord>, SessionError>;

    async fn get_llm_audit(&self, audit_id: &str) -> Result<LlmAuditRecord, SessionError>;

    async fn list_llm_audit_summaries(
        &self,
        query: &LlmAuditQuery,
    ) -> Result<Vec<LlmAuditSummaryRecord>, SessionError>;

    async fn list_llm_audit_filter_options(
        &self,
        query: &LlmAuditFilterOptionsQuery,
    ) -> Result<LlmAuditFilterOptions, SessionError>;

    async fn append_tool_audit(
        &self,
        input: &NewToolAuditRecord,
    ) -> Result<ToolAuditRecord, SessionError>;

    async fn list_tool_audit(
        &self,
        query: &ToolAuditQuery,
    ) -> Result<Vec<ToolAuditRecord>, SessionError>;

    async fn list_tool_audit_filter_options(
        &self,
        query: &ToolAuditFilterOptionsQuery,
    ) -> Result<ToolAuditFilterOptions, SessionError>;

    async fn append_webhook_event(
        &self,
        input: &NewWebhookEventRecord,
    ) -> Result<WebhookEventRecord, SessionError>;

    async fn update_webhook_event_status(
        &self,
        event_id: &str,
        update: &UpdateWebhookEventResult,
    ) -> Result<WebhookEventRecord, SessionError>;

    async fn list_webhook_events(
        &self,
        query: &WebhookEventQuery,
    ) -> Result<Vec<WebhookEventRecord>, SessionError>;

    async fn append_webhook_agent(
        &self,
        input: &NewWebhookAgentRecord,
    ) -> Result<WebhookAgentRecord, SessionError>;

    async fn update_webhook_agent_status(
        &self,
        event_id: &str,
        update: &UpdateWebhookAgentResult,
    ) -> Result<WebhookAgentRecord, SessionError>;

    async fn list_webhook_agents(
        &self,
        query: &WebhookAgentQuery,
    ) -> Result<Vec<WebhookAgentRecord>, SessionError>;
}

pub struct SqliteSessionManager {
    store: DefaultSessionStore,
}

impl SqliteSessionManager {
    pub async fn open_default() -> Result<Self, SessionError> {
        let store = open_default_store().await?;
        Ok(Self { store })
    }

    pub fn from_store(store: DefaultSessionStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl SessionManager for SqliteSessionManager {
    async fn touch_session(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, SessionError> {
        Ok(self
            .store
            .touch_session(session_key, chat_id, channel)
            .await?)
    }

    async fn complete_turn(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, SessionError> {
        Ok(self
            .store
            .complete_turn(session_key, chat_id, channel)
            .await?)
    }

    async fn append_chat_record(
        &self,
        session_key: &str,
        record: &ChatRecord,
    ) -> Result<(), SessionError> {
        Ok(self.store.append_chat_record(session_key, record).await?)
    }

    async fn read_chat_records(&self, session_key: &str) -> Result<Vec<ChatRecord>, SessionError> {
        Ok(self.store.read_chat_records(session_key).await?)
    }

    async fn get_session(&self, session_key: &str) -> Result<SessionIndex, SessionError> {
        Ok(self.store.get_session(session_key).await?)
    }

    async fn set_session_title(
        &self,
        session_key: &str,
        title: Option<&str>,
    ) -> Result<SessionIndex, SessionError> {
        Ok(self.store.set_session_title(session_key, title).await?)
    }

    async fn delete_session(&self, session_key: &str) -> Result<bool, SessionError> {
        Ok(self.store.delete_session(session_key).await?)
    }

    async fn get_or_create_session_state(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        default_provider: &str,
        default_model: &str,
    ) -> Result<SessionIndex, SessionError> {
        Ok(self
            .store
            .get_or_create_session_state(
                session_key,
                chat_id,
                channel,
                default_provider,
                default_model,
            )
            .await?)
    }

    async fn set_active_session(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        active_session_key: &str,
    ) -> Result<SessionIndex, SessionError> {
        Ok(self
            .store
            .set_active_session(session_key, chat_id, channel, active_session_key)
            .await?)
    }

    async fn set_model_provider(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        model_provider: &str,
        model: &str,
    ) -> Result<SessionIndex, SessionError> {
        Ok(self
            .store
            .set_model_provider(session_key, chat_id, channel, model_provider, model)
            .await?)
    }

    async fn set_model(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        model: &str,
    ) -> Result<SessionIndex, SessionError> {
        Ok(self
            .store
            .set_model(session_key, chat_id, channel, model)
            .await?)
    }

    async fn set_delivery_metadata(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        delivery_metadata_json: Option<&str>,
    ) -> Result<SessionIndex, SessionError> {
        Ok(self
            .store
            .set_delivery_metadata(session_key, chat_id, channel, delivery_metadata_json)
            .await?)
    }

    async fn clear_model_routing_override(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, SessionError> {
        Ok(self
            .store
            .clear_model_routing_override(session_key, chat_id, channel)
            .await?)
    }

    async fn get_session_compression_state(
        &self,
        session_key: &str,
    ) -> Result<Option<SessionCompressionState>, SessionError> {
        Ok(self
            .store
            .get_session_compression_state(session_key)
            .await?)
    }

    async fn set_session_compression_state(
        &self,
        session_key: &str,
        state: &SessionCompressionState,
    ) -> Result<(), SessionError> {
        Ok(self
            .store
            .set_session_compression_state(session_key, state)
            .await?)
    }

    async fn list_sessions(
        &self,
        query: SessionListQuery,
    ) -> Result<Vec<SessionIndex>, SessionError> {
        let limit = query.limit.max(1);
        let offset = query.offset.max(0);
        Ok(self
            .store
            .list_sessions(
                limit,
                offset,
                query.updated_from_ms,
                query.updated_to_ms,
                query.channel.as_deref(),
                query.sort_order,
            )
            .await?)
    }

    async fn list_session_channels(&self) -> Result<Vec<String>, SessionError> {
        Ok(self.store.list_session_channels().await?)
    }

    async fn append_llm_usage(
        &self,
        input: &NewLlmUsageRecord,
    ) -> Result<LlmUsageRecord, SessionError> {
        Ok(self.store.append_llm_usage(input).await?)
    }

    async fn list_llm_usage(
        &self,
        session_key: &str,
        query: SessionListQuery,
    ) -> Result<Vec<LlmUsageRecord>, SessionError> {
        let limit = query.limit.max(1);
        let offset = query.offset.max(0);
        Ok(self
            .store
            .list_llm_usage(session_key, limit, offset)
            .await?)
    }

    async fn sum_llm_usage_by_session(
        &self,
        session_key: &str,
    ) -> Result<LlmUsageSummary, SessionError> {
        Ok(self.store.sum_llm_usage_by_session(session_key).await?)
    }

    async fn sum_llm_usage_by_turn(
        &self,
        session_key: &str,
        turn_index: i64,
    ) -> Result<LlmUsageSummary, SessionError> {
        Ok(self
            .store
            .sum_llm_usage_by_turn(session_key, turn_index)
            .await?)
    }

    async fn append_llm_audit(
        &self,
        input: &NewLlmAuditRecord,
    ) -> Result<LlmAuditRecord, SessionError> {
        Ok(self.store.append_llm_audit(input).await?)
    }

    async fn list_llm_audit(
        &self,
        query: &LlmAuditQuery,
    ) -> Result<Vec<LlmAuditRecord>, SessionError> {
        Ok(self.store.list_llm_audit(query).await?)
    }

    async fn get_llm_audit(&self, audit_id: &str) -> Result<LlmAuditRecord, SessionError> {
        Ok(self.store.get_llm_audit(audit_id).await?)
    }

    async fn list_llm_audit_summaries(
        &self,
        query: &LlmAuditQuery,
    ) -> Result<Vec<LlmAuditSummaryRecord>, SessionError> {
        Ok(self.store.list_llm_audit_summaries(query).await?)
    }

    async fn list_llm_audit_filter_options(
        &self,
        query: &LlmAuditFilterOptionsQuery,
    ) -> Result<LlmAuditFilterOptions, SessionError> {
        Ok(self.store.list_llm_audit_filter_options(query).await?)
    }

    async fn append_tool_audit(
        &self,
        input: &NewToolAuditRecord,
    ) -> Result<ToolAuditRecord, SessionError> {
        Ok(self.store.append_tool_audit(input).await?)
    }

    async fn list_tool_audit(
        &self,
        query: &ToolAuditQuery,
    ) -> Result<Vec<ToolAuditRecord>, SessionError> {
        Ok(self.store.list_tool_audit(query).await?)
    }

    async fn list_tool_audit_filter_options(
        &self,
        query: &ToolAuditFilterOptionsQuery,
    ) -> Result<ToolAuditFilterOptions, SessionError> {
        Ok(self.store.list_tool_audit_filter_options(query).await?)
    }

    async fn append_webhook_event(
        &self,
        input: &NewWebhookEventRecord,
    ) -> Result<WebhookEventRecord, SessionError> {
        Ok(self.store.append_webhook_event(input).await?)
    }

    async fn update_webhook_event_status(
        &self,
        event_id: &str,
        update: &UpdateWebhookEventResult,
    ) -> Result<WebhookEventRecord, SessionError> {
        Ok(self
            .store
            .update_webhook_event_status(event_id, update)
            .await?)
    }

    async fn list_webhook_events(
        &self,
        query: &WebhookEventQuery,
    ) -> Result<Vec<WebhookEventRecord>, SessionError> {
        Ok(self.store.list_webhook_events(query).await?)
    }

    async fn append_webhook_agent(
        &self,
        input: &NewWebhookAgentRecord,
    ) -> Result<WebhookAgentRecord, SessionError> {
        Ok(self.store.append_webhook_agent(input).await?)
    }

    async fn update_webhook_agent_status(
        &self,
        event_id: &str,
        update: &UpdateWebhookAgentResult,
    ) -> Result<WebhookAgentRecord, SessionError> {
        Ok(self
            .store
            .update_webhook_agent_status(event_id, update)
            .await?)
    }

    async fn list_webhook_agents(
        &self,
        query: &WebhookAgentQuery,
    ) -> Result<Vec<WebhookAgentRecord>, SessionError> {
        Ok(self.store.list_webhook_agents(query).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionListQuery, SessionManager, SqliteSessionManager};
    use crate::SessionSortOrder;
    use klaw_storage::{ChatRecord, DefaultSessionStore, SessionStorage, StoragePaths};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn create_store() -> DefaultSessionStore {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("klaw-session-test-{now_ms}-{suffix}"));
        DefaultSessionStore::open(StoragePaths::from_root(root))
            .await
            .expect("session store should open")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_sessions_returns_latest_first() {
        let store = create_store().await;
        let _ = store
            .touch_session("terminal:first", "chat-1", "terminal")
            .await
            .expect("first session should be created");
        let _ = store
            .touch_session("terminal:second", "chat-2", "terminal")
            .await
            .expect("second session should be created");

        let manager = SqliteSessionManager::from_store(store);
        let sessions = manager
            .list_sessions(SessionListQuery {
                limit: 10,
                offset: 0,
                updated_from_ms: None,
                updated_to_ms: None,
                channel: None,
                sort_order: SessionSortOrder::UpdatedAtDesc,
            })
            .await
            .expect("sessions should load");

        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_key, "terminal:second");
        assert_eq!(sessions[1].session_key, "terminal:first");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_sessions_clamps_limit_and_offset() {
        let store = create_store().await;
        let _ = store
            .touch_session("terminal:only", "chat-1", "terminal")
            .await
            .expect("session should be created");

        let manager = SqliteSessionManager::from_store(store);
        let sessions = manager
            .list_sessions(SessionListQuery {
                limit: 0,
                offset: -5,
                updated_from_ms: None,
                updated_to_ms: None,
                channel: None,
                sort_order: SessionSortOrder::UpdatedAtDesc,
            })
            .await
            .expect("sessions should load");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_key, "terminal:only");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_sessions_filters_by_channel_and_sorts_in_sql_order() {
        let store = create_store().await;
        let _ = store
            .touch_session("terminal:first", "chat-1", "terminal")
            .await
            .expect("terminal session should be created");
        let _ = store
            .touch_session("telegram:second", "chat-2", "telegram")
            .await
            .expect("telegram session should be created");

        let manager = SqliteSessionManager::from_store(store);
        let sessions = manager
            .list_sessions(SessionListQuery {
                limit: 10,
                offset: 0,
                updated_from_ms: None,
                updated_to_ms: None,
                channel: Some("terminal".to_string()),
                sort_order: SessionSortOrder::UpdatedAtAsc,
            })
            .await
            .expect("filtered sessions should load");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_key, "terminal:first");

        let channels = manager
            .list_session_channels()
            .await
            .expect("session channels should load");
        assert_eq!(
            channels,
            vec!["telegram".to_string(), "terminal".to_string()]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn manager_reads_and_updates_session_state() {
        let store = create_store().await;
        let manager = SqliteSessionManager::from_store(store);

        let base = manager
            .get_or_create_session_state(
                "terminal:base",
                "chat-1",
                "terminal",
                "openai",
                "gpt-4o-mini",
            )
            .await
            .expect("base session should be created");
        assert_eq!(base.active_session_key.as_deref(), Some("terminal:base"));

        let _ = manager
            .set_model_provider(
                "terminal:base",
                "chat-1",
                "terminal",
                "anthropic",
                "claude-3-7-sonnet",
            )
            .await
            .expect("provider should update");
        let updated = manager
            .set_model("terminal:base", "chat-1", "terminal", "claude-3-7-opus")
            .await
            .expect("model should update");
        assert_eq!(updated.model.as_deref(), Some("claude-3-7-opus"));

        manager
            .append_chat_record("terminal:base", &ChatRecord::new("user", "hello", None))
            .await
            .expect("chat record should append");
        let records = manager
            .read_chat_records("terminal:base")
            .await
            .expect("chat records should load");
        assert_eq!(records.len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn manager_persists_session_title_updates() {
        let store = create_store().await;
        let manager = SqliteSessionManager::from_store(store);
        manager
            .touch_session("web:test", "chat-1", "web")
            .await
            .expect("session should be created");

        let updated = manager
            .set_session_title("web:test", Some("Renamed agent"))
            .await
            .expect("title should update");
        assert_eq!(updated.title.as_deref(), Some("Renamed agent"));

        let reloaded = manager
            .get_session("web:test")
            .await
            .expect("session should reload");
        assert_eq!(reloaded.title.as_deref(), Some("Renamed agent"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn manager_deletes_session_and_history() {
        let store = create_store().await;
        let manager = SqliteSessionManager::from_store(store);
        manager
            .touch_session("web:delete-me", "chat-1", "web")
            .await
            .expect("session should be created");
        manager
            .append_chat_record("web:delete-me", &ChatRecord::new("user", "hello", None))
            .await
            .expect("history should append");

        let deleted = manager
            .delete_session("web:delete-me")
            .await
            .expect("delete should succeed");
        assert!(deleted);
        assert!(
            manager
                .read_chat_records("web:delete-me")
                .await
                .expect("history should load")
                .is_empty()
        );
        assert!(manager.get_session("web:delete-me").await.is_err());
    }
}
