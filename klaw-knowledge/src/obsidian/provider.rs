use std::{
    cmp::Ordering,
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use klaw_model::QueryIntent;
use klaw_storage::{DatabaseExecutor, DbRow, DbValue};
use serde_json::{Value, json};

use crate::{
    KnowledgeEntry, KnowledgeError, KnowledgeHit, KnowledgeProvider, KnowledgeSearchQuery,
    KnowledgeSourceInfo, KnowledgeStatus, KnowledgeSyncProgress,
    models::{EmbeddingModel, KnowledgeOrchestration, OrchestratorModel, RerankModel},
    obsidian::indexer::{
        KNOWLEDGE_VECTOR_INDEX_NAME, embed_missing_chunks, embed_missing_chunks_with_progress,
        ensure_vector_index, has_indexed_entries, has_vector_index, index_note_path, index_vault,
        index_vault_with_progress, init_schema, remove_note_path, serialize_embedding,
    },
    obsidian::watcher::{AutoIndexWatcher, start_auto_index_watcher},
    retrieval::fusion::{RankedHit, weighted_reciprocal_rank_fuse},
};

#[derive(Clone)]
pub struct ObsidianKnowledgeProvider {
    db: Arc<dyn DatabaseExecutor>,
    vault_root: PathBuf,
    exclude_folders: Vec<String>,
    max_excerpt_length: usize,
    source_name: String,
    fts_virtual: bool,
    vector_index_enabled: Arc<Mutex<bool>>,
    embedder: Option<Arc<dyn EmbeddingModel>>,
    reranker: Option<Arc<dyn RerankModel>>,
    orchestrator: Option<Arc<dyn OrchestratorModel>>,
}

impl ObsidianKnowledgeProvider {
    pub async fn open(
        db: Arc<dyn DatabaseExecutor>,
        vault_root: PathBuf,
        exclude_folders: Vec<String>,
        max_excerpt_length: usize,
        source_name: impl Into<String>,
    ) -> Result<Self, KnowledgeError> {
        init_schema(&db).await?;
        let fts_virtual = detect_virtual_fts(&db).await?;
        let vector_index_enabled = has_vector_index(&db).await?;
        let provider = Self {
            db,
            vault_root,
            exclude_folders,
            max_excerpt_length,
            source_name: source_name.into(),
            fts_virtual,
            vector_index_enabled: Arc::new(Mutex::new(vector_index_enabled)),
            embedder: None,
            reranker: None,
            orchestrator: None,
        };
        Ok(provider)
    }

    pub fn with_orchestrator(mut self, orchestrator: Arc<dyn OrchestratorModel>) -> Self {
        self.orchestrator = Some(orchestrator);
        self
    }

    pub fn with_embedding_model(mut self, embedder: Arc<dyn EmbeddingModel>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    pub fn with_reranker(mut self, reranker: Arc<dyn RerankModel>) -> Self {
        self.reranker = Some(reranker);
        self
    }

    pub async fn reindex(&self) -> Result<usize, KnowledgeError> {
        let indexed = index_vault(
            self.db.clone(),
            &self.vault_root,
            &self.exclude_folders,
            self.max_excerpt_length,
            self.embedder.as_deref(),
        )
        .await?;
        self.refresh_vector_index_enabled().await?;
        Ok(indexed)
    }

    pub fn vault_root(&self) -> &Path {
        &self.vault_root
    }

    pub fn exclude_folders(&self) -> &[String] {
        &self.exclude_folders
    }

    pub async fn index_path(&self, absolute_path: &Path) -> Result<bool, KnowledgeError> {
        let indexed = index_note_path(
            self.db.clone(),
            &self.vault_root,
            absolute_path,
            self.max_excerpt_length,
            self.embedder.as_deref(),
        )
        .await?;
        self.refresh_vector_index_enabled().await?;
        Ok(indexed)
    }

    pub async fn remove_path(&self, relative_path: &str) -> Result<(), KnowledgeError> {
        remove_note_path(self.db.clone(), relative_path).await
    }

    pub async fn has_indexed_entries(&self) -> Result<bool, KnowledgeError> {
        has_indexed_entries(self.db.clone()).await
    }

    pub async fn reconcile_existing_index(&self) -> Result<usize, KnowledgeError> {
        if !self.has_indexed_entries().await? {
            return Ok(0);
        }
        self.reindex().await
    }

    pub fn start_auto_index_watcher(&self) -> Result<AutoIndexWatcher, KnowledgeError> {
        start_auto_index_watcher(self.clone())
    }

    pub async fn reindex_with_progress<F>(&self, progress: F) -> Result<usize, KnowledgeError>
    where
        F: FnMut(KnowledgeSyncProgress),
    {
        let indexed = index_vault_with_progress(
            self.db.clone(),
            &self.vault_root,
            &self.exclude_folders,
            self.max_excerpt_length,
            self.embedder.as_deref(),
            progress,
        )
        .await?;
        self.refresh_vector_index_enabled().await?;
        Ok(indexed)
    }

    pub async fn embed_missing_chunks(&self) -> Result<usize, KnowledgeError> {
        let Some(embedder) = self.embedder.as_deref() else {
            return Ok(0);
        };
        let embedded = embed_missing_chunks(self.db.clone(), embedder).await?;
        self.refresh_vector_index_enabled().await?;
        Ok(embedded)
    }

    pub async fn embed_missing_chunks_with_progress<F>(
        &self,
        progress: F,
    ) -> Result<usize, KnowledgeError>
    where
        F: FnMut(KnowledgeSyncProgress),
    {
        let Some(embedder) = self.embedder.as_deref() else {
            return Ok(0);
        };
        let embedded =
            embed_missing_chunks_with_progress(self.db.clone(), embedder, progress).await?;
        self.refresh_vector_index_enabled().await?;
        Ok(embedded)
    }

    pub async fn status(&self, enabled: bool) -> Result<KnowledgeStatus, KnowledgeError> {
        let entry_count = self.count_rows("knowledge_entries").await?;
        let chunk_count = self.count_rows("knowledge_chunks").await?;
        let embedded_chunk_count = self.count_embedded_chunks().await?;
        Ok(KnowledgeStatus {
            enabled,
            provider: "obsidian".to_string(),
            source_name: self.source_name.clone(),
            vault_path: Some(self.vault_root.to_string_lossy().to_string()),
            entry_count,
            chunk_count,
            embedded_chunk_count,
            missing_embedding_count: chunk_count.saturating_sub(embedded_chunk_count),
        })
    }

    async fn fts_lane(&self, query: &str, limit: usize) -> Result<Vec<RankedHit>, KnowledgeError> {
        if self.fts_virtual {
            let rows = self
                .db
                .query(
                    "SELECT e.id, e.title, c.snippet, bm25(knowledge_fts)
                     FROM knowledge_fts
                     JOIN knowledge_entries e ON e.id = knowledge_fts.entry_id
                     JOIN knowledge_chunks c ON c.id = knowledge_fts.chunk_id
                     WHERE knowledge_fts MATCH ?1
                     ORDER BY bm25(knowledge_fts) ASC
                     LIMIT ?2",
                    &[
                        DbValue::Text(query.to_string()),
                        DbValue::Integer(limit as i64),
                    ],
                )
                .await
                .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
            return Ok(dedup_ranked_hits(rows, |row| {
                let rank = real_at(row, 3).unwrap_or(1.0);
                RankedHit {
                    id: text_at(row, 0).unwrap_or_default(),
                    title: text_at(row, 1).unwrap_or_default(),
                    excerpt: text_at(row, 2).unwrap_or_default(),
                    score: 1.0 / (rank.abs() + 1.0),
                }
            }));
        }

        let rows = self
            .db
            .query(
                "SELECT e.id, e.title, c.snippet
                 FROM knowledge_fts f
                 JOIN knowledge_entries e ON e.id = f.entry_id
                 JOIN knowledge_chunks c ON c.id = f.chunk_id
                 LIMIT ?1",
                &[DbValue::Integer((limit * 20) as i64)],
            )
            .await
            .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
        let tokens = tokenize_query(query);
        let ranked = rows
            .into_iter()
            .filter_map(|row| {
                let title = text_at(&row, 1).unwrap_or_default();
                let excerpt = text_at(&row, 2).unwrap_or_default();
                let haystack = format!("{title} {excerpt}").to_ascii_lowercase();
                let matches = tokens
                    .iter()
                    .filter(|token| haystack.contains(token.as_str()))
                    .count();
                (matches > 0).then(|| RankedHit {
                    id: text_at(&row, 0).unwrap_or_default(),
                    title,
                    excerpt,
                    score: matches as f64,
                })
            })
            .collect();
        Ok(dedup_ranked_hits_from_vec(ranked))
    }

    async fn graph_lane(
        &self,
        seeds: &[RankedHit],
        limit: usize,
    ) -> Result<Vec<RankedHit>, KnowledgeError> {
        let mut hits = Vec::new();
        for seed in seeds.iter().take(limit) {
            let outgoing = self
                .db
                .query(
                    "SELECT DISTINCT target.id, target.title, substr(target.content, 1, 400)
                     FROM knowledge_links link
                     JOIN knowledge_entries target ON (
                        target.id = link.target_entry_id
                        OR (
                            link.target_entry_id IS NULL
                            AND lower(target.title) = lower(link.target_title)
                        )
                     )
                     WHERE link.source_entry_id = ?1 AND target.id != link.source_entry_id
                     LIMIT ?2",
                    &[
                        DbValue::Text(seed.id.clone()),
                        DbValue::Integer(limit as i64),
                    ],
                )
                .await
                .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
            hits.extend(outgoing.into_iter().filter_map(|row| {
                Some(RankedHit {
                    id: text_at(&row, 0)?,
                    title: text_at(&row, 1)?,
                    excerpt: text_at(&row, 2)?,
                    score: 0.5,
                })
            }));
        }
        Ok(dedup_ranked_hits_from_vec(hits))
    }

    async fn temporal_lane(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<RankedHit>, KnowledgeError> {
        let Some(pattern) = temporal_pattern(query) else {
            return Ok(Vec::new());
        };
        let rows = self
            .db
            .query(
                "SELECT id, title, substr(content, 1, 400)
                 FROM knowledge_entries
                 WHERE note_date LIKE ?1
                 ORDER BY updated_at_ms DESC
                 LIMIT ?2",
                &[DbValue::Text(pattern), DbValue::Integer(limit as i64)],
            )
            .await
            .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
        Ok(dedup_ranked_hits(rows, |row| RankedHit {
            id: text_at(row, 0).unwrap_or_default(),
            title: text_at(row, 1).unwrap_or_default(),
            excerpt: text_at(row, 2).unwrap_or_default(),
            score: 0.4,
        }))
    }

    async fn semantic_lane(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<RankedHit>, KnowledgeError> {
        let Some(embedder) = self.embedder.as_ref() else {
            return Ok(Vec::new());
        };
        let query_vector = embedder.embed(query).await?;
        if query_vector.is_empty() {
            return Ok(Vec::new());
        }
        if !self.is_vector_index_enabled() {
            let enabled = ensure_vector_index(&self.db, query_vector.len()).await?;
            self.set_vector_index_enabled(enabled);
        }
        if self.is_vector_index_enabled() {
            match self.semantic_lane_vector_top_k(&query_vector, limit).await {
                Ok(hits) => return Ok(hits),
                Err(err) if is_vector_query_capability_error(&err.to_string()) => {
                    self.set_vector_index_enabled(false);
                }
                Err(err) => return Err(err),
            }
        }
        match self.semantic_lane_sql_distance(&query_vector, limit).await {
            Ok(hits) => return Ok(hits),
            Err(err) if is_vector_query_capability_error(&err.to_string()) => {}
            Err(err) => return Err(err),
        }
        self.semantic_lane_fallback(&query_vector, limit).await
    }

    async fn semantic_lane_vector_top_k(
        &self,
        query_vector: &[f32],
        limit: usize,
    ) -> Result<Vec<RankedHit>, KnowledgeError> {
        let rows = self
            .db
            .query(
                &format!(
                    "SELECT e.id, e.title, c.snippet, v.distance
                     FROM vector_top_k('{KNOWLEDGE_VECTOR_INDEX_NAME}', ?1, ?2) v
                     JOIN knowledge_chunks c ON c.rowid = v.id
                     JOIN knowledge_entries e ON e.id = c.entry_id
                     ORDER BY v.distance ASC"
                ),
                &[
                    DbValue::Blob(serialize_embedding(query_vector)),
                    DbValue::Integer(limit as i64),
                ],
            )
            .await
            .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
        Ok(dedup_ranked_hits(rows, |row| {
            let distance = real_at(row, 3).unwrap_or(1.0);
            RankedHit {
                id: text_at(row, 0).unwrap_or_default(),
                title: text_at(row, 1).unwrap_or_default(),
                excerpt: text_at(row, 2).unwrap_or_default(),
                score: 1.0 - distance,
            }
        }))
    }

    async fn semantic_lane_sql_distance(
        &self,
        query_vector: &[f32],
        limit: usize,
    ) -> Result<Vec<RankedHit>, KnowledgeError> {
        let rows = self
            .db
            .query(
                "SELECT e.id, e.title, c.snippet, vector_distance_cos(c.embedding, ?1) AS distance
                 FROM knowledge_chunks c
                 JOIN knowledge_entries e ON e.id = c.entry_id
                 WHERE c.embedding IS NOT NULL
                 ORDER BY distance ASC
                 LIMIT ?2",
                &[
                    DbValue::Blob(serialize_embedding(query_vector)),
                    DbValue::Integer(limit as i64),
                ],
            )
            .await
            .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
        Ok(dedup_ranked_hits(rows, |row| {
            let distance = real_at(row, 3).unwrap_or(1.0);
            RankedHit {
                id: text_at(row, 0).unwrap_or_default(),
                title: text_at(row, 1).unwrap_or_default(),
                excerpt: text_at(row, 2).unwrap_or_default(),
                score: 1.0 - distance,
            }
        }))
    }

    async fn semantic_lane_fallback(
        &self,
        query_vector: &[f32],
        limit: usize,
    ) -> Result<Vec<RankedHit>, KnowledgeError> {
        let rows = self
            .db
            .query(
                "SELECT e.id, e.title, c.snippet, c.embedding
                 FROM knowledge_chunks c
                 JOIN knowledge_entries e ON e.id = c.entry_id
                 WHERE c.embedding IS NOT NULL",
                &[],
            )
            .await
            .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
        let mut hits = rows
            .into_iter()
            .filter_map(|row| {
                let embedding = blob_at(&row, 3).and_then(|blob| deserialize_embedding(&blob));
                let score = embedding
                    .as_deref()
                    .map(|candidate| cosine_similarity(&query_vector, candidate))?;
                Some(RankedHit {
                    id: text_at(&row, 0)?,
                    title: text_at(&row, 1)?,
                    excerpt: text_at(&row, 2)?,
                    score,
                })
            })
            .collect::<Vec<_>>();
        hits.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.title.cmp(&right.title))
        });
        hits.truncate(limit);
        Ok(dedup_ranked_hits_from_vec(hits))
    }

    async fn entry_count(&self) -> Result<usize, KnowledgeError> {
        self.count_rows("knowledge_entries").await
    }

    async fn count_rows(&self, table: &str) -> Result<usize, KnowledgeError> {
        let sql = match table {
            "knowledge_entries" => "SELECT COUNT(*) FROM knowledge_entries",
            "knowledge_chunks" => "SELECT COUNT(*) FROM knowledge_chunks",
            _ => {
                return Err(KnowledgeError::Provider(format!(
                    "unsupported knowledge table `{table}`"
                )));
            }
        };
        let rows = self
            .db
            .query(sql, &[])
            .await
            .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
        Ok(rows.first().and_then(|row| integer_at(row, 0)).unwrap_or(0) as usize)
    }

    async fn count_embedded_chunks(&self) -> Result<usize, KnowledgeError> {
        let rows = self
            .db
            .query(
                "SELECT COUNT(*) FROM knowledge_chunks WHERE embedding IS NOT NULL",
                &[],
            )
            .await
            .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
        Ok(rows.first().and_then(|row| integer_at(row, 0)).unwrap_or(0) as usize)
    }
}

#[async_trait]
impl KnowledgeProvider for ObsidianKnowledgeProvider {
    fn provider_name(&self) -> &str {
        "obsidian"
    }

    async fn search(
        &self,
        query: KnowledgeSearchQuery,
    ) -> Result<Vec<KnowledgeHit>, KnowledgeError> {
        if query.text.trim().is_empty() {
            return Err(KnowledgeError::InvalidQuery(
                "knowledge search text cannot be empty".to_string(),
            ));
        }

        let limit = query.limit.max(1);
        let orchestration = self.orchestration_plan(&query.text).await?;
        let expanded_queries = orchestration.expansions;
        let semantic = self
            .expanded_semantic_lane(&expanded_queries, limit * 4)
            .await?;
        let fts = self.expanded_fts_lane(&expanded_queries, limit * 3).await?;
        let graph = self
            .graph_lane(&merge_seed_hits(&semantic, &fts), limit)
            .await?;
        let temporal = self.temporal_lane(&query.text, limit).await?;
        let weights = lane_weights_for_intent(orchestration.intent);
        let pass1 = weighted_reciprocal_rank_fuse(
            &[
                ("semantic", &semantic, weights.semantic),
                ("fts", &fts, weights.fts),
                ("graph", &graph, weights.graph),
                ("temporal", &temporal, weights.temporal),
            ],
            60,
        );
        let rerank = self.rerank_lane(&query.text, &pass1, limit * 4).await?;
        let fused = weighted_reciprocal_rank_fuse(
            &[
                ("semantic", &semantic, weights.semantic),
                ("fts", &fts, weights.fts),
                ("graph", &graph, weights.graph),
                ("temporal", &temporal, weights.temporal),
                ("rerank", &rerank, weights.rerank),
            ],
            60,
        );

        let fused_hits = fused.into_iter().take(limit).collect::<Vec<_>>();
        let entry_ids = fused_hits
            .iter()
            .map(|hit| hit.id.clone())
            .collect::<Vec<_>>();
        let entry_map = self.load_entry_metadata(&entry_ids).await?;

        Ok(fused_hits
            .into_iter()
            .map(|hit| {
                let (uri, tags_json, metadata_json) = entry_map
                    .get(&hit.id)
                    .cloned()
                    .unwrap_or_else(|| ("".to_string(), "[]".to_string(), "{}".to_string()));
                KnowledgeHit {
                    id: hit.id,
                    title: hit.title,
                    excerpt: hit.excerpt,
                    score: hit.score,
                    tags: serde_json::from_str(&tags_json).unwrap_or_default(),
                    uri,
                    source: "obsidian".to_string(),
                    metadata: merge_lane_metadata(metadata_json, &hit.lanes),
                }
            })
            .collect())
    }

    async fn get(&self, id: &str) -> Result<Option<KnowledgeEntry>, KnowledgeError> {
        let rows = self
            .db
            .query(
                "SELECT id, title, content, tags_json, uri, metadata_json, created_at_ms, updated_at_ms
                 FROM knowledge_entries
                 WHERE id = ?1 OR uri = ?1
                 LIMIT 1",
                &[DbValue::Text(id.to_string())],
            )
            .await
            .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        Ok(Some(KnowledgeEntry {
            id: text_at(row, 0).unwrap_or_default(),
            title: text_at(row, 1).unwrap_or_default(),
            content: text_at(row, 2).unwrap_or_default(),
            tags: serde_json::from_str(&text_at(row, 3).unwrap_or_else(|| "[]".to_string()))
                .unwrap_or_default(),
            uri: text_at(row, 4).unwrap_or_default(),
            source: "obsidian".to_string(),
            metadata: serde_json::from_str(&text_at(row, 5).unwrap_or_else(|| "{}".to_string()))
                .unwrap_or_else(|_| json!({})),
            created_at_ms: integer_at(row, 6).unwrap_or_default(),
            updated_at_ms: integer_at(row, 7).unwrap_or_default(),
        }))
    }

    async fn list_sources(&self) -> Result<Vec<KnowledgeSourceInfo>, KnowledgeError> {
        Ok(vec![KnowledgeSourceInfo {
            provider: "obsidian".to_string(),
            name: self.source_name.clone(),
            description: format!(
                "Local Obsidian vault at {}",
                self.vault_root.to_string_lossy()
            ),
            entry_count: self.entry_count().await?,
        }])
    }
}

impl ObsidianKnowledgeProvider {
    async fn load_entry_metadata(
        &self,
        ids: &[String],
    ) -> Result<BTreeMap<String, (String, String, String)>, KnowledgeError> {
        let mut unique_ids = Vec::new();
        for id in ids {
            if !unique_ids.contains(id) {
                unique_ids.push(id.clone());
            }
        }
        if unique_ids.is_empty() {
            return Ok(BTreeMap::new());
        }

        let placeholders = (1..=unique_ids.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT id, uri, tags_json, metadata_json
             FROM knowledge_entries
             WHERE id IN ({placeholders})"
        );
        let params = unique_ids
            .into_iter()
            .map(DbValue::Text)
            .collect::<Vec<_>>();
        let rows = self
            .db
            .query(&sql, &params)
            .await
            .map_err(|err| KnowledgeError::Provider(err.to_string()))?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                Some((
                    text_at(&row, 0)?,
                    (text_at(&row, 1)?, text_at(&row, 2)?, text_at(&row, 3)?),
                ))
            })
            .collect())
    }

    async fn refresh_vector_index_enabled(&self) -> Result<(), KnowledgeError> {
        let enabled = has_vector_index(&self.db).await?;
        self.set_vector_index_enabled(enabled);
        Ok(())
    }

    fn is_vector_index_enabled(&self) -> bool {
        *self
            .vector_index_enabled
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn set_vector_index_enabled(&self, enabled: bool) {
        *self
            .vector_index_enabled
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = enabled;
    }

    async fn orchestration_plan(
        &self,
        query: &str,
    ) -> Result<KnowledgeOrchestration, KnowledgeError> {
        let Some(orchestrator) = self.orchestrator.as_ref() else {
            return Ok(KnowledgeOrchestration {
                intent: QueryIntent::Exploratory,
                expansions: vec![query.to_string()],
            });
        };
        let mut orchestration = orchestrator.orchestrate(query).await?;
        let mut expansions = std::mem::take(&mut orchestration.expansions);
        if expansions.is_empty() {
            expansions.push(query.to_string());
        }
        if expansions.first().is_none_or(|value| value != query) {
            expansions.insert(0, query.to_string());
        }
        expansions.dedup();
        orchestration.expansions = expansions;
        Ok(orchestration)
    }

    async fn expanded_fts_lane(
        &self,
        expansions: &[String],
        limit: usize,
    ) -> Result<Vec<RankedHit>, KnowledgeError> {
        let mut combined = Vec::new();
        for expansion in expansions {
            combined.extend(self.fts_lane(expansion, limit).await?);
        }
        Ok(dedup_ranked_hits_from_vec(combined))
    }

    async fn expanded_semantic_lane(
        &self,
        expansions: &[String],
        limit: usize,
    ) -> Result<Vec<RankedHit>, KnowledgeError> {
        let mut combined = Vec::new();
        for expansion in expansions {
            combined.extend(self.semantic_lane(expansion, limit).await?);
        }
        Ok(dedup_ranked_hits_from_vec(combined))
    }

    async fn rerank_lane(
        &self,
        query: &str,
        fused_hits: &[crate::retrieval::fusion::FusedHit],
        limit: usize,
    ) -> Result<Vec<RankedHit>, KnowledgeError> {
        let Some(reranker) = self.reranker.as_ref() else {
            return Ok(Vec::new());
        };
        let candidates = fused_hits.iter().take(limit).collect::<Vec<_>>();
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        let candidate_texts = candidates
            .iter()
            .map(|candidate| candidate.excerpt.clone())
            .collect::<Vec<_>>();
        let scores = reranker.rerank(query, &candidate_texts).await?;
        let mut reranked = candidates
            .into_iter()
            .zip(scores.into_iter())
            .map(|(candidate, score)| RankedHit {
                id: candidate.id.clone(),
                title: candidate.title.clone(),
                excerpt: candidate.excerpt.clone(),
                score: score as f64,
            })
            .collect::<Vec<_>>();
        reranked.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.title.cmp(&right.title))
        });
        Ok(reranked)
    }
}

fn is_vector_query_capability_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("vector_top_k")
        || normalized.contains("libsql_vector_idx")
        || normalized.contains("no such function")
        || normalized.contains("no such module")
}

fn dedup_ranked_hits<F>(rows: Vec<DbRow>, map: F) -> Vec<RankedHit>
where
    F: Fn(&DbRow) -> RankedHit,
{
    dedup_ranked_hits_from_vec(rows.iter().map(map).collect())
}

fn dedup_ranked_hits_from_vec(hits: Vec<RankedHit>) -> Vec<RankedHit> {
    let mut by_id: BTreeMap<String, RankedHit> = BTreeMap::new();
    for hit in hits {
        let replace = match by_id.get(&hit.id) {
            Some(existing) => hit.score > existing.score,
            None => true,
        };
        if replace {
            by_id.insert(hit.id.clone(), hit);
        }
    }
    by_id.into_values().collect()
}

fn temporal_pattern(query: &str) -> Option<String> {
    let query = query.trim().to_ascii_lowercase();
    if query.contains("recent") || query.contains("latest") || query.contains("today") {
        return Some("%".to_string());
    }
    for (start, _) in query.char_indices() {
        for len in [10usize, 7usize] {
            let Some(candidate) = query.get(start..start + len) else {
                continue;
            };
            if is_date_like(candidate) {
                return Some(format!("{candidate}%"));
            }
        }
    }
    None
}

fn is_date_like(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    matches!(parts.as_slice(), [year, month] if year.len() == 4 && month.len() == 2)
        || matches!(parts.as_slice(), [year, month, day] if year.len() == 4 && month.len() == 2 && day.len() == 2)
}

fn tokenize_query(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| !token.is_empty())
        .collect()
}

fn merge_lane_metadata(metadata_json: String, lanes: &[String]) -> Value {
    let mut metadata = serde_json::from_str::<Value>(&metadata_json).unwrap_or_else(|_| json!({}));
    metadata["lanes"] = json!(lanes);
    metadata
}

#[derive(Debug, Clone, Copy)]
struct LaneWeights {
    semantic: f64,
    fts: f64,
    graph: f64,
    temporal: f64,
    rerank: f64,
}

fn lane_weights_for_intent(intent: QueryIntent) -> LaneWeights {
    match intent {
        QueryIntent::Temporal => LaneWeights {
            semantic: 1.1,
            fts: 1.0,
            graph: 0.8,
            temporal: 1.6,
            rerank: 4.0,
        },
        QueryIntent::Relationship => LaneWeights {
            semantic: 0.9,
            fts: 0.9,
            graph: 1.6,
            temporal: 0.5,
            rerank: 4.0,
        },
        QueryIntent::Exact => LaneWeights {
            semantic: 0.8,
            fts: 1.6,
            graph: 0.8,
            temporal: 0.5,
            rerank: 4.0,
        },
        QueryIntent::Conceptual => LaneWeights {
            semantic: 1.5,
            fts: 0.9,
            graph: 1.1,
            temporal: 0.5,
            rerank: 4.0,
        },
        QueryIntent::Exploratory => LaneWeights {
            semantic: 1.4,
            fts: 1.1,
            graph: 0.9,
            temporal: 0.5,
            rerank: 4.0,
        },
    }
}

fn merge_seed_hits(primary: &[RankedHit], secondary: &[RankedHit]) -> Vec<RankedHit> {
    let mut merged = primary.to_vec();
    merged.extend_from_slice(secondary);
    dedup_ranked_hits_from_vec(merged)
}

async fn detect_virtual_fts(db: &Arc<dyn DatabaseExecutor>) -> Result<bool, KnowledgeError> {
    let rows = db
        .query(
            "SELECT sql FROM sqlite_master WHERE type IN ('table', 'view') AND name = 'knowledge_fts' LIMIT 1",
            &[],
        )
        .await
        .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    Ok(rows
        .first()
        .and_then(|row| text_at(row, 0))
        .is_some_and(|sql| sql.to_ascii_uppercase().contains("VIRTUAL TABLE")))
}

fn text_at(row: &DbRow, index: usize) -> Option<String> {
    match row.get(index)? {
        DbValue::Text(value) => Some(value.clone()),
        _ => None,
    }
}

fn blob_at(row: &DbRow, index: usize) -> Option<Vec<u8>> {
    match row.get(index)? {
        DbValue::Blob(value) => Some(value.clone()),
        _ => None,
    }
}

fn integer_at(row: &DbRow, index: usize) -> Option<i64> {
    match row.get(index)? {
        DbValue::Integer(value) => Some(*value),
        _ => None,
    }
}

fn real_at(row: &DbRow, index: usize) -> Option<f64> {
    match row.get(index)? {
        DbValue::Real(value) => Some(*value),
        DbValue::Integer(value) => Some(*value as f64),
        _ => None,
    }
}

fn deserialize_embedding(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.len() % std::mem::size_of::<f32>() != 0 {
        return None;
    }
    let mut values = Vec::with_capacity(bytes.len() / std::mem::size_of::<f32>());
    for chunk in bytes.chunks_exact(std::mem::size_of::<f32>()) {
        values.push(f32::from_le_bytes(chunk.try_into().ok()?));
    }
    Some(values)
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut left_norm = 0.0f64;
    let mut right_norm = 0.0f64;
    for (left_value, right_value) in left.iter().zip(right.iter()) {
        let left_value = *left_value as f64;
        let right_value = *right_value as f64;
        dot += left_value * right_value;
        left_norm += left_value * left_value;
        right_norm += right_value * right_value;
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }
    dot / (left_norm.sqrt() * right_norm.sqrt())
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    };

    use klaw_storage::{DefaultKnowledgeDb, StorageError, StoragePaths};

    use super::*;

    #[derive(Default)]
    struct RecordingOrchestrator {
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl OrchestratorModel for RecordingOrchestrator {
        async fn orchestrate(&self, query: &str) -> Result<KnowledgeOrchestration, KnowledgeError> {
            self.calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(query.to_string());
            Ok(KnowledgeOrchestration {
                intent: QueryIntent::Conceptual,
                expansions: vec![query.to_string(), "token rotation".to_string()],
            })
        }
    }

    #[derive(Default)]
    struct MockEmbeddingModel;

    #[async_trait]
    impl EmbeddingModel for MockEmbeddingModel {
        async fn embed(&self, text: &str) -> Result<Vec<f32>, KnowledgeError> {
            let normalized = text.to_ascii_lowercase();
            let score = |terms: &[&str]| -> f32 {
                terms
                    .iter()
                    .filter(|term| normalized.contains(**term))
                    .count() as f32
            };
            Ok(vec![
                score(&["auth", "oauth", "login"]),
                score(&["cookie", "browser", "state"]),
                score(&["session", "storage", "persist"]),
            ])
        }
    }

    #[derive(Default)]
    struct RecordingDb {
        sql: Arc<Mutex<Vec<String>>>,
        fail_vector_index: bool,
        fail_vector_distance: bool,
    }

    #[async_trait]
    impl DatabaseExecutor for RecordingDb {
        async fn execute_batch(&self, sql: &str) -> Result<(), StorageError> {
            self.sql
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(sql.to_string());
            if self.fail_vector_index && sql.contains("libsql_vector_idx") {
                return Err(StorageError::backend("no such function: libsql_vector_idx"));
            }
            Ok(())
        }

        async fn execute(&self, sql: &str, _params: &[DbValue]) -> Result<u64, StorageError> {
            self.sql
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(sql.to_string());
            Ok(1)
        }

        async fn query(&self, sql: &str, _params: &[DbValue]) -> Result<Vec<DbRow>, StorageError> {
            self.sql
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(sql.to_string());
            if self.fail_vector_distance
                && (sql.contains("vector_top_k") || sql.contains("vector_distance_cos"))
            {
                return Err(StorageError::backend(
                    "no such function: vector_distance_cos",
                ));
            }
            if sql.contains("vector_top_k") {
                return Ok(vec![DbRow {
                    values: vec![
                        DbValue::Text("Cookies.md".to_string()),
                        DbValue::Text("Cookies".to_string()),
                        DbValue::Text("Cookie storage details.".to_string()),
                        DbValue::Real(0.2),
                    ],
                }]);
            }
            if sql.contains("vector_distance_cos") {
                return Ok(vec![DbRow {
                    values: vec![
                        DbValue::Text("Cookies.md".to_string()),
                        DbValue::Text("Cookies".to_string()),
                        DbValue::Text("Cookie storage details.".to_string()),
                        DbValue::Real(0.2),
                    ],
                }]);
            }
            if sql.contains("c.embedding") {
                return Ok(vec![DbRow {
                    values: vec![
                        DbValue::Text("Cookies.md".to_string()),
                        DbValue::Text("Cookies".to_string()),
                        DbValue::Text("Cookie storage details.".to_string()),
                        DbValue::Blob(serialize_embedding(&[0.0, 2.0, 0.0])),
                    ],
                }]);
            }
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct MockReranker;

    #[async_trait]
    impl RerankModel for MockReranker {
        async fn rerank(
            &self,
            query: &str,
            candidates: &[String],
        ) -> Result<Vec<f32>, KnowledgeError> {
            let wants_browser_state = query.to_ascii_lowercase().contains("browser state");
            Ok(candidates
                .iter()
                .map(|candidate| {
                    let normalized = candidate.to_ascii_lowercase();
                    if wants_browser_state && normalized.contains("cookie storage") {
                        1.0
                    } else if normalized.contains("oauth") {
                        0.2
                    } else {
                        0.0
                    }
                })
                .collect())
        }
    }

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn test_provider_unindexed() -> ObsidianKnowledgeProvider {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let vault_root =
            std::env::temp_dir().join(format!("klaw-knowledge-provider-vault-{suffix}"));
        std::fs::create_dir_all(&vault_root).expect("vault");
        std::fs::write(
            vault_root.join("auth.md"),
            "---\ntags: [auth]\ndate: 2026-04-24\n---\n# Auth\nOAuth and cookie auth details.\nSee [[Cookies]]",
        )
        .expect("auth note");
        std::fs::write(
            vault_root.join("Cookies.md"),
            "---\ntags: [auth, session]\n---\n# Cookies\nCookie storage details.",
        )
        .expect("cookies note");

        let db_root = std::env::temp_dir().join(format!("klaw-knowledge-provider-db-{suffix}"));
        let db = Arc::new(
            DefaultKnowledgeDb::open_knowledge(StoragePaths::from_root(db_root))
                .await
                .expect("db"),
        );
        ObsidianKnowledgeProvider::open(
            db,
            vault_root,
            vec![".obsidian".to_string()],
            400,
            "Test Vault",
        )
        .await
        .expect("provider should open")
    }

    async fn test_provider() -> ObsidianKnowledgeProvider {
        let provider = test_provider_unindexed().await;
        provider.reindex().await.expect("test vault should index");
        provider
    }

    async fn test_provider_with_models(
        embedder: Option<Arc<dyn EmbeddingModel>>,
        reranker: Option<Arc<dyn RerankModel>>,
    ) -> ObsidianKnowledgeProvider {
        let provider = test_provider_unindexed().await;
        let provider = if let Some(embedder) = embedder {
            provider.with_embedding_model(embedder)
        } else {
            provider
        };
        let provider = if let Some(reranker) = reranker {
            provider.with_reranker(reranker)
        } else {
            provider
        };
        provider.reindex().await.expect("reindex with models");
        provider
    }

    #[tokio::test]
    async fn search_returns_ranked_hits_from_indexed_vault() {
        let provider = test_provider().await;
        let hits = provider
            .search(KnowledgeSearchQuery {
                text: "cookie auth".to_string(),
                ..Default::default()
            })
            .await
            .expect("search should succeed");
        assert!(!hits.is_empty());
        assert!(hits.iter().any(|hit| hit.title == "Auth"));
    }

    #[tokio::test]
    async fn graph_lane_uses_discovered_plain_text_links() {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let vault_root =
            std::env::temp_dir().join(format!("klaw-knowledge-provider-links-vault-{suffix}"));
        std::fs::create_dir_all(&vault_root).expect("vault");
        std::fs::write(
            vault_root.join("auth.md"),
            "# Auth\nCookies keep browser state.",
        )
        .expect("auth note");
        std::fs::write(
            vault_root.join("Cookies.md"),
            "# Cookies\nCookie storage details.",
        )
        .expect("cookies note");

        let db_root =
            std::env::temp_dir().join(format!("klaw-knowledge-provider-links-db-{suffix}"));
        let db = Arc::new(
            DefaultKnowledgeDb::open_knowledge(StoragePaths::from_root(db_root))
                .await
                .expect("db"),
        );
        let provider =
            ObsidianKnowledgeProvider::open(db, vault_root, Vec::new(), 400, "Test Vault")
                .await
                .expect("provider should open");
        provider.reindex().await.expect("test vault should index");

        let hits = provider
            .graph_lane(
                &[RankedHit {
                    id: "auth.md".to_string(),
                    title: "Auth".to_string(),
                    excerpt: String::new(),
                    score: 1.0,
                }],
                5,
            )
            .await
            .expect("graph lane should load");

        assert!(hits.iter().any(|hit| hit.id == "Cookies.md"));
    }

    #[test]
    fn temporal_pattern_ignores_non_ascii_queries() {
        assert_eq!(temporal_pattern("激活码"), None);
    }

    #[tokio::test]
    async fn get_returns_full_entry_by_path_id() {
        let provider = test_provider().await;
        let entry = provider.get("auth.md").await.expect("get should succeed");
        assert!(entry.expect("entry").content.contains("OAuth"));
    }

    #[tokio::test]
    async fn search_uses_orchestrator_expansions() {
        let provider = test_provider().await;
        let orchestrator = RecordingOrchestrator::default();
        let calls = Arc::clone(&orchestrator.calls);
        let provider = provider.with_orchestrator(Arc::new(orchestrator));

        let hits = provider
            .search(KnowledgeSearchQuery {
                text: "how auth works".to_string(),
                ..Default::default()
            })
            .await
            .expect("search should succeed");

        assert!(!hits.is_empty());
        assert_eq!(
            calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_slice(),
            ["how auth works"]
        );
        assert!(
            hits.iter()
                .any(|hit| hit.excerpt.to_ascii_lowercase().contains("cookie"))
        );
    }

    #[tokio::test]
    async fn search_uses_semantic_lane_when_embeddings_exist() {
        let provider = test_provider_with_models(Some(Arc::new(MockEmbeddingModel)), None).await;
        let hits = provider
            .search(KnowledgeSearchQuery {
                text: "browser state".to_string(),
                ..Default::default()
            })
            .await
            .expect("search should succeed");

        assert!(!hits.is_empty());
        assert_eq!(hits[0].title, "Cookies");
        assert!(
            hits[0]
                .metadata
                .get("lanes")
                .and_then(Value::as_array)
                .is_some_and(|lanes| lanes.iter().any(|lane| lane == "semantic"))
        );
    }

    #[tokio::test]
    async fn semantic_lane_uses_vector_top_k_when_vector_index_is_enabled() {
        let db = Arc::new(RecordingDb::default());
        let sql = Arc::clone(&db.sql);
        let provider = ObsidianKnowledgeProvider {
            db,
            vault_root: PathBuf::new(),
            exclude_folders: Vec::new(),
            max_excerpt_length: 400,
            source_name: "Test Vault".to_string(),
            fts_virtual: false,
            vector_index_enabled: Arc::new(Mutex::new(true)),
            embedder: Some(Arc::new(MockEmbeddingModel)),
            reranker: None,
            orchestrator: None,
        };

        let hits = provider
            .semantic_lane("browser state", 5)
            .await
            .expect("semantic lane should query vector index");

        assert_eq!(hits[0].title, "Cookies");
        let sql = sql.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(sql.iter().any(|query| query.contains("vector_top_k")));
        assert!(!sql.iter().any(|query| query.contains("c.embedding")));
    }

    #[tokio::test]
    async fn semantic_lane_uses_sql_vector_distance_before_rust_fallback() {
        let db = Arc::new(RecordingDb {
            fail_vector_index: true,
            ..RecordingDb::default()
        });
        let sql = Arc::clone(&db.sql);
        let provider = ObsidianKnowledgeProvider {
            db,
            vault_root: PathBuf::new(),
            exclude_folders: Vec::new(),
            max_excerpt_length: 400,
            source_name: "Test Vault".to_string(),
            fts_virtual: false,
            vector_index_enabled: Arc::new(Mutex::new(false)),
            embedder: Some(Arc::new(MockEmbeddingModel)),
            reranker: None,
            orchestrator: None,
        };

        let hits = provider
            .semantic_lane("browser state", 5)
            .await
            .expect("semantic lane should use SQL vector distance");

        assert_eq!(hits[0].title, "Cookies");
        let sql = sql.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(
            sql.iter()
                .any(|query| query.contains("vector_distance_cos"))
        );
        assert!(
            !sql.iter()
                .any(|query| query.contains("SELECT e.id, e.title, c.snippet, c.embedding"))
        );
    }

    #[tokio::test]
    async fn semantic_lane_uses_rust_cosine_only_when_vector_sql_is_unavailable() {
        let db = Arc::new(RecordingDb {
            fail_vector_index: true,
            fail_vector_distance: true,
            ..RecordingDb::default()
        });
        let sql = Arc::clone(&db.sql);
        let provider = ObsidianKnowledgeProvider {
            db,
            vault_root: PathBuf::new(),
            exclude_folders: Vec::new(),
            max_excerpt_length: 400,
            source_name: "Test Vault".to_string(),
            fts_virtual: false,
            vector_index_enabled: Arc::new(Mutex::new(false)),
            embedder: Some(Arc::new(MockEmbeddingModel)),
            reranker: None,
            orchestrator: None,
        };

        let hits = provider
            .semantic_lane("browser state", 5)
            .await
            .expect("semantic lane should fall back to Rust cosine");

        assert_eq!(hits[0].title, "Cookies");
        let sql = sql.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(
            sql.iter()
                .any(|query| query.contains("vector_distance_cos"))
        );
        assert!(
            sql.iter()
                .any(|query| query.contains("SELECT e.id, e.title, c.snippet, c.embedding"))
        );
    }

    #[tokio::test]
    async fn entry_metadata_loading_filters_to_fused_hit_ids() {
        let db = Arc::new(RecordingDb::default());
        let sql = Arc::clone(&db.sql);
        let provider = ObsidianKnowledgeProvider {
            db,
            vault_root: PathBuf::new(),
            exclude_folders: Vec::new(),
            max_excerpt_length: 400,
            source_name: "Test Vault".to_string(),
            fts_virtual: false,
            vector_index_enabled: Arc::new(Mutex::new(false)),
            embedder: None,
            reranker: None,
            orchestrator: None,
        };

        let _ = provider
            .load_entry_metadata(&["Cookies.md".to_string()])
            .await
            .expect("metadata should load");

        let sql = sql.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let metadata_query = sql
            .iter()
            .find(|query| query.contains("tags_json"))
            .expect("metadata query should run");
        assert!(metadata_query.contains("WHERE id IN"));
    }

    #[tokio::test]
    async fn rerank_lane_marks_final_hits() {
        let provider = test_provider_with_models(
            Some(Arc::new(MockEmbeddingModel)),
            Some(Arc::new(MockReranker)),
        )
        .await;
        let hits = provider
            .search(KnowledgeSearchQuery {
                text: "browser state auth".to_string(),
                limit: 2,
                ..Default::default()
            })
            .await
            .expect("search should succeed");

        assert!(hits.iter().any(|hit| {
            hit.metadata
                .get("lanes")
                .and_then(Value::as_array)
                .is_some_and(|lanes| lanes.iter().any(|lane| lane == "rerank"))
        }));
    }
}
