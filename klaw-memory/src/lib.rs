use async_trait::async_trait;
use klaw_config::{AppConfig, ModelProviderConfig};
use klaw_storage::{open_default_memory_db, DbValue, MemoryDb, StorageError};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    env,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use uuid::Uuid;

const RRF_K: f64 = 60.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    pub scope: String,
    pub content: String,
    pub metadata: Value,
    pub pinned: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct UpsertMemoryInput {
    pub id: Option<String>,
    pub scope: String,
    pub content: String,
    pub metadata: Value,
    pub pinned: bool,
}

#[derive(Debug, Clone)]
pub struct MemorySearchQuery {
    pub scope: Option<String>,
    pub text: String,
    pub limit: usize,
    pub fts_limit: usize,
    pub vector_limit: usize,
    pub use_vector: bool,
}

impl Default for MemorySearchQuery {
    fn default() -> Self {
        Self {
            scope: None,
            text: String::new(),
            limit: 8,
            fts_limit: 20,
            vector_limit: 20,
            use_vector: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemoryHit {
    pub record: MemoryRecord,
    pub fused_score: f64,
    pub bm25_rank: Option<usize>,
    pub vector_rank: Option<usize>,
}

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("invalid query: {0}")]
    InvalidQuery(String),
    #[error("embedding provider error: {0}")]
    Provider(String),
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("capability unavailable: {0}")]
    CapabilityUnavailable(String),
}

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn provider_name(&self) -> &str;
    fn model(&self) -> &str;
    async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, MemoryError>;
}

#[async_trait]
pub trait MemoryService: Send + Sync {
    async fn upsert(&self, input: UpsertMemoryInput) -> Result<MemoryRecord, MemoryError>;
    async fn search(&self, query: MemorySearchQuery) -> Result<Vec<MemoryHit>, MemoryError>;
    async fn get(&self, id: &str) -> Result<Option<MemoryRecord>, MemoryError>;
    async fn delete(&self, id: &str) -> Result<bool, MemoryError>;
    async fn pin(&self, id: &str, pinned: bool) -> Result<Option<MemoryRecord>, MemoryError>;
}

pub struct SqliteMemoryService {
    db: Arc<dyn MemoryDb>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    fts_enabled: bool,
    vector_enabled: bool,
}

impl SqliteMemoryService {
    pub async fn open_default(config: &AppConfig) -> Result<Self, MemoryError> {
        let db = open_default_memory_db().await?;
        let provider = if config.memory.embedding.enabled {
            build_embedding_provider_from_config(config).ok()
        } else {
            None
        };
        Self::new(Arc::new(db), provider).await
    }

    pub async fn new(
        db: Arc<dyn MemoryDb>,
        embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    ) -> Result<Self, MemoryError> {
        let mut service = Self {
            db,
            embedding_provider,
            fts_enabled: false,
            vector_enabled: false,
        };
        service.init_schema().await?;
        service.fts_enabled = service.try_enable_fts().await;
        service.vector_enabled =
            service.embedding_provider.is_some() && service.try_enable_vector_index().await;
        Ok(service)
    }

    async fn init_schema(&self) -> Result<(), MemoryError> {
        self.db
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS memories (
                    id TEXT PRIMARY KEY,
                    scope TEXT NOT NULL,
                    content TEXT NOT NULL,
                    metadata_json TEXT NOT NULL,
                    pinned INTEGER NOT NULL DEFAULT 0,
                    embedding BLOB,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                )",
            )
            .await?;
        Ok(())
    }

    async fn try_enable_fts(&self) -> bool {
        self.db
            .execute_batch(
                "CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
                    id UNINDEXED,
                    content
                )",
            )
            .await
            .is_ok()
    }

    async fn try_enable_vector_index(&self) -> bool {
        self.db
            .execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_memories_embedding
                 ON memories(libsql_vector_idx(embedding))",
            )
            .await
            .is_ok()
    }

    async fn try_embed_one(&self, content: &str) -> Option<Vec<u8>> {
        let provider = self.embedding_provider.as_ref()?;
        let response = provider.embed_texts(vec![content.to_string()]).await.ok()?;
        let vector = response.into_iter().next()?;
        Some(f32_vec_to_blob(&vector))
    }
}

#[async_trait]
impl MemoryService for SqliteMemoryService {
    async fn upsert(&self, input: UpsertMemoryInput) -> Result<MemoryRecord, MemoryError> {
        if input.scope.trim().is_empty() {
            return Err(MemoryError::InvalidQuery(
                "scope cannot be empty".to_string(),
            ));
        }
        if input.content.trim().is_empty() {
            return Err(MemoryError::InvalidQuery(
                "content cannot be empty".to_string(),
            ));
        }
        let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let now = now_ms();
        let metadata_json = serde_json::to_string(&input.metadata)?;
        let vector_blob = self.try_embed_one(&input.content).await;

        self.db
            .execute(
                "INSERT INTO memories (
                    id, scope, content, metadata_json, pinned, embedding, created_at_ms, updated_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET
                    scope=excluded.scope,
                    content=excluded.content,
                    metadata_json=excluded.metadata_json,
                    pinned=excluded.pinned,
                    embedding=excluded.embedding,
                    updated_at_ms=excluded.updated_at_ms",
                &[
                    DbValue::Text(id.clone()),
                    DbValue::Text(input.scope.clone()),
                    DbValue::Text(input.content.clone()),
                    DbValue::Text(metadata_json),
                    DbValue::Integer(if input.pinned { 1 } else { 0 }),
                    vector_blob.map(DbValue::Blob).unwrap_or(DbValue::Null),
                    DbValue::Integer(now),
                    DbValue::Integer(now),
                ],
            )
            .await?;

        if self.fts_enabled {
            self.db
                .execute(
                    "DELETE FROM memories_fts WHERE id = ?1",
                    &[DbValue::Text(id.clone())],
                )
                .await?;
            self.db
                .execute(
                    "INSERT INTO memories_fts (id, content) VALUES (?1, ?2)",
                    &[DbValue::Text(id.clone()), DbValue::Text(input.content)],
                )
                .await?;
        }

        self.get(&id).await?.ok_or_else(|| {
            MemoryError::Storage(StorageError::backend("upsert succeeded but record missing"))
        })
    }

    async fn search(&self, query: MemorySearchQuery) -> Result<Vec<MemoryHit>, MemoryError> {
        if query.text.trim().is_empty() {
            return Err(MemoryError::InvalidQuery(
                "search text cannot be empty".to_string(),
            ));
        }
        let limit = query.limit.max(1);
        let fts_rows = if self.fts_enabled {
            if let Some(scope) = query.scope.as_ref() {
                self.db
                    .query(
                        "SELECT m.id, m.scope, m.content, m.metadata_json, m.pinned, m.created_at_ms, m.updated_at_ms, bm25(memories_fts)
                         FROM memories_fts
                         JOIN memories m ON m.id = memories_fts.id
                         WHERE memories_fts MATCH ?1 AND m.scope = ?2
                         ORDER BY bm25(memories_fts) ASC
                         LIMIT ?3",
                        &[
                            DbValue::Text(query.text.clone()),
                            DbValue::Text(scope.clone()),
                            DbValue::Integer(query.fts_limit.max(limit) as i64),
                        ],
                    )
                    .await?
            } else {
                self.db
                    .query(
                        "SELECT m.id, m.scope, m.content, m.metadata_json, m.pinned, m.created_at_ms, m.updated_at_ms, bm25(memories_fts)
                         FROM memories_fts
                         JOIN memories m ON m.id = memories_fts.id
                         WHERE memories_fts MATCH ?1
                         ORDER BY bm25(memories_fts) ASC
                         LIMIT ?2",
                        &[
                            DbValue::Text(query.text.clone()),
                            DbValue::Integer(query.fts_limit.max(limit) as i64),
                        ],
                    )
                    .await?
            }
        } else if let Some(scope) = query.scope.as_ref() {
            self.db
                .query(
                    "SELECT id, scope, content, metadata_json, pinned, created_at_ms, updated_at_ms, 0.0
                     FROM memories
                     WHERE scope = ?1 AND content LIKE ?2
                     ORDER BY updated_at_ms DESC
                     LIMIT ?3",
                    &[
                        DbValue::Text(scope.clone()),
                        DbValue::Text(format!("%{}%", query.text)),
                        DbValue::Integer(query.fts_limit.max(limit) as i64),
                    ],
                )
                .await?
        } else {
            self.db
                .query(
                    "SELECT id, scope, content, metadata_json, pinned, created_at_ms, updated_at_ms, 0.0
                     FROM memories
                     WHERE content LIKE ?1
                     ORDER BY updated_at_ms DESC
                     LIMIT ?2",
                    &[
                        DbValue::Text(format!("%{}%", query.text)),
                        DbValue::Integer(query.fts_limit.max(limit) as i64),
                    ],
                )
                .await?
        };

        let mut bm25_rank = BTreeMap::new();
        let mut records_by_id = HashMap::new();
        for (idx, row) in fts_rows.iter().enumerate() {
            let record = row_to_record(row)?;
            bm25_rank.insert(record.id.clone(), idx + 1);
            records_by_id.insert(record.id.clone(), record);
        }

        let mut vector_rank = BTreeMap::new();
        if query.use_vector && self.vector_enabled {
            let query_blob = match self.try_embed_one(&query.text).await {
                Some(blob) => blob,
                None => Vec::new(),
            };
            if !query_blob.is_empty() {
                let vector_rows = self
                    .db
                    .query(
                        "SELECT id, distance
                         FROM vector_top_k('idx_memories_embedding', ?1, ?2)",
                        &[
                            DbValue::Blob(query_blob),
                            DbValue::Integer(query.vector_limit.max(limit) as i64),
                        ],
                    )
                    .await?;

                for (idx, row) in vector_rows.iter().enumerate() {
                    let id = db_string(row.get(0), "vector_top_k.id")?;
                    vector_rank.insert(id.clone(), idx + 1);
                    if !records_by_id.contains_key(&id) {
                        if let Some(record) = self.get(&id).await? {
                            if query
                                .scope
                                .as_ref()
                                .map(|scope| scope == &record.scope)
                                .unwrap_or(true)
                            {
                                records_by_id.insert(id, record);
                            }
                        }
                    }
                }
            }
        }

        let mut hits: Vec<MemoryHit> = records_by_id
            .into_values()
            .map(|record| {
                let b_rank = bm25_rank.get(&record.id).copied();
                let v_rank = vector_rank.get(&record.id).copied();
                let score = rrf_score(b_rank, v_rank);
                MemoryHit {
                    record,
                    fused_score: score,
                    bm25_rank: b_rank,
                    vector_rank: v_rank,
                }
            })
            .collect();

        hits.sort_by(|a, b| {
            b.record
                .pinned
                .cmp(&a.record.pinned)
                .then_with(|| b.fused_score.total_cmp(&a.fused_score))
                .then_with(|| b.record.updated_at_ms.cmp(&a.record.updated_at_ms))
        });
        hits.truncate(limit);
        Ok(hits)
    }

    async fn get(&self, id: &str) -> Result<Option<MemoryRecord>, MemoryError> {
        let rows = self
            .db
            .query(
                "SELECT id, scope, content, metadata_json, pinned, created_at_ms, updated_at_ms
                 FROM memories
                 WHERE id = ?1
                 LIMIT 1",
                &[DbValue::Text(id.to_string())],
            )
            .await?;
        match rows.first() {
            Some(row) => Ok(Some(row_to_record(row)?)),
            None => Ok(None),
        }
    }

    async fn delete(&self, id: &str) -> Result<bool, MemoryError> {
        let affected = self
            .db
            .execute(
                "DELETE FROM memories WHERE id = ?1",
                &[DbValue::Text(id.to_string())],
            )
            .await?;
        if self.fts_enabled {
            let _ = self
                .db
                .execute(
                    "DELETE FROM memories_fts WHERE id = ?1",
                    &[DbValue::Text(id.to_string())],
                )
                .await?;
        }
        Ok(affected > 0)
    }

    async fn pin(&self, id: &str, pinned: bool) -> Result<Option<MemoryRecord>, MemoryError> {
        let updated = self
            .db
            .execute(
                "UPDATE memories SET pinned = ?1, updated_at_ms = ?2 WHERE id = ?3",
                &[
                    DbValue::Integer(if pinned { 1 } else { 0 }),
                    DbValue::Integer(now_ms()),
                    DbValue::Text(id.to_string()),
                ],
            )
            .await?;
        if updated == 0 {
            return Ok(None);
        }
        self.get(id).await
    }
}

pub fn build_embedding_provider_from_config(
    config: &AppConfig,
) -> Result<Arc<dyn EmbeddingProvider>, MemoryError> {
    let provider_id = config.memory.embedding.provider.trim();
    if provider_id.is_empty() {
        return Err(MemoryError::InvalidConfig(
            "memory.embedding.provider cannot be empty".to_string(),
        ));
    }
    let model = config.memory.embedding.model.trim();
    if model.is_empty() {
        return Err(MemoryError::InvalidConfig(
            "memory.embedding.model cannot be empty".to_string(),
        ));
    }
    let provider_cfg = config.model_providers.get(provider_id).ok_or_else(|| {
        MemoryError::InvalidConfig(format!(
            "memory.embedding.provider '{}' not found in model_providers",
            provider_id
        ))
    })?;
    let api_key = resolve_api_key(provider_cfg).ok_or_else(|| {
        MemoryError::InvalidConfig(format!(
            "provider '{}' requires api_key or env_key",
            provider_id
        ))
    })?;
    let provider = OpenAiEmbeddingProvider {
        provider_name: provider_id.to_string(),
        base_url: provider_cfg.base_url.clone(),
        model: model.to_string(),
        api_key,
        client: Client::new(),
    };
    Ok(Arc::new(provider))
}

#[derive(Debug, Clone)]
pub struct OpenAiEmbeddingProvider {
    provider_name: String,
    base_url: String,
    model: String,
    api_key: String,
    client: Client,
}

#[derive(Debug, Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[async_trait]
impl EmbeddingProvider for OpenAiEmbeddingProvider {
    fn provider_name(&self) -> &str {
        &self.provider_name
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, MemoryError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let endpoint = format!("{}/embeddings", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(&self.api_key)
            .json(&EmbeddingRequest {
                model: &self.model,
                input: &texts,
            })
            .send()
            .await
            .map_err(|err| MemoryError::Provider(format!("request failed: {err}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read body>".to_string());
            return Err(MemoryError::Provider(format!(
                "embedding API returned {status}: {body}"
            )));
        }
        let parsed: EmbeddingResponse = response
            .json()
            .await
            .map_err(|err| MemoryError::Provider(format!("invalid response payload: {err}")))?;
        Ok(parsed.data.into_iter().map(|item| item.embedding).collect())
    }
}

fn resolve_api_key(provider: &ModelProviderConfig) -> Option<String> {
    provider.api_key.clone().or_else(|| {
        provider
            .env_key
            .as_ref()
            .and_then(|env_name| env::var(env_name).ok())
    })
}

fn row_to_record(row: &klaw_storage::DbRow) -> Result<MemoryRecord, MemoryError> {
    Ok(MemoryRecord {
        id: db_string(row.get(0), "memories.id")?,
        scope: db_string(row.get(1), "memories.scope")?,
        content: db_string(row.get(2), "memories.content")?,
        metadata: serde_json::from_str(&db_string(row.get(3), "memories.metadata_json")?)?,
        pinned: db_i64(row.get(4), "memories.pinned")? > 0,
        created_at_ms: db_i64(row.get(5), "memories.created_at_ms")?,
        updated_at_ms: db_i64(row.get(6), "memories.updated_at_ms")?,
    })
}

fn db_string(value: Option<&DbValue>, field: &str) -> Result<String, MemoryError> {
    match value {
        Some(DbValue::Text(v)) => Ok(v.clone()),
        Some(DbValue::Integer(v)) => Ok(v.to_string()),
        Some(DbValue::Real(v)) => Ok(v.to_string()),
        Some(DbValue::Null) | None => Err(MemoryError::Storage(StorageError::backend(format!(
            "missing text field: {field}"
        )))),
        Some(DbValue::Blob(_)) => Err(MemoryError::Storage(StorageError::backend(format!(
            "blob value cannot be parsed as text for field: {field}"
        )))),
    }
}

fn db_i64(value: Option<&DbValue>, field: &str) -> Result<i64, MemoryError> {
    match value {
        Some(DbValue::Integer(v)) => Ok(*v),
        Some(DbValue::Text(v)) => v.parse::<i64>().map_err(|err| {
            MemoryError::Storage(StorageError::backend(format!(
                "failed to parse integer for {field}: {err}"
            )))
        }),
        Some(DbValue::Real(v)) => Ok(*v as i64),
        Some(DbValue::Null) | None => Err(MemoryError::Storage(StorageError::backend(format!(
            "missing integer field: {field}"
        )))),
        Some(DbValue::Blob(_)) => Err(MemoryError::Storage(StorageError::backend(format!(
            "blob value cannot be parsed as integer for field: {field}"
        )))),
    }
}

fn rrf_score(bm25_rank: Option<usize>, vector_rank: Option<usize>) -> f64 {
    let mut score = 0.0;
    if let Some(rank) = bm25_rank {
        score += 1.0 / (RRF_K + rank as f64);
    }
    if let Some(rank) = vector_rank {
        score += 1.0 / (RRF_K + rank as f64);
    }
    score
}

fn f32_vec_to_blob(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<u8>>()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_storage::{DefaultMemoryDb, StoragePaths};
    use std::{
        collections::BTreeMap,
        sync::atomic::{AtomicU64, Ordering},
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

        async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, MemoryError> {
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
            tools: klaw_config::ToolsConfig::default(),
            cron: klaw_config::CronConfig::default(),
            skills: klaw_config::SkillsConfig::default(),
        };

        let provider = build_embedding_provider_from_config(&config).expect("provider build");
        assert_eq!(provider.provider_name(), "openai");
        assert_eq!(provider.model(), "text-embedding-3-small");
    }
}
