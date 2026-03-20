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
        "你是对话上下文压缩器，请输出稳定、可机读的 JSON 摘要。\n\
\n\
目标：\n\
1) 保留用户目标（goal）\n\
2) 保留已完成步骤（progress）\n\
3) 保留未完成任务（pending）\n\
4) 保留关键决策（decisions）\n\
5) 删除闲聊、寒暄、重复表达\n\
\n\
字段语义约束：\n\
- goal: 单个字符串，描述当前阶段最核心目标；未知时用空字符串。\n\
- progress: 已完成事项，按时间顺序简短陈述（每项一句）。\n\
- pending: 尚未完成且仍有效的任务；已完成项必须从 pending 移除并转入 progress。\n\
- decisions: 已确认的技术/产品决策；若被新信息推翻，保留最新决策并删除旧决策。\n\
- notes: 关键上下文（约束、依赖、风险、约定），不写闲聊。\n\
\n\
质量要求：\n\
- 合并重复内容，删除过时信息。\n\
- 优先保留事实和可执行信息，避免模糊措辞。\n\
- 不得输出 markdown、解释文字或代码块。\n\
- 只能输出一个完整 JSON 对象。\n\
\n\
输出 JSON 结构（键名必须完全一致）：\n\
{{\n\
  \"goal\": \"\",\n\
  \"progress\": [],\n\
  \"pending\": [],\n\
  \"decisions\": [],\n\
  \"notes\": []\n\
}}\n\
\n\
已有摘要：\n\
{old_summary_json}\n\
\n\
新增对话：\n\
{rendered_messages}\n\
\n\
请基于“已有摘要 + 新增对话”输出新的完整 JSON。"
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
