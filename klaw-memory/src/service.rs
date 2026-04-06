use crate::{
    EmbeddingProvider, MemoryError, MemoryHit, MemoryRecord, MemorySearchQuery, MemoryService,
    UpsertMemoryInput, build_embedding_provider_from_config,
    util::{db_string, f32_vec_to_blob, now_ms, row_to_record, rrf_score},
};
use async_trait::async_trait;
use klaw_config::AppConfig;
use klaw_storage::{DbRow, DbValue, MemoryDb, StorageError, open_default_memory_db};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};
use uuid::Uuid;

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

    async fn fts_search_rows(
        &self,
        text: &str,
        scope: Option<&str>,
        row_limit: i64,
    ) -> Result<Vec<DbRow>, MemoryError> {
        let rows = if let Some(scope) = scope {
            self.db
                .query(
                    "SELECT m.id, m.scope, m.content, m.metadata_json, m.pinned, m.created_at_ms, m.updated_at_ms, bm25(memories_fts)
                     FROM memories_fts
                     JOIN memories m ON m.id = memories_fts.id
                     WHERE memories_fts MATCH ?1 AND m.scope = ?2
                     ORDER BY bm25(memories_fts) ASC
                     LIMIT ?3",
                    &[
                        DbValue::Text(text.to_string()),
                        DbValue::Text(scope.to_string()),
                        DbValue::Integer(row_limit),
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
                    &[DbValue::Text(text.to_string()), DbValue::Integer(row_limit)],
                )
                .await?
        };
        Ok(rows)
    }

    async fn like_search_rows(
        &self,
        text: &str,
        scope: Option<&str>,
        row_limit: i64,
    ) -> Result<Vec<DbRow>, MemoryError> {
        let pattern = format!("%{text}%");
        let rows = if let Some(scope) = scope {
            self.db
                .query(
                    "SELECT id, scope, content, metadata_json, pinned, created_at_ms, updated_at_ms, 0.0
                     FROM memories
                     WHERE scope = ?1 AND content LIKE ?2
                     ORDER BY updated_at_ms DESC
                     LIMIT ?3",
                    &[
                        DbValue::Text(scope.to_string()),
                        DbValue::Text(pattern),
                        DbValue::Integer(row_limit),
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
                    &[DbValue::Text(pattern), DbValue::Integer(row_limit)],
                )
                .await?
        };
        Ok(rows)
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

    async fn list_scope_records(&self, scope: &str) -> Result<Vec<MemoryRecord>, MemoryError> {
        let rows = self
            .db
            .query(
                "SELECT id, scope, content, metadata_json, pinned, created_at_ms, updated_at_ms
                 FROM memories
                 WHERE scope = ?1
                 ORDER BY pinned DESC, updated_at_ms DESC, created_at_ms DESC, id ASC",
                &[DbValue::Text(scope.to_string())],
            )
            .await?;

        rows.iter().map(row_to_record).collect()
    }

    async fn search(&self, query: MemorySearchQuery) -> Result<Vec<MemoryHit>, MemoryError> {
        if query.text.trim().is_empty() {
            return Err(MemoryError::InvalidQuery(
                "search text cannot be empty".to_string(),
            ));
        }

        let limit = query.limit.max(1);
        let candidate_limit = query.fts_limit.max(limit) as i64;
        let scope = query.scope.as_deref();
        let fts_rows = if self.fts_enabled {
            self.fts_search_rows(&query.text, scope, candidate_limit)
                .await?
        } else {
            self.like_search_rows(&query.text, scope, candidate_limit)
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
                MemoryHit {
                    record,
                    fused_score: rrf_score(b_rank, v_rank),
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
