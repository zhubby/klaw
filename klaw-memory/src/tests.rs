use crate::{
    EmbeddingProvider, MemorySearchQuery, MemoryService, SqliteMemoryService, UpsertMemoryInput,
    build_embedding_provider_from_config,
    util::{now_ms, rrf_score},
};
use async_trait::async_trait;
use klaw_config::{AppConfig, ModelProviderConfig};
use klaw_storage::{DefaultMemoryDb, MemoryDb, StoragePaths};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct MockEmbeddingProvider;

#[async_trait]
impl EmbeddingProvider for MockEmbeddingProvider {
    fn provider_name(&self) -> &str {
        "mock"
    }

    fn model(&self) -> &str {
        "mock-v1"
    }

    async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, crate::MemoryError> {
        Ok(texts
            .into_iter()
            .map(|text| vec![text.len() as f32, 1.0, 0.5])
            .collect())
    }
}

async fn create_db() -> Arc<dyn MemoryDb> {
    let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("klaw-memory-test-{suffix}-{}", now_ms()));
    let paths = StoragePaths::from_root(root);
    Arc::new(DefaultMemoryDb::open(paths).await.expect("open memory db"))
}

#[tokio::test(flavor = "current_thread")]
async fn upsert_and_get_memory_record() {
    let db = create_db().await;
    let service = SqliteMemoryService::new(db, Some(Arc::new(MockEmbeddingProvider)))
        .await
        .expect("service should init");

    let stored = service
        .upsert(UpsertMemoryInput {
            id: None,
            scope: "session:abc".to_string(),
            content: "remember the SKU A-123".to_string(),
            metadata: serde_json::json!({"kind":"sku"}),
            pinned: false,
        })
        .await
        .expect("upsert should work");

    let loaded = service.get(&stored.id).await.expect("get should work");
    assert!(loaded.is_some());
    let loaded = loaded.expect("record exists");
    assert_eq!(loaded.scope, "session:abc");
    assert_eq!(loaded.metadata["kind"], "sku");
}

#[tokio::test(flavor = "current_thread")]
async fn fts_search_returns_hits_without_vector() {
    let db = create_db().await;
    let service = SqliteMemoryService::new(db, Some(Arc::new(MockEmbeddingProvider)))
        .await
        .expect("service should init");

    let _ = service
        .upsert(UpsertMemoryInput {
            id: Some("m-1".to_string()),
            scope: "session:abc".to_string(),
            content: "error code E_CONNRESET should retry".to_string(),
            metadata: serde_json::json!({}),
            pinned: false,
        })
        .await
        .expect("upsert should work");

    let hits = service
        .search(MemorySearchQuery {
            scope: Some("session:abc".to_string()),
            text: "E_CONNRESET".to_string(),
            use_vector: false,
            ..MemorySearchQuery::default()
        })
        .await
        .expect("search should work");

    assert!(!hits.is_empty());
    assert!(hits[0].record.content.contains("E_CONNRESET"));
}

#[tokio::test(flavor = "current_thread")]
async fn pin_and_delete_are_consistent() {
    let db = create_db().await;
    let service = SqliteMemoryService::new(db, Some(Arc::new(MockEmbeddingProvider)))
        .await
        .expect("service should init");
    let stored = service
        .upsert(UpsertMemoryInput {
            id: Some("m-2".to_string()),
            scope: "session:abc".to_string(),
            content: "name is Alice".to_string(),
            metadata: serde_json::json!({}),
            pinned: false,
        })
        .await
        .expect("upsert should work");

    let pinned = service
        .pin(&stored.id, true)
        .await
        .expect("pin should work")
        .expect("record should exist");
    assert!(pinned.pinned);

    let deleted = service
        .delete(&stored.id)
        .await
        .expect("delete should work");
    assert!(deleted);
    let loaded = service.get(&stored.id).await.expect("get should work");
    assert!(loaded.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn works_without_embedding_provider() {
    let db = create_db().await;
    let service = SqliteMemoryService::new(db, None)
        .await
        .expect("service should init without embeddings");

    let _ = service
        .upsert(UpsertMemoryInput {
            id: Some("m-no-embed".to_string()),
            scope: "session:abc".to_string(),
            content: "remember fallback path".to_string(),
            metadata: serde_json::json!({}),
            pinned: false,
        })
        .await
        .expect("upsert should work without embeddings");

    let hits = service
        .search(MemorySearchQuery {
            text: "fallback".to_string(),
            use_vector: true,
            ..MemorySearchQuery::default()
        })
        .await
        .expect("search should fallback to text");
    assert!(!hits.is_empty());
}

#[test]
fn rrf_favors_multi_channel_hits() {
    let dual = rrf_score(Some(1), Some(3));
    let only_one = rrf_score(Some(1), None);
    assert!(dual > only_one);
}

#[test]
fn embedding_provider_build_uses_memory_config() {
    let mut providers = BTreeMap::new();
    providers.insert(
        "openai".to_string(),
        ModelProviderConfig {
            name: None,
            base_url: "https://api.openai.com/v1".to_string(),
            wire_api: "responses".to_string(),
            default_model: "gpt-4o-mini".to_string(),
            tokenizer_path: None,
            proxy: false,
            stream: false,
            api_key: Some("test-key".to_string()),
            env_key: None,
        },
    );
    let config = AppConfig {
        model_provider: "openai".to_string(),
        model_providers: providers,
        memory: klaw_config::MemoryConfig {
            embedding: klaw_config::EmbeddingConfig {
                enabled: true,
                provider: "openai".to_string(),
                model: "text-embedding-3-small".to_string(),
            },
        },
        ..Default::default()
    };

    let provider = build_embedding_provider_from_config(&config).expect("provider build");
    assert_eq!(provider.provider_name(), "openai");
    assert_eq!(provider.model(), "text-embedding-3-small");
}
