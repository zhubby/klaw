mod backup;
mod error;
mod jsonl;
mod memory_db;
mod paths;
mod traits;
mod types;
mod util;

pub mod backend;

pub use backup::{
    BackupItem, BackupPlan, BackupProgress, BackupProgressStage, BackupResult, BackupService,
    DatabaseSnapshotExporter, LatestRef, ManifestEntry, ManifestEntryKind, S3SnapshotStoreConfig,
    SnapshotListItem, SnapshotMode, SnapshotPrepareResult, SnapshotRestoreResult, SnapshotSchedule,
    SnapshotStore, SyncManifest,
};
pub use error::StorageError;
pub use memory_db::{DbRow, DbValue, MemoryDb};
pub use paths::StoragePaths;
pub use traits::{CronStorage, HeartbeatStorage, SessionStorage};
pub use types::{
    ApprovalRecord, ApprovalStatus, ChatRecord, CronJob, CronScheduleKind, CronTaskRun,
    CronTaskStatus, HeartbeatJob, HeartbeatTaskRun, HeartbeatTaskStatus, LlmAuditFilterOptions,
    LlmAuditFilterOptionsQuery, LlmAuditQuery, LlmAuditRecord, LlmAuditSortOrder, LlmAuditStatus,
    LlmUsageRecord, LlmUsageSource, LlmUsageSummary, NewApprovalRecord, NewCronJob, NewCronTaskRun,
    NewHeartbeatJob, NewHeartbeatTaskRun, NewLlmAuditRecord, NewLlmUsageRecord,
    NewWebhookAgentRecord, NewWebhookEventRecord, SessionCompressionState, SessionIndex,
    UpdateCronJobPatch, UpdateHeartbeatJobPatch, UpdateWebhookAgentResult,
    UpdateWebhookEventResult, WebhookAgentQuery, WebhookAgentRecord, WebhookEventQuery,
    WebhookEventRecord, WebhookEventSortOrder, WebhookEventStatus,
};

#[cfg(all(feature = "turso", feature = "sqlx"))]
compile_error!("features `turso` and `sqlx` are mutually exclusive; enable only one backend");

#[cfg(not(any(feature = "turso", feature = "sqlx")))]
compile_error!("enable one backend feature: `turso` or `sqlx`");

#[cfg(all(feature = "turso", not(feature = "sqlx")))]
pub type DefaultSessionStore = backend::turso::TursoSessionStore;
#[cfg(all(feature = "sqlx", not(feature = "turso")))]
pub type DefaultSessionStore = backend::sqlx::SqlxSessionStore;
#[cfg(all(feature = "turso", not(feature = "sqlx")))]
pub type DefaultMemoryDb = backend::turso::TursoMemoryDb;
#[cfg(all(feature = "sqlx", not(feature = "turso")))]
pub type DefaultMemoryDb = backend::sqlx::SqlxMemoryDb;
#[cfg(all(feature = "turso", not(feature = "sqlx")))]
pub type DefaultArchiveDb = backend::turso::TursoArchiveDb;
#[cfg(all(feature = "sqlx", not(feature = "turso")))]
pub type DefaultArchiveDb = backend::sqlx::SqlxArchiveDb;

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
            .touch_session("stdio:test1", "test1", "stdio")
            .await
            .expect("touch should succeed");
        assert_eq!(first.turn_count, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn complete_turn_increments_only_on_response() {
        let store = create_store().await;
        let _ = store
            .touch_session("stdio:test2", "test2", "stdio")
            .await
            .expect("touch should succeed");
        let completed = store
            .complete_turn("stdio:test2", "test2", "stdio")
            .await
            .expect("complete turn should succeed");
        assert_eq!(completed.turn_count, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn append_chat_record_writes_jsonl() {
        let store = create_store().await;
        let record = ChatRecord::new("user", "hello", Some("m1".to_string()));
        store
            .append_chat_record("stdio:test3", &record)
            .await
            .expect("append should succeed");

        let file_path = store.session_jsonl_path("stdio:test3");
        let contents = fs::read_to_string(file_path)
            .await
            .expect("jsonl file should exist");
        assert!(contents.contains("\"role\":\"user\""));
        assert!(contents.contains("\"content\":\"hello\""));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_chat_records_returns_ordered_history() {
        let store = create_store().await;
        store
            .append_chat_record(
                "stdio:test-history",
                &ChatRecord::new("user", "hello", Some("m1".to_string())),
            )
            .await
            .expect("first append should succeed");
        store
            .append_chat_record(
                "stdio:test-history",
                &ChatRecord::new("assistant", "world", Some("m2".to_string())),
            )
            .await
            .expect("second append should succeed");

        let records = store
            .read_chat_records("stdio:test-history")
            .await
            .expect("history read should succeed");
        let summary: Vec<(&str, &str)> = records
            .iter()
            .map(|record| (record.role.as_str(), record.content.as_str()))
            .collect();
        assert_eq!(summary, vec![("user", "hello"), ("assistant", "world")]);
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
            .touch_session("stdio:usage", "chat-usage", "stdio")
            .await
            .expect("session should exist");

        store
            .append_llm_usage(&NewLlmUsageRecord {
                id: "usage-1".to_string(),
                session_key: "stdio:usage".to_string(),
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
                session_key: "stdio:usage".to_string(),
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
            .sum_llm_usage_by_session("stdio:usage")
            .await
            .expect("session usage should sum");
        assert_eq!(by_session.request_count, 2);
        assert_eq!(by_session.input_tokens, 17);
        assert_eq!(by_session.output_tokens, 7);
        assert_eq!(by_session.total_tokens, 24);
        assert_eq!(by_session.cached_input_tokens, 2);
        assert_eq!(by_session.reasoning_tokens, 1);

        let by_turn = store
            .sum_llm_usage_by_turn("stdio:usage", 0)
            .await
            .expect("turn usage should sum");
        assert_eq!(by_turn, by_session);

        let records = store
            .list_llm_usage("stdio:usage", 10, 0)
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
            .touch_session("stdio:audit", "chat-audit", "stdio")
            .await
            .expect("session should exist");

        store
            .append_llm_audit(&NewLlmAuditRecord {
                id: "audit-1".to_string(),
                session_key: "stdio:audit".to_string(),
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
                requested_at_ms: 1_000,
                responded_at_ms: Some(1_100),
            })
            .await
            .expect("first audit should append");
        store
            .append_llm_audit(&NewLlmAuditRecord {
                id: "audit-2".to_string(),
                session_key: "stdio:audit".to_string(),
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
                requested_at_ms: 2_000,
                responded_at_ms: Some(2_050),
            })
            .await
            .expect("second audit should append");

        let filtered = store
            .list_llm_audit(&LlmAuditQuery {
                session_key: Some("stdio:audit".to_string()),
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

        let descending = store
            .list_llm_audit(&LlmAuditQuery {
                session_key: Some("stdio:audit".to_string()),
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
    }

    #[tokio::test(flavor = "current_thread")]
    async fn llm_audit_filter_options_are_aggregated_and_sorted() {
        let store = create_store().await;
        store
            .touch_session("stdio:audit-a", "chat-audit-a", "stdio")
            .await
            .expect("first session should exist");
        store
            .touch_session("stdio:audit-b", "chat-audit-b", "stdio")
            .await
            .expect("second session should exist");

        store
            .append_llm_audit(&NewLlmAuditRecord {
                id: "audit-filter-1".to_string(),
                session_key: "stdio:audit-b".to_string(),
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
                requested_at_ms: 1_000,
                responded_at_ms: Some(1_100),
            })
            .await
            .expect("first audit should append");
        store
            .append_llm_audit(&NewLlmAuditRecord {
                id: "audit-filter-2".to_string(),
                session_key: "stdio:audit-a".to_string(),
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
                requested_at_ms: 2_000,
                responded_at_ms: Some(2_100),
            })
            .await
            .expect("second audit should append");
        store
            .append_llm_audit(&NewLlmAuditRecord {
                id: "audit-filter-3".to_string(),
                session_key: "stdio:audit-a".to_string(),
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
                session_keys: vec!["stdio:audit-a".to_string()],
                providers: vec!["anthropic".to_string(), "openai".to_string()],
            }
        );
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
            let _db1 = backend::turso::TursoMemoryDb::open(paths.clone())
                .await
                .expect("memory db should open");
            let _db2 = backend::turso::TursoMemoryDb::open(paths)
                .await
                .expect("memory db should reopen");
        }

        #[cfg(feature = "sqlx")]
        {
            let _db1 = backend::sqlx::SqlxMemoryDb::open(paths.clone())
                .await
                .expect("memory db should open");
            let _db2 = backend::sqlx::SqlxMemoryDb::open(paths)
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
            let _db1 = backend::turso::TursoArchiveDb::open(paths.clone())
                .await
                .expect("archive db should open");
            let _db2 = backend::turso::TursoArchiveDb::open(paths)
                .await
                .expect("archive db should reopen");
        }

        #[cfg(feature = "sqlx")]
        {
            let _db1 = backend::sqlx::SqlxArchiveDb::open(paths.clone())
                .await
                .expect("archive db should open");
            let _db2 = backend::sqlx::SqlxArchiveDb::open(paths)
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
            .touch_session("stdio:approval", "approval-chat", "stdio")
            .await
            .expect("session should exist for approval foreign key");
        assert_eq!(session.session_key, "stdio:approval");

        let created = store
            .create_approval(&NewApprovalRecord {
                id: "approval-1".to_string(),
                session_key: "stdio:approval".to_string(),
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
                "stdio:approval",
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
            .touch_session("stdio:approval2", "approval-chat2", "stdio")
            .await
            .expect("session should exist for approval foreign key");

        store
            .create_approval(&NewApprovalRecord {
                id: "approval-latest-1".to_string(),
                session_key: "stdio:approval2".to_string(),
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
            .consume_latest_approved_shell_command("stdio:approval2", "samehash", util::now_ms())
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
