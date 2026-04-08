use crate::state::{AppState, AppStatus, MessageRole};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn render(frame: &mut Frame, state: &AppState) {
    let outer = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(72), Constraint::Percentage(28)])
        .split(frame.area());

    let main = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),
            Constraint::Length(5),
            Constraint::Length(3),
        ])
        .split(outer[0]);

    frame.render_widget(messages_widget(state), main[0]);
    frame.render_widget(input_widget(state), main[1]);
    frame.render_widget(help_widget(state), main[2]);
    let status_w = outer[1].width;
    frame.render_widget(status_widget(state, status_w), outer[1]);
}

fn panel_block(title: impl Into<String>, accent: Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .title(Line::from(vec![
            Span::styled("◆ ", Style::default().fg(accent)),
            Span::styled(
                title.into(),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
        ]))
}

fn wall_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn spinner_frame_index() -> usize {
    ((wall_millis() / 90) % 8) as usize
}

fn braille_spinner() -> &'static str {
    const FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
    FRAMES[spinner_frame_index()]
}

/// Rotating status lines before the first model token arrives (time-based, not RNG).
const WAIT_PHRASES: [&str; 10] = [
    "Waiting for the model…",
    "Connecting and loading context…",
    "Thinking through your message…",
    "Tools and memory may be consulted…",
    "Reasoning — this can take a moment…",
    "Still working on a reply…",
    "Organizing the answer…",
    "Refining details…",
    "Almost ready to stream…",
    "Hang tight…",
];

fn wait_phrase_index() -> usize {
    let ms = wall_millis();
    ((ms / 2800) as usize) % WAIT_PHRASES.len()
}

fn push_agent_waiting_placeholder(text: &mut Text<'static>, label: &str, label_color: Color) {
    let phrase = WAIT_PHRASES[wait_phrase_index()];
    let body_style = Style::default()
        .fg(Color::LightYellow)
        .add_modifier(Modifier::ITALIC);

    text.lines.push(Line::from(vec![
        Span::styled(
            format!("{label}> "),
            Style::default()
                .fg(label_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{} {}", braille_spinner(), phrase),
            body_style,
        ),
    ]));
    text.lines.push(Line::default());
}

fn messages_widget(state: &AppState) -> Paragraph<'static> {
    let mut text = Text::default();
    let accent = if state.status() == AppStatus::Submitting {
        Color::LightYellow
    } else {
        Color::LightCyan
    };

    if state.messages().is_empty() {
        text.lines.push(Line::from(vec![
            Span::styled(
                braille_spinner(),
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " Ready for instructions",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ),
        ]));
    } else {
        for message in state.messages() {
            let (label, color) = match message.role {
                MessageRole::User => ("you", Color::Cyan),
                MessageRole::Agent => ("agent", Color::Magenta),
                MessageRole::Error => ("error", Color::Red),
            };

            if message.role == MessageRole::Agent
                && message.content.is_empty()
                && state.status() == AppStatus::Submitting
            {
                push_agent_waiting_placeholder(&mut text, label, color);
                continue;
            }

            text.lines.push(Line::from(vec![
                Span::styled(
                    format!("{label}> "),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(message.content.clone(), Style::default()),
            ]));
            text.lines.push(Line::default());
        }
    }

    Paragraph::new(text)
        .block(panel_block("Chat", accent))
        .wrap(Wrap { trim: false })
}

fn input_widget(state: &AppState) -> Paragraph<'static> {
    let input = if state.input().is_empty() {
        " ".to_string()
    } else {
        state.input().to_string()
    };

    let style = if state.input().trim().is_empty() {
        Style::default()
    } else {
        Style::default().fg(Color::White)
    };

    Paragraph::new(Span::styled(input, style))
        .block(panel_block("Input", Color::LightGreen))
        .wrap(Wrap { trim: false })
}

fn status_content_width(panel_width: u16) -> usize {
    // Horizontal borders consume two columns inside the block’s inner area.
    panel_width.saturating_sub(2).max(12) as usize
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// Cap comma-separated lists so the status column stays scannable; full lists are still long in config.
fn cap_comma_separated(raw: &str, max_items: usize, more_noun: &str) -> String {
    let items: Vec<&str> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if items.len() <= max_items {
        return items.join(", ");
    }
    let head = items[..max_items].join(", ");
    let more = items.len() - max_items;
    format!("{head}, … (+{more} {more_noun})")
}

fn wrap_words(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let wlen = char_len(word);
        if wlen > width {
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            let mut rest = word.to_string();
            while char_len(&rest) > width {
                let chunk: String = rest.chars().take(width).collect();
                let n = chunk.chars().count();
                rest = rest.chars().skip(n).collect();
                lines.push(chunk);
            }
            if !rest.is_empty() {
                line = rest;
            }
            continue;
        }
        let need = if line.is_empty() {
            wlen
        } else {
            char_len(&line) + 1 + wlen
        };
        if need <= width {
            if line.is_empty() {
                line = word.to_string();
            } else {
                line.push(' ');
                line.push_str(word);
            }
        } else {
            lines.push(std::mem::take(&mut line));
            line = word.to_string();
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn wrap_comma_list(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let items: Vec<String> = text
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let mut lines = Vec::new();
    let mut cur = String::new();
    for item in items {
        let candidate = if cur.is_empty() {
            item.clone()
        } else {
            format!("{cur}, {item}")
        };
        if char_len(&candidate) <= width {
            cur = candidate;
        } else {
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            if char_len(&item) <= width {
                cur = item;
            } else {
                let mut rest = item;
                while char_len(&rest) > width {
                    let chunk: String = rest.chars().take(width).collect();
                    let n = chunk.chars().count();
                    rest = rest.chars().skip(n).collect();
                    lines.push(chunk);
                }
                cur = rest;
            }
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

#[derive(Clone, Copy)]
enum StatusValueKind {
    /// Prefer breaking at spaces; single long tokens are split by character width.
    Plain,
    /// Prefer breaking at comma boundaries, then like plain.
    CommaList,
}

const STATUS_VALUE_INDENT: &str = "  ";

fn push_status_field(
    out: &mut Vec<Line<'static>>,
    label: &'static str,
    value: &str,
    content_width: usize,
    kind: StatusValueKind,
) {
    let max_val_w = content_width
        .saturating_sub(char_len(STATUS_VALUE_INDENT))
        .max(1);

    out.push(Line::from(Span::styled(
        label,
        Style::default()
            .fg(Color::LightBlue)
            .add_modifier(Modifier::BOLD),
    )));

    let body: Vec<String> = match kind {
        StatusValueKind::Plain => {
            let single_token = !value.chars().any(|c| c.is_whitespace());
            if single_token && char_len(value) > max_val_w {
                let mut rest = value.to_string();
                let mut v = Vec::new();
                while char_len(&rest) > max_val_w {
                    let chunk: String = rest.chars().take(max_val_w).collect();
                    let n = chunk.chars().count();
                    rest = rest.chars().skip(n).collect();
                    v.push(chunk);
                }
                if !rest.is_empty() {
                    v.push(rest);
                }
                v
            } else {
                wrap_words(value, max_val_w)
            }
        }
        StatusValueKind::CommaList => wrap_comma_list(value, max_val_w),
    };

    for part in body {
        out.push(Line::from(vec![Span::raw(format!(
            "{STATUS_VALUE_INDENT}{part}"
        ))]));
    }
    out.push(Line::default());
}

fn help_widget(state: &AppState) -> Paragraph<'static> {
    let status = match state.status() {
        AppStatus::Idle => ("idle", Color::DarkGray),
        AppStatus::Submitting => ("submitting", Color::LightYellow),
    };
    let line = Line::from(vec![
        Span::styled(
            format!("Enter send | Shift+Enter newline | status: "),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            status.0,
            Style::default().fg(status.1).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " | Ctrl+R reasoning | Ctrl+L clear | Ctrl+N new session | Esc quit",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    Paragraph::new(line).block(panel_block("Help", Color::DarkGray))
}

fn status_widget(state: &AppState, panel_width: u16) -> Paragraph<'static> {
    let content_w = status_content_width(panel_width);
    let meta = state.meta();
    let mut lines: Vec<Line<'static>> = Vec::new();

    push_status_field(
        &mut lines,
        "Version:",
        &meta.version,
        content_w,
        StatusValueKind::Plain,
    );
    push_status_field(
        &mut lines,
        "Channel:",
        &meta.channel,
        content_w,
        StatusValueKind::Plain,
    );
    push_status_field(
        &mut lines,
        "Session:",
        &meta.session_key,
        content_w,
        StatusValueKind::Plain,
    );

    lines.push(Line::default());

    push_status_field(
        &mut lines,
        "Provider:",
        &meta.provider,
        content_w,
        StatusValueKind::Plain,
    );
    push_status_field(&mut lines, "Model:", &meta.model, content_w, StatusValueKind::Plain);

    lines.push(Line::default());

    let skills = cap_comma_separated(&meta.skills, 12, "more");
    push_status_field(&mut lines, "Skills:", &skills, content_w, StatusValueKind::CommaList);

    let tools = cap_comma_separated(&meta.tools, 10, "more");
    push_status_field(&mut lines, "Tools:", &tools, content_w, StatusValueKind::CommaList);

    push_status_field(&mut lines, "MCP:", &meta.mcp, content_w, StatusValueKind::Plain);

    lines.push(Line::default());

    push_status_field(
        &mut lines,
        "Reasoning:",
        if meta.show_reasoning { "on" } else { "off" },
        content_w,
        StatusValueKind::Plain,
    );

    while lines.last().is_some_and(|l| l.spans.is_empty()) {
        lines.pop();
    }

    Paragraph::new(lines)
        .block(panel_block("Status", Color::LightBlue))
        .wrap(Wrap { trim: false })
}
