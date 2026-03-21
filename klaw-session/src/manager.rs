use crate::SessionError;
use async_trait::async_trait;
use klaw_storage::{
    open_default_store, ChatRecord, DefaultSessionStore, LlmAuditQuery, LlmAuditRecord,
    LlmUsageRecord, LlmUsageSummary, NewLlmAuditRecord, NewLlmUsageRecord, NewWebhookEventRecord,
    SessionCompressionState, SessionIndex, SessionStorage, UpdateWebhookEventResult,
    WebhookEventQuery, WebhookEventRecord,
};

#[derive(Debug, Clone, Copy)]
pub struct SessionListQuery {
    pub limit: i64,
    pub offset: i64,
}

impl Default for SessionListQuery {
    fn default() -> Self {
        Self {
            limit: 100,
            offset: 0,
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
        Ok(self.store.list_sessions(limit, offset).await?)
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
}

#[cfg(test)]
mod tests {
    use super::{SessionListQuery, SessionManager, SqliteSessionManager};
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
            .touch_session("stdio:first", "chat-1", "stdio")
            .await
            .expect("first session should be created");
        let _ = store
            .touch_session("stdio:second", "chat-2", "stdio")
            .await
            .expect("second session should be created");

        let manager = SqliteSessionManager::from_store(store);
        let sessions = manager
            .list_sessions(SessionListQuery {
                limit: 10,
                offset: 0,
            })
            .await
            .expect("sessions should load");

        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_key, "stdio:second");
        assert_eq!(sessions[1].session_key, "stdio:first");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_sessions_clamps_limit_and_offset() {
        let store = create_store().await;
        let _ = store
            .touch_session("stdio:only", "chat-1", "stdio")
            .await
            .expect("session should be created");

        let manager = SqliteSessionManager::from_store(store);
        let sessions = manager
            .list_sessions(SessionListQuery {
                limit: 0,
                offset: -5,
            })
            .await
            .expect("sessions should load");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_key, "stdio:only");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn manager_reads_and_updates_session_state() {
        let store = create_store().await;
        let manager = SqliteSessionManager::from_store(store);

        let base = manager
            .get_or_create_session_state("stdio:base", "chat-1", "stdio", "openai", "gpt-4o-mini")
            .await
            .expect("base session should be created");
        assert_eq!(base.active_session_key.as_deref(), Some("stdio:base"));

        let _ = manager
            .set_model_provider(
                "stdio:base",
                "chat-1",
                "stdio",
                "anthropic",
                "claude-3-7-sonnet",
            )
            .await
            .expect("provider should update");
        let updated = manager
            .set_model("stdio:base", "chat-1", "stdio", "claude-3-7-opus")
            .await
            .expect("model should update");
        assert_eq!(updated.model.as_deref(), Some("claude-3-7-opus"));

        manager
            .append_chat_record("stdio:base", &ChatRecord::new("user", "hello", None))
            .await
            .expect("chat record should append");
        let records = manager
            .read_chat_records("stdio:base")
            .await
            .expect("chat records should load");
        assert_eq!(records.len(), 1);
    }
}
