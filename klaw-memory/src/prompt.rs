use crate::{
    LongTermMemoryKind, LongTermMemoryStatus, MemoryRecord, normalize_long_term_content,
    read_long_term_kind, read_long_term_status,
};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LongTermMemoryPromptOptions {
    pub max_items: usize,
    pub max_chars: usize,
    pub max_item_chars: usize,
}

impl Default for LongTermMemoryPromptOptions {
    fn default() -> Self {
        Self {
            max_items: 12,
            max_chars: 1800,
            max_item_chars: 240,
        }
    }
}

#[must_use]
pub fn render_long_term_memory_section(
    records: &[MemoryRecord],
    options: &LongTermMemoryPromptOptions,
) -> Option<String> {
    let mut ordered_records = records.to_vec();
    ordered_records.sort_by(|a, b| {
        let kind_pri = |record: &MemoryRecord| {
            read_long_term_kind(record)
                .unwrap_or(LongTermMemoryKind::Fact)
                .priority()
        };
        b.pinned
            .cmp(&a.pinned)
            .then_with(|| kind_pri(b).cmp(&kind_pri(a)))
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
            .then_with(|| a.id.cmp(&b.id))
    });
    let max_items = options.max_items.max(1);
    let max_chars = options.max_chars.max(1);
    let max_item_chars = options.max_item_chars.max(1);
    let mut seen = BTreeSet::new();
    let mut lines = Vec::new();
    let mut used_chars = 0usize;

    for record in &ordered_records {
        if is_inactive(record) {
            continue;
        }
        let content = normalize_long_term_content(&record.content);
        if content.is_empty() {
            continue;
        }
        let dedupe_key = content.to_ascii_lowercase();
        if !seen.insert(dedupe_key) {
            continue;
        }

        let content = truncate_chars(&content, max_item_chars);
        let line = match kind_label(&record.metadata) {
            Some(kind) => format!("- [{kind}] {content}"),
            None => format!("- {content}"),
        };
        let projected_chars = if lines.is_empty() {
            line.chars().count()
        } else {
            used_chars + 1 + line.chars().count()
        };
        if !lines.is_empty() && projected_chars > max_chars {
            break;
        }
        if lines.len() >= max_items {
            break;
        }

        used_chars = projected_chars.min(max_chars);
        lines.push(line);
    }

    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn is_inactive(record: &MemoryRecord) -> bool {
    matches!(
        read_long_term_status(record),
        Some(
            LongTermMemoryStatus::Archived
                | LongTermMemoryStatus::Rejected
                | LongTermMemoryStatus::Superseded
        )
    )
}

fn kind_label(metadata: &Value) -> Option<String> {
    metadata
        .get("kind")
        .and_then(Value::as_str)
        .and_then(LongTermMemoryKind::parse)
        .map(|kind| kind.as_str().to_string())
}

fn truncate_chars(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let head: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{head}...")
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn record(
        id: &str,
        content: &str,
        metadata: Value,
        pinned: bool,
        updated_at_ms: i64,
    ) -> MemoryRecord {
        MemoryRecord {
            id: id.to_string(),
            scope: "long_term".to_string(),
            content: content.to_string(),
            metadata,
            pinned,
            created_at_ms: updated_at_ms,
            updated_at_ms,
        }
    }

    #[test]
    fn render_long_term_memory_section_formats_and_dedupes() {
        let rendered = render_long_term_memory_section(
            &[
                record(
                    "1",
                    "  Default   language   is  Chinese. ",
                    json!({"kind": "preference"}),
                    true,
                    10,
                ),
                record(
                    "2",
                    "default language is chinese.",
                    json!({"kind": "preference"}),
                    false,
                    9,
                ),
                record(
                    "3",
                    "Do not mutate stale config snapshots before save.",
                    json!({"kind": "project_rule"}),
                    false,
                    8,
                ),
            ],
            &LongTermMemoryPromptOptions::default(),
        )
        .expect("section should render");

        assert!(rendered.contains("- [preference] Default language is Chinese."));
        assert!(
            rendered.contains("- [project_rule] Do not mutate stale config snapshots before save.")
        );
        assert_eq!(rendered.matches("[preference]").count(), 1);
    }

    #[test]
    fn render_long_term_memory_section_skips_inactive_and_honors_budget() {
        let rendered = render_long_term_memory_section(
            &[
                record(
                    "1",
                    "Pinned memory that should remain visible.",
                    json!({"kind": "fact"}),
                    true,
                    10,
                ),
                record(
                    "2",
                    "Outdated memory should not render.",
                    json!({"status": "superseded"}),
                    false,
                    9,
                ),
                record(
                    "3",
                    "A very long memory item that will be truncated for prompt safety.",
                    json!({"kind": "constraint"}),
                    false,
                    8,
                ),
            ],
            &LongTermMemoryPromptOptions {
                max_items: 2,
                max_chars: 120,
                max_item_chars: 40,
            },
        )
        .expect("section should render");

        assert!(rendered.contains("Pinned memory that should remain visible."));
        assert!(!rendered.contains("Outdated memory"));
        assert!(rendered.contains("A very long memory item"));
        assert!(rendered.contains("..."));
    }
}
