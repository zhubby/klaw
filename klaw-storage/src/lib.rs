mod error;
mod jsonl;
mod memory_db;
mod paths;
mod traits;
mod types;
mod util;

pub mod backend;

pub use error::StorageError;
pub use memory_db::{DbRow, DbValue, MemoryDb};
pub use paths::StoragePaths;
pub use traits::{CronStorage, SessionStorage};
pub use types::{
    ChatRecord, CronJob, CronScheduleKind, CronTaskRun, CronTaskStatus, NewCronJob, NewCronTaskRun,
    SessionIndex, UpdateCronJobPatch,
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
    async fn storage_paths_include_memory_db() {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("klaw-storage-paths-{suffix}"));
        let paths = StoragePaths::from_root(base.clone());
        assert_eq!(paths.memory_db_path, base.join("memory.db"));
        assert_eq!(paths.archive_db_path, base.join("archive.db"));
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
}
