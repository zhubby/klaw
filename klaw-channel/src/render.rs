use crate::ChannelResponse;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputRenderStyle {
    Markdown,
    Terminal,
}

pub fn render_agent_output(
    output: &ChannelResponse,
    show_reasoning: bool,
    style: OutputRenderStyle,
) -> String {
    match style {
        OutputRenderStyle::Markdown => render_markdown_output(output, show_reasoning),
        OutputRenderStyle::Terminal => render_terminal_output(output, show_reasoning),
    }
}

fn render_markdown_output(output: &ChannelResponse, show_reasoning: bool) -> String {
    let mut content = output.content.trim().to_string();
    if show_reasoning && let Some(reasoning) = output.reasoning.as_ref() {
        let reasoning = reasoning
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        if !reasoning.is_empty() {
            if !content.is_empty() {
                content.push_str("\n\n");
            }
            content.push_str("**Reasoning**\n");
            content.push_str(&reasoning);
        }
    }
    content
}

fn render_terminal_output(output: &ChannelResponse, show_reasoning: bool) -> String {
    let mut lines = vec![
        "--------------------".to_string(),
        "[answer]".to_string(),
        output.content.trim().to_string(),
    ];
    if show_reasoning && let Some(reasoning_text) = &output.reasoning {
        lines.push(String::new());
        lines.push("[reasoning]".to_string());
        lines.extend(reasoning_text.lines().map(|line| format!("> {line}")));
    }
    lines.push("--------------------".to_string());
    lines.join("\n")
}
