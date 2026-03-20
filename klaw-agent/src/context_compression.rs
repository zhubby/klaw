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
        "压缩时 使用结构化数据 要求：\n\
1. 保留用户目标\n\
2. 保留已完成步骤\n\
3. 保留未完成任务\n\
4. 保留关键决策\n\
5. 删除闲聊内容\n\
\n\
输出JSON：\n\
\n\
{{\n\
  \"goal\": \"\",\n\
  \"progress\": [],\n\
  \"pending\": [],\n\
  \"decisions\": [],\n\
  \"notes\": []\n\
}}\n\
\n\
prompt:\n\
\n\
已有摘要：\n\
{old_summary_json}\n\
\n\
新增对话：\n\
{rendered_messages}\n\
\n\
请更新摘要：\n\
- 保留已有重要信息\n\
- 合并重复内容\n\
- 删除过时信息\n\
- 更新任务状态\n\
\n\
输出新的完整JSON。除 JSON 外不要输出任何其他内容。"
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

        assert!(prompt.contains("已有摘要"));
        assert!(prompt.contains("新增对话"));
        assert!(prompt.contains("ship feature"));
        assert!(prompt.contains("user: continue work"));
    }

    #[test]
    fn parse_summary_supports_plain_json_and_fenced_json() {
        let plain = r#"{"goal":"g","progress":["a"],"pending":[],"decisions":[],"notes":[]}"#;
        let fenced = format!("```json\n{plain}\n```");

        let parsed_plain = parse_conversation_summary(plain).expect("plain json should parse");
        let parsed_fenced =
            parse_conversation_summary(&fenced).expect("fenced json should parse");

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
