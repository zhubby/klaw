use crate::{MemoryError, util::now_ms};
use klaw_storage::{DbValue, MemoryDb, open_default_memory_db};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct ScopeStat {
    pub scope: String,
    pub count: i64,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    pub total_records: i64,
    pub pinned_records: i64,
    pub embedded_records: i64,
    pub distinct_scopes: i64,
    pub created_min_ms: Option<i64>,
    pub created_max_ms: Option<i64>,
    pub updated_max_ms: Option<i64>,
    pub avg_content_len: Option<f64>,
    pub updated_last_24h: i64,
    pub updated_last_7d: i64,
    pub fts_enabled: bool,
    pub vector_index_enabled: bool,
    pub top_scopes: Vec<ScopeStat>,
}

pub struct SqliteMemoryStatsService {
    db: Arc<dyn MemoryDb>,
}

impl SqliteMemoryStatsService {
    pub async fn open_default() -> Result<Self, MemoryError> {
        let db = open_default_memory_db().await?;
        Ok(Self { db: Arc::new(db) })
    }

    pub fn new(db: Arc<dyn MemoryDb>) -> Self {
        Self { db }
    }

    pub async fn collect(&self, scope_limit: i64) -> Result<MemoryStats, MemoryError> {
        let summary_rows = self
            .db
            .query(
                "SELECT COUNT(*),\
                        COALESCE(SUM(CASE WHEN pinned = 1 THEN 1 ELSE 0 END), 0),\
                        COALESCE(SUM(CASE WHEN embedding IS NOT NULL THEN 1 ELSE 0 END), 0),\
                        COUNT(DISTINCT scope),\
                        MIN(created_at_ms),\
                        MAX(created_at_ms),\
                        MAX(updated_at_ms),\
                        AVG(LENGTH(content))\
                 FROM memories",
                &[],
            )
            .await?;

        let Some(summary) = summary_rows.first() else {
            return Ok(MemoryStats::default());
        };

        let total_records = value_to_i64(summary.get(0), "count")?;
        let pinned_records = value_to_i64(summary.get(1), "pinned")?;
        let embedded_records = value_to_i64(summary.get(2), "embedded")?;
        let distinct_scopes = value_to_i64(summary.get(3), "distinct_scopes")?;
        let created_min_ms = value_to_opt_i64(summary.get(4), "created_min_ms")?;
        let created_max_ms = value_to_opt_i64(summary.get(5), "created_max_ms")?;
        let updated_max_ms = value_to_opt_i64(summary.get(6), "updated_max_ms")?;
        let avg_content_len = value_to_opt_f64(summary.get(7), "avg_content_len")?;

        let now = now_ms();
        let day_ms = 24_i64 * 60 * 60 * 1000;
        let week_ms = 7_i64 * day_ms;

        let updated_last_24h = self.count_updated_since(now.saturating_sub(day_ms)).await?;
        let updated_last_7d = self
            .count_updated_since(now.saturating_sub(week_ms))
            .await?;

        let fts_enabled = self.sqlite_master_count("table", "memories_fts").await? > 0;
        let vector_index_enabled = self
            .sqlite_master_count("index", "idx_memories_embedding")
            .await?
            > 0;

        let top_scopes = self.list_top_scopes(scope_limit.max(1)).await?;

        Ok(MemoryStats {
            total_records,
            pinned_records,
            embedded_records,
            distinct_scopes,
            created_min_ms,
            created_max_ms,
            updated_max_ms,
            avg_content_len,
            updated_last_24h,
            updated_last_7d,
            fts_enabled,
            vector_index_enabled,
            top_scopes,
        })
    }

    async fn count_updated_since(&self, from_ms: i64) -> Result<i64, MemoryError> {
        let rows = self
            .db
            .query(
                "SELECT COUNT(*) FROM memories WHERE updated_at_ms >= ?1",
                &[DbValue::Integer(from_ms)],
            )
            .await?;

        let Some(row) = rows.first() else {
            return Ok(0);
        };
        value_to_i64(row.get(0), "count_updated_since")
    }

    async fn sqlite_master_count(&self, kind: &str, name: &str) -> Result<i64, MemoryError> {
        let rows = self
            .db
            .query(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = ?1 AND name = ?2",
                &[
                    DbValue::Text(kind.to_string()),
                    DbValue::Text(name.to_string()),
                ],
            )
            .await?;

        let Some(row) = rows.first() else {
            return Ok(0);
        };
        value_to_i64(row.get(0), "sqlite_master_count")
    }

    async fn list_top_scopes(&self, limit: i64) -> Result<Vec<ScopeStat>, MemoryError> {
        let rows = self
            .db
            .query(
                "SELECT scope, COUNT(*) FROM memories GROUP BY scope ORDER BY COUNT(*) DESC, scope ASC LIMIT ?1",
                &[DbValue::Integer(limit)],
            )
            .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(ScopeStat {
                scope: value_to_string(row.get(0), "scope")?,
                count: value_to_i64(row.get(1), "scope_count")?,
            });
        }
        Ok(out)
    }
}

fn value_to_string(value: Option<&DbValue>, field: &str) -> Result<String, MemoryError> {
    match value {
        Some(DbValue::Text(v)) => Ok(v.clone()),
        Some(DbValue::Integer(v)) => Ok(v.to_string()),
        Some(DbValue::Real(v)) => Ok(v.to_string()),
        Some(DbValue::Null) | None => Err(MemoryError::Storage(
            klaw_storage::StorageError::backend(format!("missing text field: {field}")),
        )),
        Some(DbValue::Blob(_)) => Err(MemoryError::Storage(klaw_storage::StorageError::backend(
            format!("invalid blob text field: {field}"),
        ))),
    }
}

fn value_to_i64(value: Option<&DbValue>, field: &str) -> Result<i64, MemoryError> {
    match value {
        Some(DbValue::Integer(v)) => Ok(*v),
        Some(DbValue::Real(v)) => Ok(*v as i64),
        Some(DbValue::Text(v)) => v.parse::<i64>().map_err(|err| {
            MemoryError::Storage(klaw_storage::StorageError::backend(format!(
                "invalid integer for {field}: {err}"
            )))
        }),
        Some(DbValue::Null) | None => Err(MemoryError::Storage(
            klaw_storage::StorageError::backend(format!("missing integer field: {field}")),
        )),
        Some(DbValue::Blob(_)) => Err(MemoryError::Storage(klaw_storage::StorageError::backend(
            format!("invalid blob integer field: {field}"),
        ))),
    }
}

fn value_to_opt_i64(value: Option<&DbValue>, field: &str) -> Result<Option<i64>, MemoryError> {
    match value {
        Some(DbValue::Null) | None => Ok(None),
        Some(v) => value_to_i64(Some(v), field).map(Some),
    }
}

fn value_to_opt_f64(value: Option<&DbValue>, field: &str) -> Result<Option<f64>, MemoryError> {
    match value {
        Some(DbValue::Null) | None => Ok(None),
        Some(DbValue::Real(v)) => Ok(Some(*v)),
        Some(DbValue::Integer(v)) => Ok(Some(*v as f64)),
        Some(DbValue::Text(v)) => v.parse::<f64>().map(Some).map_err(|err| {
            MemoryError::Storage(klaw_storage::StorageError::backend(format!(
                "invalid float for {field}: {err}"
            )))
        }),
        Some(DbValue::Blob(_)) => Err(MemoryError::Storage(klaw_storage::StorageError::backend(
            format!("invalid blob float field: {field}"),
        ))),
    }
}
