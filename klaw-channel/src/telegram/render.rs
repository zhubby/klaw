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

    let lines = input.lines().collect::<Vec<_>>();
    let mut blocks = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            index += 1;
            continue;
        }

        if let Some((language, code, next_index)) = parse_fenced_code_block(&lines, index) {
            blocks.push(render_code_block(language, &code));
            index = next_index;
            continue;
        }

        if is_horizontal_rule(trimmed) {
            index += 1;
            continue;
        }

        if is_blockquote_line(trimmed) {
            let (quote, next_index) = render_blockquote(&lines, index);
            blocks.push(quote);
            index = next_index;
            continue;
        }

        if let Some(heading) = render_heading(trimmed) {
            blocks.push(heading);
            index += 1;
            continue;
        }

        if is_list_item(line) {
            let (list, next_index) = render_list(&lines, index);
            blocks.push(list);
            index = next_index;
            continue;
        }

        let (paragraph, next_index) = render_paragraph(&lines, index);
        blocks.push(paragraph);
        index = next_index;
    }

    blocks.join("\n\n")
}

fn render_inline_html(input: &str) -> String {
    let mut out = String::new();
    let mut i = 0;

    while i < input.len() {
        let rest = &input[i..];

        if let Some((rendered, consumed)) = try_render_link(rest) {
            out.push_str(&rendered);
            i += consumed;
            continue;
        }

        if let Some((rendered, consumed)) = try_render_code_span(rest) {
            out.push_str(&rendered);
            i += consumed;
            continue;
        }

        if let Some((rendered, consumed)) = try_render_delimited_span(rest, "**", "b") {
            out.push_str(&rendered);
            i += consumed;
            continue;
        }

        if let Some((rendered, consumed)) = try_render_delimited_span(rest, "__", "u") {
            out.push_str(&rendered);
            i += consumed;
            continue;
        }

        if let Some((rendered, consumed)) = try_render_delimited_span(rest, "~~", "s") {
            out.push_str(&rendered);
            i += consumed;
            continue;
        }

        if let Some((rendered, consumed)) = try_render_delimited_span(rest, "||", "tg-spoiler") {
            out.push_str(&rendered);
            i += consumed;
            continue;
        }

        if let Some((rendered, consumed)) = try_render_emphasis(rest, '*') {
            out.push_str(&rendered);
            i += consumed;
            continue;
        }

        if let Some((rendered, consumed)) = try_render_emphasis(rest, '_') {
            out.push_str(&rendered);
            i += consumed;
            continue;
        }

        if let Some(stripped) = rest.strip_prefix('\\') {
            if let Some(ch) = stripped.chars().next() {
                out.push_str(&escape_html(ch.encode_utf8(&mut [0; 4])));
                i += 1 + ch.len_utf8();
                continue;
            }
        }

        if let Some(ch) = rest.chars().next() {
            out.push_str(&escape_html(ch.encode_utf8(&mut [0; 4])));
            i += ch.len_utf8();
            continue;
        }

        break;
    }

    out
}

fn parse_fenced_code_block(
    lines: &[&str],
    start: usize,
) -> Option<(Option<String>, String, usize)> {
    let opening = lines.get(start)?.trim_start();
    let language = opening.strip_prefix("```")?;
    let language = language.trim();
    let mut code = Vec::new();
    let mut index = start + 1;

    while index < lines.len() {
        if lines[index].trim() == "```" {
            return Some((
                (!language.is_empty()).then(|| language.to_string()),
                code.join("\n"),
                index + 1,
            ));
        }
        code.push(lines[index]);
        index += 1;
    }

    None
}

fn render_code_block(language: Option<String>, code: &str) -> String {
    let escaped_code = escape_html(code.trim_end_matches('\n'));
    match language {
        Some(language) => format!(
            "<pre><code class=\"language-{}\">{}</code></pre>",
            escape_html_attribute(&language),
            escaped_code
        ),
        None => format!("<pre>{}</pre>", escaped_code),
    }
}

fn render_blockquote(lines: &[&str], start: usize) -> (String, usize) {
    let mut rendered_lines = Vec::new();
    let mut index = start;

    while index < lines.len() {
        let trimmed = lines[index].trim();
        if trimmed.is_empty() || !is_blockquote_line(trimmed) {
            break;
        }

        let content = trimmed.trim_start_matches('>').trim_start().trim_end();
        rendered_lines.push(render_inline_html(content));
        index += 1;
    }

    (
        format!("<blockquote>{}</blockquote>", rendered_lines.join("\n")),
        index,
    )
}

fn render_heading(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let level = bytes.iter().take_while(|ch| **ch == b'#').count();
    if level == 0 || level > 6 || bytes.get(level) != Some(&b' ') {
        return None;
    }

    let content = line[level + 1..].trim();
    if content.is_empty() {
        return None;
    }

    Some(format!("<b>{}</b>", render_inline_html(content)))
}

fn render_list(lines: &[&str], start: usize) -> (String, usize) {
    let mut items = Vec::new();
    let mut index = start;

    while index < lines.len() {
        let line = lines[index];
        if let Some((marker, content)) = parse_list_item(line) {
            let prefix = marker.unwrap_or_else(|| "•".to_string());
            items.push(format!("{prefix} {}", render_inline_html(content.trim())));
            index += 1;
            continue;
        }
        break;
    }

    (items.join("\n"), index)
}

fn render_paragraph(lines: &[&str], start: usize) -> (String, usize) {
    let mut parts = Vec::new();
    let mut index = start;

    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();
        if trimmed.is_empty()
            || parse_fenced_code_block(lines, index).is_some()
            || is_blockquote_line(trimmed)
            || render_heading(trimmed).is_some()
            || is_list_item(line)
            || is_horizontal_rule(trimmed)
        {
            break;
        }

        parts.push(render_inline_html(trimmed));
        index += 1;
    }

    (parts.join("\n"), index)
}

fn try_render_link(input: &str) -> Option<(String, usize)> {
    let bracketed = input.strip_prefix('[')?;
    let text_end = find_unescaped_char(bracketed, ']')?;
    let text = &bracketed[..text_end];
    let after_text = &bracketed[text_end + 1..];
    let url_part = after_text.strip_prefix('(')?;
    let url_end = find_unescaped_char(url_part, ')')?;
    let url = url_part[..url_end].trim();
    if url.is_empty() {
        return None;
    }

    let consumed = 1 + text.len() + 1 + 1 + url_end + 1;
    Some((
        format!(
            "<a href=\"{}\">{}</a>",
            escape_html_attribute(url),
            render_inline_html(text)
        ),
        consumed,
    ))
}

fn try_render_code_span(input: &str) -> Option<(String, usize)> {
    let rest = input.strip_prefix('`')?;
    let end = find_unescaped_char(rest, '`')?;
    let content = &rest[..end];
    Some((
        format!("<code>{}</code>", escape_html(content)),
        1 + end + 1,
    ))
}

fn try_render_delimited_span(input: &str, delimiter: &str, tag: &str) -> Option<(String, usize)> {
    let rest = input.strip_prefix(delimiter)?;
    let end = find_unescaped_subslice(rest, delimiter)?;
    let content = &rest[..end];
    if content.trim().is_empty() {
        return None;
    }

    Some((
        format!("<{tag}>{}</{tag}>", render_inline_html(content)),
        delimiter.len() + end + delimiter.len(),
    ))
}

fn try_render_emphasis(input: &str, delimiter: char) -> Option<(String, usize)> {
    if !input.starts_with(delimiter) {
        return None;
    }

    let rest = &input[delimiter.len_utf8()..];
    let end = find_unescaped_char(rest, delimiter)?;
    let content = &rest[..end];
    if content.trim().is_empty() {
        return None;
    }

    Some((
        format!("<i>{}</i>", render_inline_html(content)),
        delimiter.len_utf8() + end + delimiter.len_utf8(),
    ))
}

fn find_unescaped_char(input: &str, target: char) -> Option<usize> {
    let mut escaped = false;

    for (index, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == target {
            return Some(index);
        }
    }

    None
}

fn find_unescaped_subslice(input: &str, needle: &str) -> Option<usize> {
    let mut start = 0;
    while let Some(relative) = input[start..].find(needle) {
        let index = start + relative;
        let escaped = input[..index]
            .chars()
            .rev()
            .take_while(|ch| *ch == '\\')
            .count()
            % 2
            == 1;
        if !escaped {
            return Some(index);
        }
        start = index + needle.len();
    }
    None
}

fn is_blockquote_line(line: &str) -> bool {
    line.starts_with('>')
}

fn is_horizontal_rule(line: &str) -> bool {
    matches!(line, "---" | "***" | "___")
}

fn is_list_item(line: &str) -> bool {
    parse_list_item(line).is_some()
}

fn parse_list_item(line: &str) -> Option<(Option<String>, &str)> {
    let trimmed = line.trim_start();
    for marker in ["- ", "* ", "+ "] {
        if let Some(content) = trimmed.strip_prefix(marker) {
            return Some((None, content));
        }
    }

    let digit_count = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count == 0 {
        return None;
    }

    let suffix = trimmed.get(digit_count..)?;
    let separator = suffix.chars().next()?;
    if separator != '.' && separator != ')' {
        return None;
    }

    let content = suffix[separator.len_utf8()..].strip_prefix(' ')?;
    let marker = format!("{}.", &trimmed[..digit_count]);
    Some((Some(marker), content))
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

fn escape_html_attribute(input: &str) -> String {
    escape_html(input).replace('"', "&quot;")
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
        assert!(rendered.contains("<pre><code class=\"language-text\">/help</code></pre>"));
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

    #[test]
    fn markdownish_renderer_supports_links_quotes_lists_and_inline_styles() {
        let output = ChannelResponse {
            content: "# Title\n\n> quoted **line**\n\n- item with [link](https://example.com?q=1&x=2)\n1. `code`\n\n__underline__ ~~gone~~ ||spoiler|| _italic_".to_string(),
            reasoning: None,
            metadata: BTreeMap::new(),
        };

        let rendered = render_telegram_response(&output, false);
        assert!(rendered.contains("<b>Title</b>"));
        assert!(rendered.contains("<blockquote>quoted <b>line</b></blockquote>"));
        assert!(
            rendered.contains("• item with <a href=\"https://example.com?q=1&amp;x=2\">link</a>")
        );
        assert!(rendered.contains("1. <code>code</code>"));
        assert!(rendered.contains("<u>underline</u>"));
        assert!(rendered.contains("<s>gone</s>"));
        assert!(rendered.contains("<tg-spoiler>spoiler</tg-spoiler>"));
        assert!(rendered.contains("<i>italic</i>"));
    }

    #[test]
    fn fenced_code_block_with_language_uses_telegram_supported_code_class() {
        let rendered = markdownish_to_telegram_html("```rust\nfn main() {}\n```");
        assert_eq!(
            rendered,
            "<pre><code class=\"language-rust\">fn main() {}</code></pre>"
        );
    }
}
