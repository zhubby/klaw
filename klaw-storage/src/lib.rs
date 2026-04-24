mod backup;
mod error;
mod jsonl;
mod memory_db;
mod paths;
mod traits;
mod types;
mod util;

#[cfg(feature = "sqlx")]
pub mod sqlx;
#[cfg(feature = "turso")]
pub mod turso;

pub use backup::{
    BackupItem, BackupPlan, BackupProgress, BackupProgressStage, BackupResult, BackupService,
    DatabaseSnapshotExporter, LatestRef, ManifestEntry, ManifestEntryKind, S3SnapshotStoreConfig,
    SnapshotListItem, SnapshotMode, SnapshotPrepareResult, SnapshotRestoreResult, SnapshotSchedule,
    SnapshotStore, SyncManifest,
};
pub use error::StorageError;
pub use memory_db::{DbRow, DbValue, MemoryDb};
pub use paths::StoragePaths;
pub use traits::{ChatRecordPage, CronStorage, HeartbeatStorage, SessionStorage};
pub use types::{
    ApprovalRecord, ApprovalStatus, ChatRecord, CronJob, CronScheduleKind, CronTaskRun,
    CronTaskStatus, HeartbeatJob, HeartbeatTaskRun, HeartbeatTaskStatus, LlmAuditFilterOptions,
    LlmAuditFilterOptionsQuery, LlmAuditQuery, LlmAuditRecord, LlmAuditSortOrder, LlmAuditStatus,
    LlmAuditSummaryRecord, LlmUsageRecord, LlmUsageSource, LlmUsageSummary, NewApprovalRecord,
    NewCronJob, NewCronTaskRun, NewHeartbeatJob, NewHeartbeatTaskRun, NewLlmAuditRecord,
    NewLlmUsageRecord, NewPendingQuestionRecord, NewToolAuditRecord, NewWebhookAgentRecord,
    NewWebhookEventRecord, PendingQuestionRecord, PendingQuestionStatus, SessionCompressionState,
    SessionIndex, SessionSortOrder, ToolAuditFilterOptions, ToolAuditFilterOptionsQuery,
    ToolAuditQuery, ToolAuditRecord, ToolAuditSortOrder, ToolAuditStatus, UpdateCronJobPatch,
    UpdateHeartbeatJobPatch, UpdateWebhookAgentResult, UpdateWebhookEventResult, WebhookAgentQuery,
    WebhookAgentRecord, WebhookEventQuery, WebhookEventRecord, WebhookEventSortOrder,
    WebhookEventStatus,
};

#[cfg(all(feature = "turso", feature = "sqlx"))]
compile_error!("features `turso` and `sqlx` are mutually exclusive; enable only one backend");

#[cfg(not(any(feature = "turso", feature = "sqlx")))]
compile_error!("enable one backend feature: `turso` or `sqlx`");

#[cfg(all(feature = "turso", not(feature = "sqlx")))]
pub type DefaultSessionStore = turso::TursoSessionStore;
#[cfg(all(feature = "sqlx", not(feature = "turso")))]
pub type DefaultSessionStore = sqlx::SqlxSessionStore;
#[cfg(all(feature = "turso", not(feature = "sqlx")))]
pub type DefaultMemoryDb = turso::TursoMemoryDb;
#[cfg(all(feature = "sqlx", not(feature = "turso")))]
pub type DefaultMemoryDb = sqlx::SqlxMemoryDb;
#[cfg(all(feature = "turso", not(feature = "sqlx")))]
pub type DefaultArchiveDb = turso::TursoArchiveDb;
#[cfg(all(feature = "sqlx", not(feature = "turso")))]
pub type DefaultArchiveDb = sqlx::SqlxArchiveDb;

#[cfg(all(feature = "turso", not(feature = "sqlx")))]
pub async fn open_default_store() -> Result<DefaultSessionStore, StorageError> {
    let paths = StoragePaths::from_home_dir()?;
    DefaultSessionStore::open(paths).await
}

#[cfg(all(feature = "sqlx", not(feature = "turso")))]
pub async fn open_default_store() -> Result<DefaultSessionStore, StorageError> {
    let paths = StoragePaths::from_home_dir()?;
    DefaultSessionStore::open(paths).await
}

#[cfg(all(feature = "turso", not(feature = "sqlx")))]
pub async fn open_default_memory_db() -> Result<DefaultMemoryDb, StorageError> {
    let paths = StoragePaths::from_home_dir()?;
    DefaultMemoryDb::open(paths).await
}

#[cfg(all(feature = "sqlx", not(feature = "turso")))]
pub async fn open_default_memory_db() -> Result<DefaultMemoryDb, StorageError> {
    let paths = StoragePaths::from_home_dir()?;
    DefaultMemoryDb::open(paths).await
}

#[cfg(all(feature = "turso", not(feature = "sqlx")))]
pub async fn open_default_archive_db() -> Result<DefaultArchiveDb, StorageError> {
    let paths = StoragePaths::from_home_dir()?;
    DefaultArchiveDb::open(paths).await
}

#[cfg(all(feature = "sqlx", not(feature = "turso")))]
pub async fn open_default_archive_db() -> Result<DefaultArchiveDb, StorageError> {
    let paths = StoragePaths::from_home_dir()?;
    DefaultArchiveDb::open(paths).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::fs;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn create_store() -> DefaultSessionStore {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("klaw-storage-test-{}-{suffix}", util::now_ms()));
        DefaultSessionStore::open(StoragePaths::from_root(base))
            .await
            .expect("session store should open")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn touch_does_not_increase_turn_count() {
        let store = create_store().await;
        let first = store
            .touch_session("terminal:test1", "test1", "terminal")
            .await
            .expect("touch should succeed");
        assert_eq!(first.turn_count, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn complete_turn_increments_only_on_response() {
        let store = create_store().await;
        let _ = store
            .touch_session("terminal:test2", "test2", "terminal")
            .await
            .expect("touch should succeed");
        let completed = store
            .complete_turn("terminal:test2", "test2", "terminal")
            .await
            .expect("complete turn should succeed");
        assert_eq!(completed.turn_count, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn append_chat_record_writes_jsonl() {
        let store = create_store().await;
        let record = ChatRecord::new("user", "hello", Some("m1".to_string()))
            .with_metadata_json(Some("{\"im.card\":true}".to_string()));
        store
            .append_chat_record("terminal:test3", &record)
            .await
            .expect("append should succeed");

        let file_path = store.session_jsonl_path("terminal:test3");
        let contents = fs::read_to_string(file_path)
            .await
            .expect("jsonl file should exist");
        assert!(contents.contains("\"role\":\"user\""));
        assert!(contents.contains("\"content\":\"hello\""));
        assert!(contents.contains("\"metadata_json\":\"{\\\"im.card\\\":true}\""));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_chat_records_returns_ordered_history() {
        let store = create_store().await;
        store
            .append_chat_record(
                "terminal:test-history",
                &ChatRecord::new("user", "hello", Some("m1".to_string()))
                    .with_metadata_json(Some("{\"kind\":\"plain\"}".to_string())),
            )
            .await
            .expect("first append should succeed");
        store
            .append_chat_record(
                "terminal:test-history",
                &ChatRecord::new("assistant", "world", Some("m2".to_string())),
            )
            .await
            .expect("second append should succeed");

        let records = store
            .read_chat_records("terminal:test-history")
            .await
            .expect("history read should succeed");
        let summary: Vec<(&str, &str)> = records
            .iter()
            .map(|record| (record.role.as_str(), record.content.as_str()))
            .collect();
        assert_eq!(summary, vec![("user", "hello"), ("assistant", "world")]);
        assert_eq!(
            records[0].metadata_json.as_deref(),
            Some("{\"kind\":\"plain\"}")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn session_title_updates_persist_across_reads() {
        let store = create_store().await;
        store
            .touch_session("websocket:title-test", "chat-1", "websocket")
            .await
            .expect("session should be created");

        let renamed = store
            .set_session_title("websocket:title-test", Some("Saved name"))
            .await
            .expect("title should update");
        assert_eq!(renamed.title.as_deref(), Some("Saved name"));

        let fetched = store
            .get_session("websocket:title-test")
            .await
            .expect("session should reload");
        assert_eq!(fetched.title.as_deref(), Some("Saved name"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn delete_session_removes_index_and_history_file() {
        let store = create_store().await;
        store
            .touch_session("websocket:delete-test", "chat-1", "websocket")
            .await
            .expect("session should be created");
        store
            .append_chat_record(
                "websocket:delete-test",
                &ChatRecord::new("user", "hello", Some("m1".to_string())),
            )
            .await
            .expect("history should append");

        let deleted = store
            .delete_session("websocket:delete-test")
            .await
            .expect("delete should succeed");
        assert!(deleted);
        assert!(
            store
                .read_chat_records("websocket:delete-test")
                .await
                .expect("history should read")
                .is_empty()
        );
        assert!(store.get_session("websocket:delete-test").await.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_sessions_supports_channel_filter_and_sort_order() {
        let store = create_store().await;
        let _ = store
            .touch_session("telegram:chat-1", "chat-1", "telegram")
            .await
            .expect("telegram session should be created");
        let _ = store
            .touch_session("terminal:chat-2", "chat-2", "terminal")
            .await
            .expect("terminal session should be created");

        let filtered = store
            .list_sessions(
                Some(10),
                0,
                None,
                None,
                Some("terminal"),
                None,
                SessionSortOrder::UpdatedAtAsc,
            )
            .await
            .expect("filtered sessions should load");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].session_key, "terminal:chat-2");

        let channels = store
            .list_session_channels()
            .await
            .expect("channel list should load");
        assert_eq!(
            channels,
            vec!["telegram".to_string(), "terminal".to_string()]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn session_route_state_persists_provider_model_and_active_session() {
        let store = create_store().await;
        let base = store
            .get_or_create_session_state(
                "dingtalk:acc:chat-1",
                "chat-1",
                "dingtalk",
                "openai",
                "gpt-4o-mini",
            )
            .await
            .expect("base session should be created");
        assert_eq!(
            base.active_session_key.as_deref(),
            Some("dingtalk:acc:chat-1")
        );
        assert_eq!(base.model_provider, None);
        assert!(!base.model_provider_explicit);
        assert_eq!(base.model, None);
        assert!(!base.model_explicit);

        let _new_active = store
            .get_or_create_session_state(
                "dingtalk:acc:chat-1:child",
                "chat-1",
                "dingtalk",
                "openai",
                "gpt-4o-mini",
            )
            .await
            .expect("child session should be created");
        let updated_base = store
            .set_active_session(
                "dingtalk:acc:chat-1",
                "chat-1",
                "dingtalk",
                "dingtalk:acc:chat-1:child",
            )
            .await
            .expect("active session should be updated");
        assert_eq!(
            updated_base.active_session_key.as_deref(),
            Some("dingtalk:acc:chat-1:child")
        );

        let switched = store
            .set_model_provider(
                "dingtalk:acc:chat-1:child",
                "chat-1",
                "dingtalk",
                "anthropic",
                "claude-3-7-sonnet",
            )
            .await
            .expect("provider should be updated");
        assert_eq!(switched.model_provider.as_deref(), Some("anthropic"));
        assert!(switched.model_provider_explicit);
        assert_eq!(switched.model.as_deref(), Some("claude-3-7-sonnet"));
        assert!(switched.model_explicit);

        let updated_model = store
            .set_model(
                "dingtalk:acc:chat-1:child",
                "chat-1",
                "dingtalk",
                "claude-opus-4",
            )
            .await
            .expect("model should be updated");
        assert_eq!(updated_model.model.as_deref(), Some("claude-opus-4"));
        assert!(!updated_model.model_explicit);

        let cleared = store
            .clear_model_routing_override("dingtalk:acc:chat-1:child", "chat-1", "dingtalk")
            .await
            .expect("routing override should clear");
        assert_eq!(cleared.model_provider, None);
        assert!(!cleared.model_provider_explicit);
        assert_eq!(cleared.model, None);
        assert!(!cleared.model_explicit);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn session_route_state_persists_delivery_metadata() {
        let store = create_store().await;
        store
            .touch_session("dingtalk:acc:chat-meta", "chat-meta", "dingtalk")
            .await
            .expect("session should exist");

        let updated = store
            .set_delivery_metadata(
                "dingtalk:acc:chat-meta",
                "chat-meta",
                "dingtalk",
                Some(
                    "{\"channel.dingtalk.session_webhook\":\"https://example/latest\",\"channel.dingtalk.bot_title\":\"Klaw\"}",
                ),
            )
            .await
            .expect("delivery metadata should persist");
        assert_eq!(
            updated.delivery_metadata_json.as_deref(),
            Some(
                "{\"channel.dingtalk.session_webhook\":\"https://example/latest\",\"channel.dingtalk.bot_title\":\"Klaw\"}",
            )
        );

        let reloaded = store
            .get_session("dingtalk:acc:chat-meta")
            .await
            .expect("session should reload");
        assert_eq!(
            reloaded.delivery_metadata_json,
            updated.delivery_metadata_json
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn llm_usage_is_aggregated_by_session_and_turn() {
        let store = create_store().await;
        store
            .touch_session("terminal:usage", "chat-usage", "terminal")
            .await
            .expect("session should exist");

        store
            .append_llm_usage(&NewLlmUsageRecord {
                id: "usage-1".to_string(),
                session_key: "terminal:usage".to_string(),
                chat_id: "chat-usage".to_string(),
                turn_index: 0,
                request_seq: 1,
                provider: "openai".to_string(),
                model: "gpt-4.1-mini".to_string(),
                wire_api: "responses".to_string(),
                input_tokens: 10,
                output_tokens: 4,
                total_tokens: 14,
                cached_input_tokens: Some(2),
                reasoning_tokens: Some(1),
                source: LlmUsageSource::ProviderReported,
                provider_request_id: None,
                provider_response_id: Some("resp-1".to_string()),
            })
            .await
            .expect("first usage should append");
        store
            .append_llm_usage(&NewLlmUsageRecord {
                id: "usage-2".to_string(),
                session_key: "terminal:usage".to_string(),
                chat_id: "chat-usage".to_string(),
                turn_index: 0,
                request_seq: 2,
                provider: "openai".to_string(),
                model: "gpt-4.1-mini".to_string(),
                wire_api: "responses".to_string(),
                input_tokens: 7,
                output_tokens: 3,
                total_tokens: 10,
                cached_input_tokens: None,
                reasoning_tokens: None,
                source: LlmUsageSource::ProviderReported,
                provider_request_id: None,
                provider_response_id: Some("resp-2".to_string()),
            })
            .await
            .expect("second usage should append");

        let by_session = store
            .sum_llm_usage_by_session("terminal:usage")
            .await
            .expect("session usage should sum");
        assert_eq!(by_session.request_count, 2);
        assert_eq!(by_session.input_tokens, 17);
        assert_eq!(by_session.output_tokens, 7);
        assert_eq!(by_session.total_tokens, 24);
        assert_eq!(by_session.cached_input_tokens, 2);
        assert_eq!(by_session.reasoning_tokens, 1);

        let by_turn = store
            .sum_llm_usage_by_turn("terminal:usage", 0)
            .await
            .expect("turn usage should sum");
        assert_eq!(by_turn, by_session);

        let records = store
            .list_llm_usage("terminal:usage", 10, 0)
            .await
            .expect("usage history should list");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].request_seq, 2);
        assert_eq!(records[1].request_seq, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn llm_audit_supports_filtering_and_sorting() {
        let store = create_store().await;
        store
            .touch_session("terminal:audit", "chat-audit", "terminal")
            .await
            .expect("session should exist");

        store
            .append_llm_audit(&NewLlmAuditRecord {
                id: "audit-1".to_string(),
                session_key: "terminal:audit".to_string(),
                chat_id: "chat-audit".to_string(),
                turn_index: 0,
                request_seq: 1,
                provider: "openai".to_string(),
                model: "gpt-4.1-mini".to_string(),
                wire_api: "responses".to_string(),
                status: LlmAuditStatus::Success,
                error_code: None,
                error_message: None,
                provider_request_id: Some("req-1".to_string()),
                provider_response_id: Some("resp-1".to_string()),
                request_body_json: "{\"input\":\"hello\"}".to_string(),
                response_body_json: Some("{\"output\":\"world\"}".to_string()),
                metadata_json: Some("{\"sub_agent\":true}".to_string()),
                requested_at_ms: 1_000,
                responded_at_ms: Some(1_100),
            })
            .await
            .expect("first audit should append");
        store
            .append_llm_audit(&NewLlmAuditRecord {
                id: "audit-2".to_string(),
                session_key: "terminal:audit".to_string(),
                chat_id: "chat-audit".to_string(),
                turn_index: 1,
                request_seq: 2,
                provider: "anthropic".to_string(),
                model: "claude-opus-4".to_string(),
                wire_api: "messages".to_string(),
                status: LlmAuditStatus::Failed,
                error_code: Some("provider_unavailable".to_string()),
                error_message: Some("http_status=503".to_string()),
                provider_request_id: None,
                provider_response_id: None,
                request_body_json: "{\"input\":\"retry\"}".to_string(),
                response_body_json: None,
                metadata_json: None,
                requested_at_ms: 2_000,
                responded_at_ms: Some(2_050),
            })
            .await
            .expect("second audit should append");

        let filtered = store
            .list_llm_audit(&LlmAuditQuery {
                session_key: Some("terminal:audit".to_string()),
                provider: Some("anthropic".to_string()),
                requested_from_ms: Some(1_500),
                requested_to_ms: Some(2_500),
                limit: 10,
                offset: 0,
                sort_order: LlmAuditSortOrder::RequestedAtAsc,
            })
            .await
            .expect("audit rows should load");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "audit-2");
        assert_eq!(filtered[0].status, LlmAuditStatus::Failed);
        assert_eq!(filtered[0].metadata_json, None);

        let descending = store
            .list_llm_audit(&LlmAuditQuery {
                session_key: Some("terminal:audit".to_string()),
                provider: None,
                requested_from_ms: None,
                requested_to_ms: None,
                limit: 10,
                offset: 0,
                sort_order: LlmAuditSortOrder::RequestedAtDesc,
            })
            .await
            .expect("audit rows should sort");
        assert_eq!(descending.len(), 2);
        assert_eq!(descending[0].id, "audit-2");
        assert_eq!(descending[1].id, "audit-1");
        assert_eq!(
            descending[1].metadata_json.as_deref(),
            Some("{\"sub_agent\":true}")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn llm_audit_summaries_exclude_large_json_payloads() {
        let store = create_store().await;
        store
            .touch_session("terminal:audit-summary", "chat-audit-summary", "terminal")
            .await
            .expect("session should exist");

        store
            .append_llm_audit(&NewLlmAuditRecord {
                id: "audit-summary-1".to_string(),
                session_key: "terminal:audit-summary".to_string(),
                chat_id: "chat-audit-summary".to_string(),
                turn_index: 0,
                request_seq: 1,
                provider: "openai".to_string(),
                model: "gpt-4.1-mini".to_string(),
                wire_api: "responses".to_string(),
                status: LlmAuditStatus::Success,
                error_code: None,
                error_message: None,
                provider_request_id: Some("req-summary".to_string()),
                provider_response_id: Some("resp-summary".to_string()),
                request_body_json: "{\"input\":\"hello\",\"large\":true}".to_string(),
                response_body_json: Some("{\"output\":\"world\"}".to_string()),
                metadata_json: Some("{\"source\":\"detail-only\"}".to_string()),
                requested_at_ms: 1_000,
                responded_at_ms: Some(1_100),
            })
            .await
            .expect("audit should append");

        let summaries = store
            .list_llm_audit_summaries(&LlmAuditQuery {
                session_key: Some("terminal:audit-summary".to_string()),
                provider: None,
                requested_from_ms: None,
                requested_to_ms: None,
                limit: 10,
                offset: 0,
                sort_order: LlmAuditSortOrder::RequestedAtDesc,
            })
            .await
            .expect("summary rows should load");

        assert_eq!(summaries.len(), 1);
        assert_eq!(
            summaries[0],
            LlmAuditSummaryRecord {
                id: "audit-summary-1".to_string(),
                session_key: "terminal:audit-summary".to_string(),
                chat_id: "chat-audit-summary".to_string(),
                turn_index: 0,
                request_seq: 1,
                provider: "openai".to_string(),
                model: "gpt-4.1-mini".to_string(),
                wire_api: "responses".to_string(),
                status: LlmAuditStatus::Success,
                error_code: None,
                error_message: None,
                provider_request_id: Some("req-summary".to_string()),
                provider_response_id: Some("resp-summary".to_string()),
                requested_at_ms: 1_000,
                responded_at_ms: Some(1_100),
                created_at_ms: summaries[0].created_at_ms,
            }
        );

        let detail = store
            .get_llm_audit("audit-summary-1")
            .await
            .expect("detail row should load");
        assert_eq!(
            detail.request_body_json,
            "{\"input\":\"hello\",\"large\":true}"
        );
        assert_eq!(
            detail.response_body_json.as_deref(),
            Some("{\"output\":\"world\"}")
        );
        assert_eq!(
            detail.metadata_json.as_deref(),
            Some("{\"source\":\"detail-only\"}")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn llm_audit_filter_options_are_aggregated_and_sorted() {
        let store = create_store().await;
        store
            .touch_session("terminal:audit-a", "chat-audit-a", "terminal")
            .await
            .expect("first session should exist");
        store
            .touch_session("terminal:audit-b", "chat-audit-b", "terminal")
            .await
            .expect("second session should exist");

        store
            .append_llm_audit(&NewLlmAuditRecord {
                id: "audit-filter-1".to_string(),
                session_key: "terminal:audit-b".to_string(),
                chat_id: "chat-audit-b".to_string(),
                turn_index: 0,
                request_seq: 1,
                provider: "openai".to_string(),
                model: "gpt-4.1-mini".to_string(),
                wire_api: "responses".to_string(),
                status: LlmAuditStatus::Success,
                error_code: None,
                error_message: None,
                provider_request_id: None,
                provider_response_id: None,
                request_body_json: "{}".to_string(),
                response_body_json: Some("{}".to_string()),
                metadata_json: None,
                requested_at_ms: 1_000,
                responded_at_ms: Some(1_100),
            })
            .await
            .expect("first audit should append");
        store
            .append_llm_audit(&NewLlmAuditRecord {
                id: "audit-filter-2".to_string(),
                session_key: "terminal:audit-a".to_string(),
                chat_id: "chat-audit-a".to_string(),
                turn_index: 0,
                request_seq: 1,
                provider: "anthropic".to_string(),
                model: "claude-opus-4".to_string(),
                wire_api: "messages".to_string(),
                status: LlmAuditStatus::Success,
                error_code: None,
                error_message: None,
                provider_request_id: None,
                provider_response_id: None,
                request_body_json: "{}".to_string(),
                response_body_json: Some("{}".to_string()),
                metadata_json: None,
                requested_at_ms: 2_000,
                responded_at_ms: Some(2_100),
            })
            .await
            .expect("second audit should append");
        store
            .append_llm_audit(&NewLlmAuditRecord {
                id: "audit-filter-3".to_string(),
                session_key: "terminal:audit-a".to_string(),
                chat_id: "chat-audit-a".to_string(),
                turn_index: 1,
                request_seq: 2,
                provider: "openai".to_string(),
                model: "gpt-4.1-mini".to_string(),
                wire_api: "responses".to_string(),
                status: LlmAuditStatus::Failed,
                error_code: Some("rate_limit".to_string()),
                error_message: Some("retry".to_string()),
                provider_request_id: None,
                provider_response_id: None,
                request_body_json: "{}".to_string(),
                response_body_json: None,
                metadata_json: None,
                requested_at_ms: 3_000,
                responded_at_ms: Some(3_100),
            })
            .await
            .expect("third audit should append");

        let options = store
            .list_llm_audit_filter_options(&LlmAuditFilterOptionsQuery {
                requested_from_ms: Some(1_500),
                requested_to_ms: Some(3_500),
            })
            .await
            .expect("filter options should aggregate");

        assert_eq!(
            options,
            LlmAuditFilterOptions {
                session_keys: vec!["terminal:audit-a".to_string()],
                providers: vec!["anthropic".to_string(), "openai".to_string()],
            }
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tool_audit_supports_filtering_and_sorting() {
        let store = create_store().await;
        store
            .touch_session("terminal:tool-audit", "chat-tool-audit", "terminal")
            .await
            .expect("session should exist");

        store
            .append_tool_audit(&NewToolAuditRecord {
                id: "tool-audit-1".to_string(),
                session_key: "terminal:tool-audit".to_string(),
                chat_id: "chat-tool-audit".to_string(),
                turn_index: 0,
                request_seq: 1,
                tool_call_seq: 1,
                tool_name: "shell".to_string(),
                status: ToolAuditStatus::Success,
                error_code: None,
                error_message: None,
                retryable: None,
                approval_required: false,
                arguments_json: "{\"command\":\"pwd\"}".to_string(),
                result_content: "/tmp".to_string(),
                error_details_json: None,
                signals_json: Some("[]".to_string()),
                metadata_json: Some("{\"tool_call_id\":\"call_1\"}".to_string()),
                started_at_ms: 1_000,
                finished_at_ms: 1_050,
            })
            .await
            .expect("first tool audit should append");
        store
            .append_tool_audit(&NewToolAuditRecord {
                id: "tool-audit-2".to_string(),
                session_key: "terminal:tool-audit".to_string(),
                chat_id: "chat-tool-audit".to_string(),
                turn_index: 0,
                request_seq: 1,
                tool_call_seq: 2,
                tool_name: "shell".to_string(),
                status: ToolAuditStatus::Failed,
                error_code: Some("approval_required".to_string()),
                error_message: Some("approval requested".to_string()),
                retryable: Some(true),
                approval_required: true,
                arguments_json: "{\"command\":\"rm -rf /tmp/demo\"}".to_string(),
                result_content: "approval requested".to_string(),
                error_details_json: Some("{\"risk\":\"high\"}".to_string()),
                signals_json: Some(
                    "[{\"kind\":\"approval_required\",\"payload\":{\"approval_id\":\"appr_1\"}}]"
                        .to_string(),
                ),
                metadata_json: Some("{\"tool_call_id\":\"call_2\"}".to_string()),
                started_at_ms: 2_000,
                finished_at_ms: 2_100,
            })
            .await
            .expect("second tool audit should append");

        let rows = store
            .list_tool_audit(&ToolAuditQuery {
                session_key: Some("terminal:tool-audit".to_string()),
                tool_name: Some("shell".to_string()),
                started_from_ms: Some(1_500),
                started_to_ms: Some(2_500),
                limit: 10,
                offset: 0,
                sort_order: ToolAuditSortOrder::StartedAtDesc,
            })
            .await
            .expect("tool audit rows should load");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "tool-audit-2");
        assert!(rows[0].approval_required);
        assert_eq!(rows[0].retryable, Some(true));

        let options = store
            .list_tool_audit_filter_options(&ToolAuditFilterOptionsQuery {
                started_from_ms: Some(500),
                started_to_ms: Some(2_500),
            })
            .await
            .expect("tool audit filters should load");
        assert_eq!(
            options.session_keys,
            vec!["terminal:tool-audit".to_string()]
        );
        assert_eq!(options.tool_names, vec!["shell".to_string()]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn storage_paths_include_memory_db() {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("klaw-storage-paths-{suffix}"));
        let paths = StoragePaths::from_root(base.clone());
        assert_eq!(paths.memory_db_path, base.join("memory.db"));
        assert_eq!(paths.archive_db_path, base.join("archive.db"));
        assert_eq!(paths.tmp_dir, base.join("tmp"));
        assert_eq!(paths.archives_dir, base.join("archives"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn open_default_memory_db_is_idempotent() {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("klaw-memory-db-test-{}-{suffix}", util::now_ms()));
        let paths = StoragePaths::from_root(base);

        #[cfg(feature = "turso")]
        {
            let _db1 = turso::TursoMemoryDb::open(paths.clone())
                .await
                .expect("memory db should open");
            let _db2 = turso::TursoMemoryDb::open(paths)
                .await
                .expect("memory db should reopen");
        }

        #[cfg(feature = "sqlx")]
        {
            let _db1 = sqlx::SqlxMemoryDb::open(paths.clone())
                .await
                .expect("memory db should open");
            let _db2 = sqlx::SqlxMemoryDb::open(paths)
                .await
                .expect("memory db should reopen");
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn open_default_archive_db_is_idempotent() {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("klaw-archive-db-test-{}-{suffix}", util::now_ms()));
        let paths = StoragePaths::from_root(base);

        #[cfg(feature = "turso")]
        {
            let _db1 = turso::TursoArchiveDb::open(paths.clone())
                .await
                .expect("archive db should open");
            let _db2 = turso::TursoArchiveDb::open(paths)
                .await
                .expect("archive db should reopen");
        }

        #[cfg(feature = "sqlx")]
        {
            let _db1 = sqlx::SqlxArchiveDb::open(paths.clone())
                .await
                .expect("archive db should open");
            let _db2 = sqlx::SqlxArchiveDb::open(paths)
                .await
                .expect("archive db should reopen");
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cron_claim_next_run_is_cas_safe() {
        let store = create_store().await;
        let new_job = NewCronJob {
            id: "job-cas".to_string(),
            name: "cas".to_string(),
            schedule_kind: CronScheduleKind::Every,
            schedule_expr: "30s".to_string(),
            payload_json: "{\"channel\":\"cron\",\"sender_id\":\"cron\",\"chat_id\":\"c1\",\"session_key\":\"cron:c1\",\"content\":\"ping\",\"metadata\":{}}".to_string(),
            enabled: true,
            timezone: "UTC".to_string(),
            next_run_at_ms: 1000,
        };
        let job = store
            .create_cron(&new_job)
            .await
            .expect("create cron should succeed");
        let first = store
            .claim_next_run(&job.id, 1000, 2000, 1100)
            .await
            .expect("first claim should succeed");
        let second = store
            .claim_next_run(&job.id, 1000, 3000, 1200)
            .await
            .expect("second claim should return false");
        assert!(first);
        assert!(!second);
        let updated = store.get_cron(&job.id).await.expect("cron should exist");
        assert_eq!(updated.next_run_at_ms, 2000);
        assert_eq!(updated.last_run_at_ms, Some(1000));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cron_missing_id_mutations_return_not_found_errors() {
        let store = create_store().await;

        let err = store
            .update_cron(
                "missing-cron",
                &UpdateCronJobPatch {
                    name: Some("renamed".to_string()),
                    ..Default::default()
                },
            )
            .await
            .expect_err("update should fail for missing cron");
        assert!(err.to_string().contains("cron job not found"));

        let err = store
            .set_enabled("missing-cron", false)
            .await
            .expect_err("set_enabled should fail for missing cron");
        assert!(
            err.to_string()
                .contains("cron job 'missing-cron' not found when setting enabled")
        );

        let err = store
            .delete_cron("missing-cron")
            .await
            .expect_err("delete should fail for missing cron");
        assert!(
            err.to_string()
                .contains("cron job 'missing-cron' not found when deleting")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cron_task_lifecycle_transitions() {
        let store = create_store().await;
        store
            .create_cron(&NewCronJob {
                id: "job-run".to_string(),
                name: "run".to_string(),
                schedule_kind: CronScheduleKind::Cron,
                schedule_expr: "0 * * * * *".to_string(),
                payload_json: "{\"channel\":\"cron\",\"sender_id\":\"cron\",\"chat_id\":\"c2\",\"session_key\":\"cron:c2\",\"content\":\"hello\",\"metadata\":{}}".to_string(),
                enabled: true,
                timezone: "UTC".to_string(),
                next_run_at_ms: 2000,
            })
            .await
            .expect("create cron should succeed");

        let run = store
            .append_task_run(&NewCronTaskRun {
                id: "run-1".to_string(),
                cron_id: "job-run".to_string(),
                scheduled_at_ms: 2000,
                status: CronTaskStatus::Pending,
                attempt: 0,
                created_at_ms: 2001,
            })
            .await
            .expect("append task run should succeed");
        assert_eq!(run.status, CronTaskStatus::Pending);

        store
            .mark_task_running("run-1", 2010)
            .await
            .expect("mark running should succeed");
        store
            .mark_task_result(
                "run-1",
                CronTaskStatus::Success,
                2020,
                None,
                Some("message-1"),
            )
            .await
            .expect("mark result should succeed");

        let runs = store
            .list_task_runs("job-run", 10, 0)
            .await
            .expect("list task runs should succeed");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, CronTaskStatus::Success);
        assert_eq!(runs[0].published_message_id.as_deref(), Some("message-1"));
        assert_eq!(runs[0].started_at_ms, Some(2010));
        assert_eq!(runs[0].finished_at_ms, Some(2020));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn approval_lifecycle_create_approve_consume() {
        let store = create_store().await;
        let session = store
            .touch_session("terminal:approval", "approval-chat", "terminal")
            .await
            .expect("session should exist for approval foreign key");
        assert_eq!(session.session_key, "terminal:approval");

        let created = store
            .create_approval(&NewApprovalRecord {
                id: "approval-1".to_string(),
                session_key: "terminal:approval".to_string(),
                tool_name: "shell".to_string(),
                command_hash: "abc123".to_string(),
                command_preview: "touch file.txt".to_string(),
                command_text: "touch file.txt".to_string(),
                risk_level: "mutating".to_string(),
                requested_by: "agent".to_string(),
                justification: None,
                expires_at_ms: util::now_ms() + 60_000,
            })
            .await
            .expect("create approval should succeed");
        assert_eq!(created.status, ApprovalStatus::Pending);

        let approved = store
            .update_approval_status("approval-1", ApprovalStatus::Approved, Some("user"))
            .await
            .expect("approve should succeed");
        assert_eq!(approved.status, ApprovalStatus::Approved);
        assert_eq!(approved.approved_by.as_deref(), Some("user"));

        let consumed = store
            .consume_approved_shell_command(
                "approval-1",
                "terminal:approval",
                "abc123",
                util::now_ms(),
            )
            .await
            .expect("consume should succeed");
        assert!(consumed);

        let post = store
            .get_approval("approval-1")
            .await
            .expect("approval should still exist");
        assert_eq!(post.status, ApprovalStatus::Consumed);
        assert!(post.consumed_at_ms.is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn approval_consume_latest_by_session_and_command_hash() {
        let store = create_store().await;
        store
            .touch_session("terminal:approval2", "approval-chat2", "terminal")
            .await
            .expect("session should exist for approval foreign key");

        store
            .create_approval(&NewApprovalRecord {
                id: "approval-latest-1".to_string(),
                session_key: "terminal:approval2".to_string(),
                tool_name: "shell".to_string(),
                command_hash: "samehash".to_string(),
                command_preview: "touch a.txt".to_string(),
                command_text: "touch a.txt".to_string(),
                risk_level: "mutating".to_string(),
                requested_by: "agent".to_string(),
                justification: None,
                expires_at_ms: util::now_ms() + 60_000,
            })
            .await
            .expect("create should succeed");
        store
            .update_approval_status("approval-latest-1", ApprovalStatus::Approved, Some("user"))
            .await
            .expect("approve should succeed");

        let consumed = store
            .consume_latest_approved_shell_command("terminal:approval2", "samehash", util::now_ms())
            .await
            .expect("consume latest should succeed");
        assert!(consumed);

        let post = store
            .get_approval("approval-latest-1")
            .await
            .expect("approval should exist");
        assert_eq!(post.status, ApprovalStatus::Consumed);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn approval_consume_tool_command_supports_non_shell_tools() {
        let store = create_store().await;
        store
            .touch_session("terminal:approval3", "approval-chat3", "terminal")
            .await
            .expect("session should exist for approval foreign key");

        store
            .create_approval(&NewApprovalRecord {
                id: "approval-apply-patch-1".to_string(),
                session_key: "terminal:approval3".to_string(),
                tool_name: "apply_patch".to_string(),
                command_hash: "patchhash".to_string(),
                command_preview: "add_file:/tmp/outside.txt".to_string(),
                command_text: "add_file:/tmp/outside.txt".to_string(),
                risk_level: "mutating".to_string(),
                requested_by: "agent".to_string(),
                justification: None,
                expires_at_ms: util::now_ms() + 60_000,
            })
            .await
            .expect("create approval should succeed");
        store
            .update_approval_status(
                "approval-apply-patch-1",
                ApprovalStatus::Approved,
                Some("user"),
            )
            .await
            .expect("approve should succeed");

        let consumed = store
            .consume_approved_tool_command(
                "approval-apply-patch-1",
                "apply_patch",
                "terminal:approval3",
                "patchhash",
                util::now_ms(),
            )
            .await
            .expect("generic consume should succeed");
        assert!(consumed);

        let post = store
            .get_approval("approval-apply-patch-1")
            .await
            .expect("approval should still exist");
        assert_eq!(post.status, ApprovalStatus::Consumed);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn approval_consume_tool_command_rejects_tool_name_mismatch() {
        let store = create_store().await;
        store
            .touch_session("terminal:approval4", "approval-chat4", "terminal")
            .await
            .expect("session should exist for approval foreign key");

        store
            .create_approval(&NewApprovalRecord {
                id: "approval-shell-only-1".to_string(),
                session_key: "terminal:approval4".to_string(),
                tool_name: "shell".to_string(),
                command_hash: "shellhash".to_string(),
                command_preview: "rm -rf /tmp/demo".to_string(),
                command_text: "rm -rf /tmp/demo".to_string(),
                risk_level: "unsafe".to_string(),
                requested_by: "agent".to_string(),
                justification: None,
                expires_at_ms: util::now_ms() + 60_000,
            })
            .await
            .expect("create approval should succeed");
        store
            .update_approval_status(
                "approval-shell-only-1",
                ApprovalStatus::Approved,
                Some("user"),
            )
            .await
            .expect("approve should succeed");

        let consumed = store
            .consume_approved_tool_command(
                "approval-shell-only-1",
                "apply_patch",
                "terminal:approval4",
                "shellhash",
                util::now_ms(),
            )
            .await
            .expect("generic consume should still return a boolean");
        assert!(!consumed);

        let post = store
            .get_approval("approval-shell-only-1")
            .await
            .expect("approval should still exist");
        assert_eq!(post.status, ApprovalStatus::Approved);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn approval_consume_latest_tool_command_supports_non_shell_tools() {
        let store = create_store().await;
        store
            .touch_session("terminal:approval5", "approval-chat5", "terminal")
            .await
            .expect("session should exist for approval foreign key");

        store
            .create_approval(&NewApprovalRecord {
                id: "approval-apply-patch-latest-1".to_string(),
                session_key: "terminal:approval5".to_string(),
                tool_name: "apply_patch".to_string(),
                command_hash: "latesthash".to_string(),
                command_preview: "move_file:/tmp/a->/tmp/b".to_string(),
                command_text: "move_file:/tmp/a->/tmp/b".to_string(),
                risk_level: "outside_workspace".to_string(),
                requested_by: "agent".to_string(),
                justification: None,
                expires_at_ms: util::now_ms() + 60_000,
            })
            .await
            .expect("create approval should succeed");
        store
            .update_approval_status(
                "approval-apply-patch-latest-1",
                ApprovalStatus::Approved,
                Some("user"),
            )
            .await
            .expect("approve should succeed");

        let consumed = store
            .consume_latest_approved_tool_command(
                "apply_patch",
                "terminal:approval5",
                "latesthash",
                util::now_ms(),
            )
            .await
            .expect("generic latest consume should succeed");
        assert!(consumed);

        let post = store
            .get_approval("approval-apply-patch-latest-1")
            .await
            .expect("approval should still exist");
        assert_eq!(post.status, ApprovalStatus::Consumed);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn webhook_event_supports_append_update_and_filtering() {
        let store = create_store().await;
        store
            .touch_session("webhook:test", "webhook:test", "webhook")
            .await
            .expect("session should exist for webhook foreign key");

        store
            .append_webhook_event(&NewWebhookEventRecord {
                id: "evt-1".to_string(),
                source: "github".to_string(),
                event_type: "issue_comment.created".to_string(),
                session_key: "webhook:test".to_string(),
                chat_id: "webhook:test".to_string(),
                sender_id: "github:webhook".to_string(),
                content: "New comment".to_string(),
                payload_json: Some("{\"k\":1}".to_string()),
                metadata_json: Some("{\"trigger.kind\":\"webhook\"}".to_string()),
                status: WebhookEventStatus::Accepted,
                error_message: None,
                response_summary: None,
                received_at_ms: 1000,
                processed_at_ms: None,
                remote_addr: Some("127.0.0.1".to_string()),
            })
            .await
            .expect("append webhook event should succeed");

        store
            .update_webhook_event_status(
                "evt-1",
                &UpdateWebhookEventResult {
                    status: WebhookEventStatus::Processed,
                    error_message: None,
                    response_summary: Some("processed".to_string()),
                    processed_at_ms: Some(2000),
                },
            )
            .await
            .expect("update webhook event should succeed");

        let rows = store
            .list_webhook_events(&WebhookEventQuery {
                source: Some("github".to_string()),
                event_type: None,
                session_key: Some("webhook:test".to_string()),
                status: Some(WebhookEventStatus::Processed),
                received_from_ms: Some(900),
                received_to_ms: Some(1500),
                limit: 10,
                offset: 0,
                sort_order: WebhookEventSortOrder::ReceivedAtDesc,
            })
            .await
            .expect("list webhook events should succeed");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "evt-1");
        assert_eq!(rows[0].status, WebhookEventStatus::Processed);
        assert_eq!(rows[0].response_summary.as_deref(), Some("processed"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn webhook_agent_supports_append_update_and_filtering() {
        let store = create_store().await;
        store
            .touch_session("webhook:agent", "webhook:agent", "webhook")
            .await
            .expect("session should exist for webhook foreign key");

        store
            .append_webhook_agent(&NewWebhookAgentRecord {
                id: "agent-1".to_string(),
                hook_id: "order_sync".to_string(),
                session_key: "webhook:agent".to_string(),
                chat_id: "webhook:agent".to_string(),
                sender_id: "webhook-agent:order_sync".to_string(),
                content: "Prompt".to_string(),
                payload_json: Some("{\"order\":1}".to_string()),
                metadata_json: Some("{\"webhook.kind\":\"agents\"}".to_string()),
                status: WebhookEventStatus::Accepted,
                error_message: None,
                response_summary: None,
                received_at_ms: 3000,
                processed_at_ms: None,
                remote_addr: Some("127.0.0.1".to_string()),
            })
            .await
            .expect("append webhook agent should succeed");

        store
            .update_webhook_agent_status(
                "agent-1",
                &UpdateWebhookAgentResult {
                    status: WebhookEventStatus::Processed,
                    error_message: None,
                    response_summary: Some("agent processed".to_string()),
                    processed_at_ms: Some(4000),
                },
            )
            .await
            .expect("update webhook agent should succeed");

        let rows = store
            .list_webhook_agents(&WebhookAgentQuery {
                hook_id: Some("order_sync".to_string()),
                session_key: Some("webhook:agent".to_string()),
                status: Some(WebhookEventStatus::Processed),
                received_from_ms: Some(2500),
                received_to_ms: Some(3500),
                limit: 10,
                offset: 0,
                sort_order: WebhookEventSortOrder::ReceivedAtDesc,
            })
            .await
            .expect("list webhook agents should succeed");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "agent-1");
        assert_eq!(rows[0].hook_id, "order_sync");
        assert_eq!(rows[0].response_summary.as_deref(), Some("agent processed"));
    }
}
