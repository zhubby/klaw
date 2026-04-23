use crate::{
    EmbeddingProvider, LongTermArchiveConfig, MemorySearchQuery, MemoryService,
    SqliteMemoryService, SqliteMemoryStatsService, UpsertMemoryInput,
    archive_stale_long_term_memories, build_embedding_provider_from_config,
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
async fn search_skips_inactive_long_term_records() {
    let db = create_db().await;
    let service = SqliteMemoryService::new(db, Some(Arc::new(MockEmbeddingProvider)))
        .await
        .expect("service should init");

    let _ = service
        .upsert(UpsertMemoryInput {
            id: Some("m-active".to_string()),
            scope: "long_term".to_string(),
            content: "Default reply language is Chinese.".to_string(),
            metadata: serde_json::json!({
                "kind": "preference",
                "status": "active",
            }),
            pinned: false,
        })
        .await
        .expect("active upsert should work");

    let _ = service
        .upsert(UpsertMemoryInput {
            id: Some("m-archived".to_string()),
            scope: "long_term".to_string(),
            content: "Default reply language is Chinese (old).".to_string(),
            metadata: serde_json::json!({
                "kind": "preference",
                "status": "archived",
            }),
            pinned: false,
        })
        .await
        .expect("archived upsert should work");

    let hits = service
        .search(MemorySearchQuery {
            scope: Some("long_term".to_string()),
            text: "reply language".to_string(),
            use_vector: false,
            limit: 10,
            ..MemorySearchQuery::default()
        })
        .await
        .expect("search should work");

    let ids = hits
        .iter()
        .map(|hit| hit.record.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["m-active"]);
}

#[tokio::test(flavor = "current_thread")]
async fn archive_stale_long_term_memories_archives_low_priority_records_and_creates_summary() {
    let db = create_db().await;
    let service = SqliteMemoryService::new(db.clone(), Some(Arc::new(MockEmbeddingProvider)))
        .await
        .expect("service should init");
    let now = now_ms();
    let forty_days_ms = 40_i64 * 24 * 60 * 60 * 1000;

    let _ = service
        .upsert(UpsertMemoryInput {
            id: Some("old-1".to_string()),
            scope: "long_term".to_string(),
            content: "Worked from Beijing office.".to_string(),
            metadata: serde_json::json!({
                "kind": "fact",
                "priority": "low",
                "topic": "work_city",
                "status": "active",
            }),
            pinned: false,
        })
        .await
        .expect("first record should upsert");
    let _ = service
        .upsert(UpsertMemoryInput {
            id: Some("old-2".to_string()),
            scope: "long_term".to_string(),
            content: "Moved to Shenzhen for work.".to_string(),
            metadata: serde_json::json!({
                "kind": "fact",
                "priority": "low",
                "topic": "work_city",
                "status": "active",
            }),
            pinned: false,
        })
        .await
        .expect("second record should upsert");

    for id in ["old-1", "old-2"] {
        let _ = db
            .execute(
                "UPDATE memories SET updated_at_ms = ?1, created_at_ms = ?1 WHERE id = ?2",
                &[
                    klaw_storage::DbValue::Integer(now - forty_days_ms),
                    klaw_storage::DbValue::Text(id.to_string()),
                ],
            )
            .await
            .expect("timestamps should update");
    }

    let outcome = archive_stale_long_term_memories(
        db.clone(),
        LongTermArchiveConfig {
            max_age_days: 30,
            summary_max_sources: 8,
        },
    )
    .await
    .expect("archive should succeed");

    assert_eq!(outcome.archived_records, 2);
    assert_eq!(outcome.summary_records_upserted, 1);

    let records = SqliteMemoryStatsService::new(db)
        .list_scope_records("long_term")
        .await
        .expect("records should load");

    let archived = records
        .iter()
        .filter(|record| {
            matches!(
                record.metadata.get("status").and_then(serde_json::Value::as_str),
                Some("archived")
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(archived.len(), 2);
    assert!(archived.iter().all(|record| {
        record
            .metadata
            .get("archived_at")
            .and_then(serde_json::Value::as_i64)
            .is_some()
    }));
    let summary = records
        .iter()
        .find(|record| {
            record
                .metadata
                .get("summary")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
        })
        .expect("summary record should exist");
    assert_eq!(summary.metadata["status"], "active");
    assert_eq!(summary.metadata["priority"], "low");
    assert_eq!(summary.metadata["topic"], "work_city");
    assert_eq!(summary.metadata["source_ids"], serde_json::json!(["old-1", "old-2"]));
}

#[tokio::test(flavor = "current_thread")]
async fn archive_stale_long_term_memories_reuses_existing_summary_for_same_topic() {
    let db = create_db().await;
    let service = SqliteMemoryService::new(db.clone(), Some(Arc::new(MockEmbeddingProvider)))
        .await
        .expect("service should init");
    let now = now_ms();
    let forty_days_ms = 40_i64 * 24 * 60 * 60 * 1000;

    let _ = service
        .upsert(UpsertMemoryInput {
            id: Some("summary-1".to_string()),
            scope: "long_term".to_string(),
            content: "Archived summary for work_city.".to_string(),
            metadata: serde_json::json!({
                "kind": "fact",
                "priority": "low",
                "topic": "work_city",
                "status": "active",
                "summary": true,
                "source_ids": ["old-1"],
            }),
            pinned: false,
        })
        .await
        .expect("summary should upsert");
    let _ = service
        .upsert(UpsertMemoryInput {
            id: Some("old-2".to_string()),
            scope: "long_term".to_string(),
            content: "Moved to Shenzhen for work.".to_string(),
            metadata: serde_json::json!({
                "kind": "fact",
                "priority": "low",
                "topic": "work_city",
                "status": "active",
            }),
            pinned: false,
        })
        .await
        .expect("record should upsert");
    let _ = db
        .execute(
            "UPDATE memories SET updated_at_ms = ?1, created_at_ms = ?1 WHERE id = ?2",
            &[
                klaw_storage::DbValue::Integer(now - forty_days_ms),
                klaw_storage::DbValue::Text("old-2".to_string()),
            ],
        )
        .await
        .expect("timestamps should update");

    let outcome = archive_stale_long_term_memories(
        db.clone(),
        LongTermArchiveConfig {
            max_age_days: 30,
            summary_max_sources: 8,
        },
    )
    .await
    .expect("archive should succeed");

    assert_eq!(outcome.archived_records, 1);
    assert_eq!(outcome.summary_records_upserted, 1);

    let summary = service
        .get("summary-1")
        .await
        .expect("summary lookup should work")
        .expect("summary should exist");
    assert_eq!(
        summary.metadata["source_ids"],
        serde_json::json!(["old-1", "old-2"])
    );
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

#[tokio::test(flavor = "current_thread")]
async fn stats_service_lists_scope_records_in_detail_order() {
    let db = create_db().await;
    let service = SqliteMemoryService::new(db.clone(), Some(Arc::new(MockEmbeddingProvider)))
        .await
        .expect("service should init");

    let older = service
        .upsert(UpsertMemoryInput {
            id: Some("scope-old".to_string()),
            scope: "session:detail".to_string(),
            content: "older record".to_string(),
            metadata: serde_json::json!({"seq": 1}),
            pinned: false,
        })
        .await
        .expect("upsert should work");
    let newer = service
        .upsert(UpsertMemoryInput {
            id: Some("scope-new".to_string()),
            scope: "session:detail".to_string(),
            content: "newer record".to_string(),
            metadata: serde_json::json!({"seq": 2}),
            pinned: false,
        })
        .await
        .expect("upsert should work");
    let _pinned = service
        .upsert(UpsertMemoryInput {
            id: Some("scope-pinned".to_string()),
            scope: "session:detail".to_string(),
            content: "pinned record".to_string(),
            metadata: serde_json::json!({"seq": 3}),
            pinned: true,
        })
        .await
        .expect("upsert should work");

    assert!(newer.updated_at_ms >= older.updated_at_ms);

    let stats = SqliteMemoryStatsService::new(db);
    let records = stats
        .list_scope_records("session:detail")
        .await
        .expect("scope detail query should work");

    let ids = records
        .iter()
        .map(|record| record.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["scope-pinned", "scope-new", "scope-old"]);
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].metadata["seq"], 3);
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
