use crate::state::{AppState, AppStatus, MessageRole};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
};

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
    frame.render_widget(status_widget(state), outer[1]);
}

fn messages_widget(state: &AppState) -> Paragraph<'static> {
    let mut text = Text::default();
    if state.messages().is_empty() {
        text.lines.push(Line::from(vec![Span::styled(
            "Ready for instructions",
            Style::default().fg(Color::DarkGray),
        )]));
    } else {
        for message in state.messages() {
            let (label, color) = match message.role {
                MessageRole::User => ("you", Color::Cyan),
                MessageRole::Agent => ("agent", Color::Magenta),
                MessageRole::Error => ("error", Color::Red),
            };
            text.lines.push(Line::from(vec![
                Span::styled(
                    format!("{label}> "),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(message.content.clone()),
            ]));
            text.lines.push(Line::default());
        }
    }

    Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Chat"))
        .wrap(Wrap { trim: false })
}

fn input_widget(state: &AppState) -> Paragraph<'static> {
    let input = if state.input().is_empty() {
        " ".to_string()
    } else {
        state.input().to_string()
    };

    Paragraph::new(input)
        .block(Block::default().borders(Borders::ALL).title("Input"))
        .wrap(Wrap { trim: false })
}

fn help_widget(state: &AppState) -> Paragraph<'static> {
    let status = match state.status() {
        AppStatus::Idle => "idle",
        AppStatus::Submitting => "submitting",
    };
    let line = format!(
        "Enter send | Shift+Enter newline | Ctrl+R reasoning | Ctrl+L clear | Ctrl+N new session | Esc quit | status: {status}"
    );
    Paragraph::new(line).block(Block::default().borders(Borders::ALL).title("Help"))
}

fn status_widget(state: &AppState) -> Paragraph<'static> {
    let meta = state.meta();
    let lines = vec![
        Line::from(vec![
            Span::styled("Version: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(meta.version.clone()),
        ]),
        Line::from(vec![
            Span::styled("Channel: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(meta.channel.clone()),
        ]),
        Line::from(vec![
            Span::styled("Session: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(meta.session_key.clone()),
        ]),
        Line::from(vec![
            Span::styled("Provider: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(meta.provider.clone()),
        ]),
        Line::from(vec![
            Span::styled("Model: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(meta.model.clone()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Skills: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(meta.skills.clone()),
        ]),
        Line::from(vec![
            Span::styled("Tools: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(meta.tools.clone()),
        ]),
        Line::from(vec![
            Span::styled("MCP: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(meta.mcp.clone()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Reasoning: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(if meta.show_reasoning { "on" } else { "off" }),
        ]),
    ];

    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .wrap(Wrap { trim: false })
}
