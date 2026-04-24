use crate::{
    LongTermMemoryKind, LongTermMemoryPriority, MemoryError, MemoryRecord, MemoryService,
    SqliteMemoryService, SqliteMemoryStatsService, UpsertMemoryInput, effective_long_term_priority,
    is_inactive_long_term_record, is_summary_record, normalize_long_term_content,
    read_long_term_kind, read_long_term_topic,
};
use async_trait::async_trait;
use klaw_storage::MemoryDb;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use tracing::warn;
use uuid::Uuid;
use futures_util::future::join_all;

use crate::util::now_ms;

/// Strategy for generating summary content when archiving stale long-term memories.
///
/// The default [`TemplateSummaryGenerator`] concatenates source snippets into a
/// structured template string. Consumers that want richer, LLM-generated summaries
/// should provide their own implementation (e.g. one that calls an LLM provider).
#[async_trait]
pub trait SummaryGenerator: Send + Sync {
    /// Generate a summary string for a group of archived records.
    ///
    /// - `group` identifies the kind + topic of the records being summarized.
    /// - `source_records` are the original records that will be archived.
    /// - `max_sources` limits how many source entries to include.
    async fn generate_summary(
        &self,
        group: &ArchiveGroupKey,
        source_records: &[MemoryRecord],
        max_sources: usize,
    ) -> Result<String, MemoryError>;
}

/// Fallback summary generator that uses the original template-based concatenation.
///
/// This does not call any LLM; it produces a deterministic string like:
///
/// > Past notes on preference (3 entries): Default language is Chinese; Use concise replies; +2 more
pub struct TemplateSummaryGenerator;

#[async_trait]
impl SummaryGenerator for TemplateSummaryGenerator {
    async fn generate_summary(
        &self,
        group: &ArchiveGroupKey,
        source_records: &[MemoryRecord],
        max_sources: usize,
    ) -> Result<String, MemoryError> {
        Ok(build_summary_content(group, source_records, max_sources))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LongTermArchiveConfig {
    pub max_age_days: i64,
    pub summary_max_sources: usize,
}

impl Default for LongTermArchiveConfig {
    fn default() -> Self {
        Self {
            max_age_days: 30,
            summary_max_sources: 8,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LongTermArchiveOutcome {
    pub archived_records: usize,
    pub summary_records_upserted: usize,
    pub skipped_records: usize,
}

/// Key used to group records for archival summary generation.
///
/// Records sharing the same `kind` and `topic` are grouped together
/// and summarized as a single roll-up entry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ArchiveGroupKey {
    pub kind: LongTermMemoryKind,
    pub topic: Option<String>,
}

pub async fn archive_stale_long_term_memories(
    db: Arc<dyn MemoryDb>,
    config: LongTermArchiveConfig,
    summary_generator: Arc<dyn SummaryGenerator>,
) -> Result<LongTermArchiveOutcome, MemoryError> {
    if config.max_age_days <= 0 {
        return Err(MemoryError::InvalidQuery(
            "archive max_age_days must be greater than 0".to_string(),
        ));
    }
    if config.summary_max_sources == 0 {
        return Err(MemoryError::InvalidQuery(
            "archive summary_max_sources must be greater than 0".to_string(),
        ));
    }

    let stats = SqliteMemoryStatsService::new(db.clone());
    let service = Arc::new(SqliteMemoryService::new(db, None).await?);
    let records = stats.list_scope_records("long_term").await?;
    let now = now_ms();
    let cutoff_ms = now.saturating_sub(config.max_age_days.saturating_mul(24 * 60 * 60 * 1000));

    let mut summaries_by_group = BTreeMap::new();
    let mut source_records_by_id = BTreeMap::new();
    let mut candidate_groups: BTreeMap<ArchiveGroupKey, Vec<MemoryRecord>> = BTreeMap::new();
    let mut outcome = LongTermArchiveOutcome::default();

    for record in &records {
        source_records_by_id.insert(record.id.clone(), record.clone());
        if is_summary_record(record)
            && !is_inactive_long_term_record(record)
            && record.scope == "long_term"
        {
            summaries_by_group.insert(group_key(record), record.clone());
        }
    }

    for record in &records {
        if should_archive_record(record, cutoff_ms) {
            candidate_groups
                .entry(group_key(record))
                .or_default()
                .push(record.clone());
        } else if record.scope == "long_term"
            && !is_summary_record(record)
            && !is_inactive_long_term_record(record)
            && effective_long_term_priority(record) == LongTermMemoryPriority::Low
        {
            outcome.skipped_records += 1;
        }
    }

    // Process each group in parallel: each group independently generates a
    // summary and writes to DB, so there are no data dependencies between groups.
    let group_futures = candidate_groups.into_iter().map(|(group, mut candidates)| {
        let service = service.clone();
        let summary_generator = summary_generator.clone();
        let max_sources = config.summary_max_sources;
        let existing_summary = summaries_by_group.get(&group).cloned();
        let source_records_by_id = &source_records_by_id;

        candidates.sort_by(|a, b| {
            a.updated_at_ms
                .cmp(&b.updated_at_ms)
                .then_with(|| a.created_at_ms.cmp(&b.created_at_ms))
                .then_with(|| a.id.cmp(&b.id))
        });

        let summary_id = existing_summary
            .as_ref()
            .map(|record| record.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        let source_ids = merged_source_ids(existing_summary.as_ref(), &candidates);
        let source_records = source_ids
            .iter()
            .filter_map(|id| source_records_by_id.get(id))
            .cloned()
            .collect::<Vec<_>>();

        let summary_metadata =
            build_summary_metadata(&group, &source_ids, now, existing_summary.as_ref());

        async move {
            let summary_content = summary_generator
                .generate_summary(&group, &source_records, max_sources)
                .await
                .unwrap_or_else(|err| {
                    warn!(error = %err, "summary generator failed, falling back to template");
                    build_summary_content(&group, &source_records, max_sources)
                });

            service
                .upsert(UpsertMemoryInput {
                    id: Some(summary_id.clone()),
                    scope: "long_term".to_string(),
                    content: summary_content,
                    metadata: Value::Object(summary_metadata),
                    pinned: false,
                })
                .await?;

            let mut archived_count = 0;
            for record in candidates {
                let metadata = archive_metadata(&record, now, &summary_id);
                service
                    .upsert(UpsertMemoryInput {
                        id: Some(record.id.clone()),
                        scope: record.scope.clone(),
                        content: record.content.clone(),
                        metadata: Value::Object(metadata),
                        pinned: record.pinned,
                    })
                    .await?;
                archived_count += 1;
            }

            Ok::<(usize, usize), MemoryError>((1, archived_count))
        }
    });

    // Collect results from all parallel group tasks.
    let group_results = join_all(group_futures).await;
    for result in group_results {
        let (summary_count, archived_count) = result?;
        outcome.summary_records_upserted += summary_count;
        outcome.archived_records += archived_count;
    }

    Ok(outcome)
}

fn should_archive_record(record: &MemoryRecord, cutoff_ms: i64) -> bool {
    record.scope == "long_term"
        && !record.pinned
        && !is_summary_record(record)
        && !is_inactive_long_term_record(record)
        && effective_long_term_priority(record) == LongTermMemoryPriority::Low
        && record
            .metadata
            .get("archived_at")
            .and_then(Value::as_i64)
            .is_none()
        && record.updated_at_ms <= cutoff_ms
}

fn group_key(record: &MemoryRecord) -> ArchiveGroupKey {
    ArchiveGroupKey {
        kind: read_long_term_kind(record).unwrap_or(LongTermMemoryKind::Fact),
        topic: read_long_term_topic(record),
    }
}

fn merged_source_ids(
    existing_summary: Option<&MemoryRecord>,
    candidates: &[MemoryRecord],
) -> Vec<String> {
    let mut seen = BTreeSet::new();
    existing_summary
        .into_iter()
        .flat_map(read_source_ids)
        .chain(candidates.iter().map(|record| record.id.clone()))
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

fn read_source_ids(record: &MemoryRecord) -> Vec<String> {
    match record.metadata.get("source_ids") {
        Some(Value::String(value)) => vec![value.clone()],
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(ToString::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn build_summary_metadata(
    group: &ArchiveGroupKey,
    source_ids: &[String],
    archived_at_ms: i64,
    existing_summary: Option<&MemoryRecord>,
) -> Map<String, Value> {
    let mut metadata = existing_summary
        .and_then(|record| record.metadata.as_object().cloned())
        .unwrap_or_default();
    metadata.insert("kind".to_string(), json!(group.kind.as_str()));
    metadata.insert(
        "priority".to_string(),
        json!(LongTermMemoryPriority::Low.as_str()),
    );
    metadata.insert("status".to_string(), json!("active"));
    metadata.insert("summary".to_string(), json!(true));
    metadata.insert("source_ids".to_string(), json!(source_ids));
    metadata.insert("archived_at".to_string(), json!(archived_at_ms));
    metadata.insert("summary_type".to_string(), json!("archive_rollup"));
    if let Some(topic) = group.topic.as_ref() {
        metadata.insert("topic".to_string(), json!(topic));
    } else {
        metadata.remove("topic");
    }
    metadata
}

fn build_summary_content(
    group: &ArchiveGroupKey,
    source_records: &[MemoryRecord],
    max_sources: usize,
) -> String {
    let label = group
        .topic
        .as_deref()
        .map(ToString::to_string)
        .unwrap_or_else(|| group.kind.as_str().to_string());
    let mut snippets = source_records
        .iter()
        .map(|record| normalize_long_term_content(&record.content))
        .filter(|content| !content.is_empty())
        .collect::<Vec<_>>();
    snippets.sort();
    snippets.dedup();

    let total = snippets.len();
    let preview = snippets.into_iter().take(max_sources).collect::<Vec<_>>();
    let more = total.saturating_sub(preview.len());
    let mut content = format!(
        "Past notes on {label} ({total} entries): {}",
        preview.join("; ")
    );
    if more > 0 {
        content.push_str(&format!("; +{more} more"));
    }
    content
}

fn archive_metadata(
    record: &MemoryRecord,
    archived_at_ms: i64,
    summary_id: &str,
) -> Map<String, Value> {
    let mut metadata = record.metadata.as_object().cloned().unwrap_or_default();
    metadata.insert("status".to_string(), json!("archived"));
    metadata.insert("archived_at".to_string(), json!(archived_at_ms));
    metadata.insert("archived_by_summary".to_string(), json!(summary_id));
    metadata
}