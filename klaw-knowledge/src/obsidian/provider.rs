use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use klaw_storage::{DbRow, DbValue, MemoryDb};
use serde_json::{Value, json};

use crate::{
    KnowledgeEntry, KnowledgeError, KnowledgeHit, KnowledgeProvider, KnowledgeSearchQuery,
    KnowledgeSourceInfo,
    obsidian::indexer::{index_vault, init_schema},
    retrieval::fusion::{RankedHit, reciprocal_rank_fuse},
};

#[derive(Clone)]
pub struct ObsidianKnowledgeProvider {
    db: Arc<dyn MemoryDb>,
    vault_root: PathBuf,
    exclude_folders: Vec<String>,
    max_excerpt_length: usize,
    source_name: String,
    fts_virtual: bool,
}

impl ObsidianKnowledgeProvider {
    pub async fn open(
        db: Arc<dyn MemoryDb>,
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
        };
        if index_on_startup {
            provider.reindex().await?;
        }
        Ok(provider)
    }

    pub async fn reindex(&self) -> Result<usize, KnowledgeError> {
        index_vault(
            self.db.clone(),
            &self.vault_root,
            &self.exclude_folders,
            self.max_excerpt_length,
        )
        .await
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
                    &[DbValue::Text(query.to_string()), DbValue::Integer(limit as i64)],
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
                let matches = tokens.iter().filter(|token| haystack.contains(token.as_str())).count();
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

    async fn entry_count(&self) -> Result<usize, KnowledgeError> {
        let rows = self
            .db
            .query("SELECT COUNT(*) FROM knowledge_entries", &[])
            .await
            .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
        Ok(integer_at(&rows[0], 0).unwrap_or(0) as usize)
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
        let fts = self.fts_lane(&query.text, limit * 3).await?;
        let graph = self.graph_lane(&fts, limit).await?;
        let temporal = self.temporal_lane(&query.text, limit).await?;
        let fused = reciprocal_rank_fuse(&[("fts", &fts), ("graph", &graph), ("temporal", &temporal)], 60);

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
                    (
                        text_at(&row, 1)?,
                        text_at(&row, 2)?,
                        text_at(&row, 3)?,
                    ),
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
    let bytes = query.as_bytes();
    for start in 0..bytes.len() {
        for len in [10usize, 7usize] {
            if start + len <= bytes.len() {
                let candidate = &query[start..start + len];
                if is_date_like(candidate) {
                    return Some(format!("{candidate}%"));
                }
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

async fn detect_virtual_fts(db: &Arc<dyn MemoryDb>) -> Result<bool, KnowledgeError> {
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use klaw_storage::{DefaultKnowledgeDb, StoragePaths};

    use super::*;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn test_provider() -> ObsidianKnowledgeProvider {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let vault_root = std::env::temp_dir().join(format!("klaw-knowledge-provider-vault-{suffix}"));
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
            true,
            "Test Vault",
        )
        .await
        .expect("provider should open")
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
    async fn get_returns_full_entry_by_path_id() {
        let provider = test_provider().await;
        let entry = provider.get("auth.md").await.expect("get should succeed");
        assert!(entry.expect("entry").content.contains("OAuth"));
    }
}
