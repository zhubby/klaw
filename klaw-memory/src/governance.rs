use crate::{MemoryError, MemoryRecord, UpsertMemoryInput};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LongTermMemoryKind {
    Identity,
    Preference,
    ProjectRule,
    Workflow,
    Fact,
    Constraint,
}

impl LongTermMemoryKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Preference => "preference",
            Self::ProjectRule => "project_rule",
            Self::Workflow => "workflow",
            Self::Fact => "fact",
            Self::Constraint => "constraint",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "identity" => Some(Self::Identity),
            "preference" => Some(Self::Preference),
            "project_rule" => Some(Self::ProjectRule),
            "workflow" => Some(Self::Workflow),
            "fact" => Some(Self::Fact),
            "constraint" => Some(Self::Constraint),
            _ => None,
        }
    }

    #[must_use]
    pub fn priority(self) -> u8 {
        match self {
            Self::Identity => 5,
            Self::ProjectRule => 4,
            Self::Constraint => 3,
            Self::Preference => 2,
            Self::Workflow => 1,
            Self::Fact => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LongTermMemoryPriority {
    High,
    Medium,
    Low,
}

impl LongTermMemoryPriority {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "high" => Some(Self::High),
            "medium" => Some(Self::Medium),
            "low" => Some(Self::Low),
            _ => None,
        }
    }

    #[must_use]
    pub fn rank(self) -> u8 {
        match self {
            Self::High => 2,
            Self::Medium => 1,
            Self::Low => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LongTermMemoryStatus {
    Active,
    Superseded,
    Archived,
    Rejected,
}

impl LongTermMemoryStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Superseded => "superseded",
            Self::Archived => "archived",
            Self::Rejected => "rejected",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "active" => Some(Self::Active),
            "superseded" => Some(Self::Superseded),
            "archived" => Some(Self::Archived),
            "rejected" => Some(Self::Rejected),
            _ => None,
        }
    }
}

#[must_use]
pub fn default_priority_for_kind(kind: LongTermMemoryKind) -> LongTermMemoryPriority {
    match kind {
        LongTermMemoryKind::Identity | LongTermMemoryKind::ProjectRule => {
            LongTermMemoryPriority::High
        }
        LongTermMemoryKind::Preference
        | LongTermMemoryKind::Workflow
        | LongTermMemoryKind::Constraint => LongTermMemoryPriority::Medium,
        LongTermMemoryKind::Fact => LongTermMemoryPriority::Low,
    }
}

#[derive(Debug, Clone)]
pub struct GovernedLongTermWrite {
    pub primary: UpsertMemoryInput,
    pub superseded_updates: Vec<UpsertMemoryInput>,
    pub reused_existing_id: Option<String>,
    pub supersedes_ids: Vec<String>,
    pub kind: LongTermMemoryKind,
}

fn is_active_long_term_with_kind(record: &MemoryRecord, kind: LongTermMemoryKind) -> bool {
    record.scope == "long_term"
        && !is_summary_record(record)
        && read_status(record).unwrap_or(LongTermMemoryStatus::Active)
            == LongTermMemoryStatus::Active
        && read_kind(record).unwrap_or(LongTermMemoryKind::Fact) == kind
}

pub fn govern_long_term_write(
    existing_records: &[MemoryRecord],
    draft: UpsertMemoryInput,
) -> Result<GovernedLongTermWrite, MemoryError> {
    if draft.scope != "long_term" {
        return Err(MemoryError::InvalidQuery(
            "long-term governance only supports scope `long_term`".to_string(),
        ));
    }

    let normalized_content = normalize_content(&draft.content);
    if normalized_content.is_empty() {
        return Err(MemoryError::InvalidQuery(
            "content cannot be empty".to_string(),
        ));
    }

    let Some(metadata) = draft.metadata.as_object() else {
        return Err(MemoryError::InvalidQuery(
            "metadata must be a JSON object".to_string(),
        ));
    };
    let mut normalized_metadata = metadata.clone();
    let kind = normalize_kind(&normalized_metadata)?;
    normalize_incoming_status(&normalized_metadata)?;
    let priority = normalize_priority(&normalized_metadata, kind)?;
    let topic = normalize_optional_string_field(&mut normalized_metadata, "topic");
    let mut supersedes = normalize_supersedes_field(&mut normalized_metadata)?;
    normalized_metadata.remove("superseded_by");

    let duplicate = existing_records.iter().find(|record| {
        is_active_long_term_with_kind(record, kind)
            && normalize_content(&record.content) == normalized_content
    });

    let primary_id = duplicate
        .map(|record| record.id.clone())
        .or(draft.id.clone())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let conflicts = if let Some(topic) = topic.as_deref() {
        existing_records
            .iter()
            .filter(|record| {
                is_active_long_term_with_kind(record, kind)
                    && record.id != primary_id
                    && read_topic(record).as_deref() == Some(topic)
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    supersedes.extend(conflicts.iter().map(|record| record.id.clone()));
    let supersedes = dedupe_strings(supersedes);

    normalized_metadata.insert("kind".to_string(), json!(kind.as_str()));
    normalized_metadata.insert("priority".to_string(), json!(priority.as_str()));
    normalized_metadata.insert(
        "status".to_string(),
        json!(LongTermMemoryStatus::Active.as_str()),
    );
    if let Some(topic) = topic {
        normalized_metadata.insert("topic".to_string(), json!(topic));
    }
    if !supersedes.is_empty() {
        normalized_metadata.insert("supersedes".to_string(), json!(supersedes.clone()));
    } else {
        normalized_metadata.remove("supersedes");
    }

    let merged_metadata = if let Some(existing) = duplicate {
        merge_existing_metadata(existing, &normalized_metadata, &supersedes)
    } else {
        Value::Object(normalized_metadata)
    };

    let primary = UpsertMemoryInput {
        id: Some(primary_id.clone()),
        scope: "long_term".to_string(),
        content: normalized_content,
        metadata: merged_metadata,
        pinned: draft.pinned || duplicate.is_some_and(|record| record.pinned),
    };

    let superseded_updates = conflicts
        .into_iter()
        .map(|record| UpsertMemoryInput {
            id: Some(record.id.clone()),
            scope: record.scope.clone(),
            content: record.content.clone(),
            metadata: superseded_metadata(record, &primary_id),
            pinned: record.pinned,
        })
        .collect();

    Ok(GovernedLongTermWrite {
        primary,
        superseded_updates,
        reused_existing_id: duplicate.map(|record| record.id.clone()),
        supersedes_ids: supersedes,
        kind,
    })
}

pub fn read_kind(record: &MemoryRecord) -> Option<LongTermMemoryKind> {
    record
        .metadata
        .get("kind")
        .and_then(Value::as_str)
        .and_then(LongTermMemoryKind::parse)
}

pub fn read_status(record: &MemoryRecord) -> Option<LongTermMemoryStatus> {
    record
        .metadata
        .get("status")
        .and_then(Value::as_str)
        .and_then(LongTermMemoryStatus::parse)
}

pub fn read_topic(record: &MemoryRecord) -> Option<String> {
    record
        .metadata
        .get("topic")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub fn read_priority(record: &MemoryRecord) -> Option<LongTermMemoryPriority> {
    record
        .metadata
        .get("priority")
        .and_then(Value::as_str)
        .and_then(LongTermMemoryPriority::parse)
}

pub fn read_archived_at(record: &MemoryRecord) -> Option<i64> {
    record.metadata.get("archived_at").and_then(Value::as_i64)
}

#[must_use]
pub fn is_summary_record(record: &MemoryRecord) -> bool {
    record
        .metadata
        .get("summary")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[must_use]
pub fn effective_priority(record: &MemoryRecord) -> LongTermMemoryPriority {
    read_priority(record).unwrap_or_else(|| {
        default_priority_for_kind(read_kind(record).unwrap_or(LongTermMemoryKind::Fact))
    })
}

#[must_use]
pub fn is_inactive_long_term_record(record: &MemoryRecord) -> bool {
    record.scope == "long_term"
        && matches!(
            read_status(record),
            Some(
                LongTermMemoryStatus::Archived
                    | LongTermMemoryStatus::Rejected
                    | LongTermMemoryStatus::Superseded
            )
        )
}

pub fn normalize_content(content: &str) -> String {
    content.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_kind(metadata: &Map<String, Value>) -> Result<LongTermMemoryKind, MemoryError> {
    match metadata.get("kind").and_then(Value::as_str) {
        Some(raw) => LongTermMemoryKind::parse(raw).ok_or_else(|| {
            MemoryError::InvalidQuery(format!(
                "invalid memory metadata.kind `{raw}`; expected one of identity/preference/project_rule/workflow/fact/constraint"
            ))
        }),
        None => Ok(LongTermMemoryKind::Fact),
    }
}

fn normalize_incoming_status(metadata: &Map<String, Value>) -> Result<(), MemoryError> {
    let Some(raw_status) = metadata.get("status").and_then(Value::as_str) else {
        return Ok(());
    };
    let Some(status) = LongTermMemoryStatus::parse(raw_status) else {
        return Err(MemoryError::InvalidQuery(format!(
            "invalid memory metadata.status `{raw_status}`"
        )));
    };
    if status != LongTermMemoryStatus::Active {
        return Err(MemoryError::InvalidQuery(
            "new long-term memories can only be created with status `active`; other statuses are system-managed"
                .to_string(),
        ));
    }
    Ok(())
}

fn normalize_priority(
    metadata: &Map<String, Value>,
    kind: LongTermMemoryKind,
) -> Result<LongTermMemoryPriority, MemoryError> {
    match metadata.get("priority").and_then(Value::as_str) {
        Some(raw) => LongTermMemoryPriority::parse(raw).ok_or_else(|| {
            MemoryError::InvalidQuery(format!(
                "invalid memory metadata.priority `{raw}`; expected one of high/medium/low"
            ))
        }),
        None => Ok(default_priority_for_kind(kind)),
    }
}

fn normalize_supersedes_field(
    metadata: &mut Map<String, Value>,
) -> Result<Vec<String>, MemoryError> {
    let Some(raw) = metadata.get("supersedes") else {
        return Ok(Vec::new());
    };
    let values = match raw {
        Value::String(value) => vec![value.clone()],
        Value::Array(values) => values
            .iter()
            .map(|value| {
                value.as_str().map(ToString::to_string).ok_or_else(|| {
                    MemoryError::InvalidQuery(
                        "metadata.supersedes must be a string or string array".to_string(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(MemoryError::InvalidQuery(
                "metadata.supersedes must be a string or string array".to_string(),
            ));
        }
    };
    Ok(dedupe_strings(values))
}

fn normalize_optional_string_field(
    metadata: &mut Map<String, Value>,
    field: &str,
) -> Option<String> {
    let value = metadata
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    if value.is_none() {
        metadata.remove(field);
    }
    value
}

fn merge_existing_metadata(
    existing: &MemoryRecord,
    normalized_metadata: &Map<String, Value>,
    supersedes: &[String],
) -> Value {
    let mut merged = existing.metadata.as_object().cloned().unwrap_or_default();
    for (key, value) in normalized_metadata {
        merged.insert(key.clone(), value.clone());
    }
    if !supersedes.is_empty() {
        merged.insert("supersedes".to_string(), json!(supersedes));
    }
    Value::Object(merged)
}

fn superseded_metadata(record: &MemoryRecord, superseded_by: &str) -> Value {
    let mut metadata = record.metadata.as_object().cloned().unwrap_or_default();
    metadata.insert(
        "status".to_string(),
        json!(LongTermMemoryStatus::Superseded.as_str()),
    );
    metadata.insert("superseded_by".to_string(), json!(superseded_by));
    Value::Object(metadata)
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(id: &str, content: &str, metadata: Value, pinned: bool) -> MemoryRecord {
        MemoryRecord {
            id: id.to_string(),
            scope: "long_term".to_string(),
            content: content.to_string(),
            metadata,
            pinned,
            created_at_ms: 1,
            updated_at_ms: 1,
        }
    }

    #[test]
    fn govern_long_term_write_defaults_kind_and_active_status() {
        let plan = govern_long_term_write(
            &[],
            UpsertMemoryInput {
                id: None,
                scope: "long_term".to_string(),
                content: "  remember   this  ".to_string(),
                metadata: json!({}),
                pinned: false,
            },
        )
        .expect("governance should succeed");

        assert_eq!(plan.kind, LongTermMemoryKind::Fact);
        assert_eq!(plan.primary.content, "remember this");
        assert_eq!(plan.primary.metadata["kind"], "fact");
        assert_eq!(plan.primary.metadata["status"], "active");
    }

    #[test]
    fn govern_long_term_write_reuses_exact_active_duplicate() {
        let existing = vec![record(
            "m1",
            "Default language is Chinese.",
            json!({"kind": "preference", "status": "active"}),
            false,
        )];
        let plan = govern_long_term_write(
            &existing,
            UpsertMemoryInput {
                id: None,
                scope: "long_term".to_string(),
                content: "Default language is Chinese.".to_string(),
                metadata: json!({"kind": "preference"}),
                pinned: true,
            },
        )
        .expect("governance should succeed");

        assert_eq!(plan.reused_existing_id.as_deref(), Some("m1"));
        assert_eq!(plan.primary.id.as_deref(), Some("m1"));
        assert!(plan.primary.pinned);
        assert!(plan.superseded_updates.is_empty());
    }

    #[test]
    fn govern_long_term_write_supersedes_same_kind_and_topic_conflicts() {
        let existing = vec![
            record(
                "old-1",
                "Default language is English.",
                json!({"kind": "preference", "topic": "reply_language", "status": "active"}),
                false,
            ),
            record(
                "old-2",
                "Use concise replies.",
                json!({"kind": "preference", "topic": "verbosity", "status": "active"}),
                false,
            ),
        ];
        let plan = govern_long_term_write(
            &existing,
            UpsertMemoryInput {
                id: Some("new-1".to_string()),
                scope: "long_term".to_string(),
                content: "Default language is Chinese.".to_string(),
                metadata: json!({"kind": "preference", "topic": "reply_language"}),
                pinned: false,
            },
        )
        .expect("governance should succeed");

        assert_eq!(plan.primary.metadata["supersedes"], json!(["old-1"]));
        assert_eq!(plan.superseded_updates.len(), 1);
        assert_eq!(plan.superseded_updates[0].id.as_deref(), Some("old-1"));
        assert_eq!(plan.superseded_updates[0].metadata["status"], "superseded");
        assert_eq!(
            plan.superseded_updates[0].metadata["superseded_by"],
            "new-1"
        );
    }

    #[test]
    fn govern_long_term_write_rejects_system_managed_status() {
        let err = govern_long_term_write(
            &[],
            UpsertMemoryInput {
                id: None,
                scope: "long_term".to_string(),
                content: "Outdated fact".to_string(),
                metadata: json!({"status": "superseded"}),
                pinned: false,
            },
        )
        .expect_err("non-active status should be rejected");

        assert!(err.to_string().contains("system-managed"));
    }

    #[test]
    fn govern_long_term_write_rejects_invalid_priority() {
        let err = govern_long_term_write(
            &[],
            UpsertMemoryInput {
                id: None,
                scope: "long_term".to_string(),
                content: "Keep my replies concise.".to_string(),
                metadata: json!({"priority": "urgent"}),
                pinned: false,
            },
        )
        .expect_err("invalid priority should be rejected");

        assert!(err.to_string().contains("invalid memory metadata.priority"));
    }

    #[test]
    fn govern_long_term_write_ignores_archive_summary_records_when_detecting_conflicts() {
        let existing = vec![record(
            "summary-1",
            "Archived summary for reply_language.",
            json!({
                "kind": "preference",
                "topic": "reply_language",
                "status": "active",
                "summary": true,
                "source_ids": ["old-1"],
            }),
            false,
        )];

        let plan = govern_long_term_write(
            &existing,
            UpsertMemoryInput {
                id: Some("new-1".to_string()),
                scope: "long_term".to_string(),
                content: "Default language is Chinese.".to_string(),
                metadata: json!({"kind": "preference", "topic": "reply_language"}),
                pinned: false,
            },
        )
        .expect("governance should ignore summaries");

        assert!(plan.superseded_updates.is_empty());
        assert_eq!(plan.primary.metadata["topic"], "reply_language");
        assert_eq!(plan.primary.metadata["status"], "active");
    }
}
