use crate::{MemoryError, MemoryRecord};
use klaw_storage::{DbRow, DbValue, StorageError};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const RRF_K: f64 = 60.0;

pub(crate) fn row_to_record(row: &DbRow) -> Result<MemoryRecord, MemoryError> {
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

pub(crate) fn db_string(value: Option<&DbValue>, field: &str) -> Result<String, MemoryError> {
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

pub(crate) fn db_i64(value: Option<&DbValue>, field: &str) -> Result<i64, MemoryError> {
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

pub(crate) fn rrf_score(bm25_rank: Option<usize>, vector_rank: Option<usize>) -> f64 {
    let mut score = 0.0;
    if let Some(rank) = bm25_rank {
        score += 1.0 / (RRF_K + rank as f64);
    }
    if let Some(rank) = vector_rank {
        score += 1.0 / (RRF_K + rank as f64);
    }
    score
}

pub(crate) fn f32_vec_to_blob(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<u8>>()
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
