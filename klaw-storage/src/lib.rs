mod error;
mod jsonl;
mod paths;
mod traits;
mod types;
mod util;

pub mod backend;

pub use error::StorageError;
pub use paths::StoragePaths;
pub use traits::SessionStorage;
pub use types::{ChatRecord, SessionIndex};

#[cfg(all(feature = "turso", feature = "sqlx"))]
compile_error!("features `turso` and `sqlx` are mutually exclusive; enable only one backend");

#[cfg(not(any(feature = "turso", feature = "sqlx")))]
compile_error!("enable one backend feature: `turso` or `sqlx`");

#[cfg(all(feature = "turso", not(feature = "sqlx")))]
pub type DefaultSessionStore = backend::turso::TursoSessionStore;
#[cfg(all(feature = "sqlx", not(feature = "turso")))]
pub type DefaultSessionStore = backend::sqlx::SqlxSessionStore;

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
}
