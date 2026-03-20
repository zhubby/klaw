use crate::ChannelResponse;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelegramParseMode {
    Html,
}

impl TelegramParseMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Html => "HTML",
        }
    }
}

pub fn render_telegram_response(output: &ChannelResponse, show_reasoning: bool) -> String {
    let mut body = markdownish_to_telegram_html(output.content.trim());
    if show_reasoning {
        if let Some(reasoning) = output.reasoning.as_deref().map(str::trim) {
            if !reasoning.is_empty() {
                if !body.is_empty() {
                    body.push_str("\n\n");
                }
                body.push_str("<b>Reasoning</b>\n<pre>");
                body.push_str(&escape_html(reasoning));
                body.push_str("</pre>");
            }
        }
    }
    body
}

pub fn extract_approval_id(output: &ChannelResponse) -> Option<String> {
    if let Some(approval_id) = output
        .metadata
        .get("approval.id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(approval_id.to_string());
    }
    if let Some(approval_id) = output
        .metadata
        .get("approval.signal")
        .and_then(Value::as_object)
        .and_then(|value| value.get("approval_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(approval_id.to_string());
    }
    extract_shell_approval_id(&output.content)
}

pub fn extract_approval_command_preview(output: &ChannelResponse) -> Option<String> {
    output
        .metadata
        .get("approval.signal")
        .and_then(Value::as_object)
        .and_then(|value| value.get("command_preview"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub fn build_approval_message(output: &ChannelResponse, approval_id: &str) -> String {
    let mut parts = vec!["<b>Approval Required</b>".to_string()];
    if let Some(command_preview) = extract_approval_command_preview(output) {
        parts.push(format!(
            "<b>Command</b>\n<pre>{}</pre>",
            escape_html(&command_preview)
        ));
    }
    let content = render_telegram_response(output, false);
    if !content.trim().is_empty() {
        parts.push(content);
    }
    parts.push(format!(
        "<b>Approval ID</b>\n<code>{}</code>",
        escape_html(approval_id)
    ));
    parts.join("\n\n")
}

fn markdownish_to_telegram_html(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    let mut first_segment = true;
    let mut remainder = input;

    while let Some(start) = remainder.find("```") {
        let before = &remainder[..start];
        if !before.is_empty() {
            if !first_segment && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&render_inline_html(before));
            first_segment = false;
        }

        let after_ticks = &remainder[start + 3..];
        let (language, after_header) = match after_ticks.find('\n') {
            Some(newline) => (&after_ticks[..newline], &after_ticks[newline + 1..]),
            None => ("", ""),
        };

        if let Some(end) = after_header.find("```") {
            let code = &after_header[..end];
            if !out.is_empty() && !out.ends_with("\n\n") {
                out.push_str("\n\n");
            }
            if !language.trim().is_empty() {
                out.push_str(&format!("<b>{}</b>\n", escape_html(language.trim())));
            }
            out.push_str("<pre>");
            out.push_str(&escape_html(code.trim_end_matches('\n')));
            out.push_str("</pre>");
            remainder = &after_header[end + 3..];
        } else {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&render_inline_html(&remainder[start..]));
            return out;
        }
    }

    if !remainder.is_empty() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&render_inline_html(remainder));
    }

    out
}

fn render_inline_html(input: &str) -> String {
    let mut out = String::new();
    let mut i = 0;

    while i < input.len() {
        let rest = &input[i..];
        let rest_bytes = rest.as_bytes();

        if rest_bytes.starts_with(b"**") {
            if let Some(end) = find_subslice(&rest_bytes[2..], b"**") {
                let content = &rest[2..2 + end];
                out.push_str("<b>");
                out.push_str(&escape_html(content));
                out.push_str("</b>");
                i += 2 + end + 2;
                continue;
            }
        }

        if rest_bytes.first() == Some(&b'`') {
            if let Some(end) = rest_bytes[1..].iter().position(|ch| *ch == b'`') {
                let content = &rest[1..1 + end];
                out.push_str("<code>");
                out.push_str(&escape_html(content));
                out.push_str("</code>");
                i += end + 2;
                continue;
            }
        }

        let mut chars = rest.chars();
        let ch = chars
            .next()
            .expect("rest should be non-empty while iterating");
        out.push_str(&escape_html(ch.encode_utf8(&mut [0; 4])));
        i += ch.len_utf8();
    }

    out
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn extract_shell_approval_id(content: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<Value>(content) {
        if let Some(token) = value
            .pointer("/approval/id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            return Some(token.to_string());
        }
        if let Some(token) = value
            .get("approval_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            return Some(token.to_string());
        }
        if let Some(token) = value
            .get("approvalId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            return Some(token.to_string());
        }
    }

    let marker = "approval_id=";
    if let Some(idx) = content.find(marker) {
        let rest = &content[idx + marker.len()..];
        let token = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
            .collect::<String>();
        if !token.is_empty() {
            return Some(token);
        }
    }

    None
}

pub fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn render_telegram_response_renders_bold_code_and_reasoning() {
        let output = ChannelResponse {
            content: "**Title**\n\n```text\n/help\n```".to_string(),
            reasoning: Some("line 1\nline 2".to_string()),
            metadata: BTreeMap::new(),
        };

        let rendered = render_telegram_response(&output, true);
        assert!(rendered.contains("<b>Title</b>"));
        assert!(rendered.contains("<pre>/help</pre>"));
        assert!(rendered.contains("<b>Reasoning</b>"));
    }

    #[test]
    fn approval_helpers_prefer_structured_metadata() {
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "approval.id".to_string(),
            Value::String("approval-1".to_string()),
        );
        metadata.insert(
            "approval.signal".to_string(),
            serde_json::json!({"command_preview": "python3 -c 'print(1)'"}),
        );
        let output = ChannelResponse {
            content: "approval_id=from-content".to_string(),
            reasoning: None,
            metadata,
        };

        assert_eq!(extract_approval_id(&output).as_deref(), Some("approval-1"));
        let body = build_approval_message(&output, "approval-1");
        assert!(body.contains("Approval Required"));
        assert!(body.contains("python3 -c"));
    }

    #[test]
    fn render_telegram_response_handles_leading_emoji_without_panicking() {
        let output = ChannelResponse {
            content: "📘 **Command Center**".to_string(),
            reasoning: None,
            metadata: BTreeMap::new(),
        };

        let rendered = render_telegram_response(&output, false);
        assert!(rendered.contains("📘"));
        assert!(rendered.contains("<b>Command Center</b>"));
    }
}
