use crate::ConversationMessage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ConversationSummary {
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub progress: Vec<String>,
    #[serde(default)]
    pub pending: Vec<String>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

pub fn build_compression_prompt(
    old_summary: Option<&ConversationSummary>,
    new_messages: &[ConversationMessage],
) -> String {
    let rendered_messages = render_messages(new_messages);
    let old_summary_json = old_summary
        .and_then(|summary| serde_json::to_string_pretty(summary).ok())
        .unwrap_or_else(|| "{}".to_string());

    format!(
        r#"You are a conversation context compressor. Output a stable, machine-readable JSON summary.

Objectives:
1) Preserve user goal (goal)
2) Preserve completed steps (progress)
3) Preserve pending tasks (pending)
4) Preserve key decisions (decisions)
5) Remove chitchat, pleasantries, and repetitive expressions

Field semantic constraints:
- goal: Single string describing the current stage's core goal; use empty string if unknown.
- progress: Completed items, briefly stated in chronological order (one sentence each).
- pending: Unfinished and still-valid tasks; completed items must be moved from pending to progress.
- decisions: Confirmed technical/product decisions; if overturned by new information, keep only the latest decision and remove old ones.
- notes: Key context (constraints, dependencies, risks, conventions), no chitchat.

Quality requirements:
- Merge duplicate content, delete outdated information.
- Prioritize factual and actionable information, avoid vague wording.
- Do not output markdown, explanatory text, or code blocks.
- Output only one complete JSON object.

Output JSON structure (key names must match exactly):
{{
  "goal": "",
  "progress": [],
  "pending": [],
  "decisions": [],
  "notes": []
}}

Existing summary:
{old_summary_json}

New messages:
{rendered_messages}

Based on "existing summary + new messages", output the new complete JSON."#
    )
}

pub fn merge_or_reset_summary(
    old_summary: Option<&ConversationSummary>,
    model_output: &str,
) -> ConversationSummary {
    parse_conversation_summary(model_output)
        .or_else(|| old_summary.cloned())
        .unwrap_or_default()
}

pub fn parse_conversation_summary(model_output: &str) -> Option<ConversationSummary> {
    let direct = model_output.trim();
    if let Ok(summary) = serde_json::from_str::<ConversationSummary>(direct) {
        return Some(summary);
    }

    if let Some(fenced) = extract_fenced_json(direct) {
        if let Ok(summary) = serde_json::from_str::<ConversationSummary>(&fenced) {
            return Some(summary);
        }
    }

    extract_first_json_object(direct)
        .and_then(|json_text| serde_json::from_str::<ConversationSummary>(&json_text).ok())
}

fn render_messages(messages: &[ConversationMessage]) -> String {
    if messages.is_empty() {
        return "(empty)".to_string();
    }
    messages
        .iter()
        .map(|msg| format!("{}: {}", msg.role, msg.content))
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_fenced_json(input: &str) -> Option<String> {
    let stripped = input.strip_prefix("```")?;
    let mut parts = stripped.splitn(2, '\n');
    let _lang = parts.next()?;
    let rest = parts.next()?;
    let end = rest.rfind("```")?;
    Some(rest[..end].trim().to_string())
}

fn extract_first_json_object(input: &str) -> Option<String> {
    let start = input.find('{')?;
    let end = input.rfind('}')?;
    if end <= start {
        return None;
    }
    Some(input[start..=end].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_includes_old_summary_and_new_messages() {
        let old = ConversationSummary {
            goal: "ship feature".to_string(),
            progress: vec!["done a".to_string()],
            pending: vec![],
            decisions: vec![],
            notes: vec![],
        };
        let prompt = build_compression_prompt(
            Some(&old),
            &[ConversationMessage {
                role: "user".to_string(),
                content: "continue work".to_string(),
            }],
        );

        assert!(prompt.contains("Existing summary"));
        assert!(prompt.contains("New messages"));
        assert!(prompt.contains("ship feature"));
        assert!(prompt.contains("user: continue work"));
    }

    #[test]
    fn parse_summary_supports_plain_json_and_fenced_json() {
        let plain = r#"{"goal":"g","progress":["a"],"pending":[],"decisions":[],"notes":[]}"#;
        let fenced = format!("```json\n{plain}\n```");

        let parsed_plain = parse_conversation_summary(plain).expect("plain json should parse");
        let parsed_fenced = parse_conversation_summary(&fenced).expect("fenced json should parse");

        assert_eq!(parsed_plain.goal, "g");
        assert_eq!(parsed_fenced.goal, "g");
    }

    #[test]
    fn merge_or_reset_summary_falls_back_to_old_summary_on_parse_failure() {
        let old = ConversationSummary {
            goal: "keep old".to_string(),
            progress: vec!["p".to_string()],
            pending: vec!["x".to_string()],
            decisions: vec!["d".to_string()],
            notes: vec!["n".to_string()],
        };
        let merged = merge_or_reset_summary(Some(&old), "not-json");

        assert_eq!(merged, old);
    }
}
