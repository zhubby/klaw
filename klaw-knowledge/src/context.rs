use serde::{Deserialize, Serialize};

use crate::KnowledgeHit;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextBundle {
    pub topic: String,
    pub sections: Vec<ContextSection>,
    pub total_chars: usize,
    pub budget_chars: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextSection {
    pub label: String,
    pub title: String,
    pub uri: String,
    pub content: String,
    pub relevance: String,
}

const SECTION_OVERHEAD: usize = 80;

pub fn assemble_context_bundle(
    topic: &str,
    hits: &[KnowledgeHit],
    budget_chars: usize,
) -> ContextBundle {
    let budget_chars = budget_chars.max(1);
    let mut sections = Vec::new();
    let mut total_chars = 0;
    let mut truncated = false;

    for hit in hits {
        if total_chars >= budget_chars {
            truncated = true;
            break;
        }
        let available = budget_chars.saturating_sub(total_chars + SECTION_OVERHEAD);
        if available == 0 {
            truncated = true;
            break;
        }

        let content = if hit.excerpt.chars().count() > available {
            truncated = true;
            truncate_chars(&hit.excerpt, available.saturating_sub(14)) + "... [truncated]"
        } else {
            hit.excerpt.clone()
        };
        total_chars += content.len() + SECTION_OVERHEAD;
        sections.push(ContextSection {
            label: "Direct match".to_string(),
            title: hit.title.clone(),
            uri: hit.uri.clone(),
            content,
            relevance: format!("score {:.2}", hit.score),
        });
    }

    ContextBundle {
        topic: topic.to_string(),
        sections,
        total_chars,
        budget_chars,
        truncated,
    }
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn sample_hit(title: &str, excerpt: &str) -> KnowledgeHit {
        KnowledgeHit {
            id: title.to_lowercase(),
            title: title.to_string(),
            excerpt: excerpt.to_string(),
            score: 0.9,
            tags: vec!["rust".to_string()],
            uri: format!("vault/{title}.md"),
            source: "obsidian".to_string(),
            metadata: json!({}),
        }
    }

    #[test]
    fn assembles_context_bundle_within_budget() {
        let hits = vec![sample_hit("Auth", "OAuth and cookie notes")];
        let bundle = assemble_context_bundle("auth", &hits, 200);
        assert_eq!(bundle.sections.len(), 1);
        assert!(!bundle.truncated);
        assert!(bundle.sections[0].content.contains("OAuth"));
    }

    #[test]
    fn truncates_context_when_budget_is_small() {
        let hits = vec![sample_hit("Auth", &"word ".repeat(200))];
        let bundle = assemble_context_bundle("auth", &hits, 120);
        assert_eq!(bundle.sections.len(), 1);
        assert!(bundle.truncated);
        assert!(bundle.sections[0].content.contains("[truncated]"));
    }
}
