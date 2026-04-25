use std::{cmp::Ordering, collections::BTreeMap, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use klaw_model::QueryIntent;
use klaw_storage::{DatabaseExecutor, DbRow, DbValue};
use serde_json::{Value, json};

use crate::{
    KnowledgeEntry, KnowledgeError, KnowledgeHit, KnowledgeProvider, KnowledgeSearchQuery,
    KnowledgeSourceInfo, KnowledgeStatus, KnowledgeSyncProgress,
    models::{EmbeddingModel, KnowledgeOrchestration, OrchestratorModel, RerankModel},
    obsidian::indexer::{
        embed_missing_chunks, embed_missing_chunks_with_progress, index_vault,
        index_vault_with_progress, init_schema,
    },
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
        index_on_startup: bool,
        source_name: impl Into<String>,
    ) -> Result<Self, KnowledgeError> {
        init_schema(&db).await?;
        let fts_virtual = detect_virtual_fts(&db).await?;
        let provider = Self {
            db,
            vault_root,
            exclude_folders,
            max_excerpt_length,
            source_name: source_name.into(),
            fts_virtual,
            embedder: None,
            reranker: None,
            orchestrator: None,
        };
        if index_on_startup {
            provider.reindex().await?;
        }
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
        index_vault(
            self.db.clone(),
            &self.vault_root,
            &self.exclude_folders,
            self.max_excerpt_length,
            self.embedder.as_deref(),
        )
        .await
    }

    pub async fn reindex_with_progress<F>(&self, progress: F) -> Result<usize, KnowledgeError>
    where
        F: FnMut(KnowledgeSyncProgress),
    {
        index_vault_with_progress(
            self.db.clone(),
            &self.vault_root,
            &self.exclude_folders,
            self.max_excerpt_length,
            self.embedder.as_deref(),
            progress,
        )
        .await
    }

    pub async fn embed_missing_chunks(&self) -> Result<usize, KnowledgeError> {
        let Some(embedder) = self.embedder.as_deref() else {
            return Ok(0);
        };
        embed_missing_chunks(self.db.clone(), embedder).await
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
        embed_missing_chunks_with_progress(self.db.clone(), embedder, progress).await
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
                    "SELECT target.id, target.title, substr(target.content, 1, 400)
                     FROM knowledge_links link
                     JOIN knowledge_entries target ON lower(target.title) = lower(link.target_title)
                     WHERE link.source_entry_id = ?1
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

        let entry_map = self
            .db
            .query(
                "SELECT id, uri, tags_json, metadata_json FROM knowledge_entries",
                &[],
            )
            .await
            .map_err(|err| KnowledgeError::Provider(err.to_string()))?
            .into_iter()
            .filter_map(|row| {
                Some((
                    text_at(&row, 0)?,
                    (text_at(&row, 1)?, text_at(&row, 2)?, text_at(&row, 3)?),
                ))
            })
            .collect::<BTreeMap<_, _>>();

        Ok(fused
            .into_iter()
            .take(limit)
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

    use klaw_storage::{DefaultKnowledgeDb, StoragePaths};

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

    async fn test_provider_with_startup_index(index_on_startup: bool) -> ObsidianKnowledgeProvider {
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
            index_on_startup,
            "Test Vault",
        )
        .await
        .expect("provider should open")
    }

    async fn test_provider() -> ObsidianKnowledgeProvider {
        test_provider_with_startup_index(true).await
    }

    async fn test_provider_with_models(
        embedder: Option<Arc<dyn EmbeddingModel>>,
        reranker: Option<Arc<dyn RerankModel>>,
    ) -> ObsidianKnowledgeProvider {
        let provider = test_provider_with_startup_index(false).await;
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
