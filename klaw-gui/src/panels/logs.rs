use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::runtime_bridge;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

const LOG_POLL_INTERVAL: Duration = Duration::from_millis(200);
const LOG_DRAIN_BATCH: usize = 512;
const DEFAULT_MAX_LINES: usize = 5000;
const MIN_MAX_LINES: usize = 100;
const MAX_MAX_LINES: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Unknown,
}

#[derive(Debug, Clone)]
struct LogEntry {
    level: ParsedLevel,
    line: String,
}

#[derive(Debug, Clone)]
struct LevelFilter {
    trace: bool,
    debug: bool,
    info: bool,
    warn: bool,
    error: bool,
    unknown: bool,
}

impl Default for LevelFilter {
    fn default() -> Self {
        Self {
            trace: true,
            debug: true,
            info: true,
            warn: true,
            error: true,
            unknown: true,
        }
    }
}

impl LevelFilter {
    fn matches(&self, level: ParsedLevel) -> bool {
        match level {
            ParsedLevel::Trace => self.trace,
            ParsedLevel::Debug => self.debug,
            ParsedLevel::Info => self.info,
            ParsedLevel::Warn => self.warn,
            ParsedLevel::Error => self.error,
            ParsedLevel::Unknown => self.unknown,
        }
    }
}

#[derive(Default)]
pub struct LogsPanel {
    entries: VecDeque<LogEntry>,
    pending_fragment: String,
    dropped_lines: usize,
    max_lines: usize,
    max_lines_text: String,
    auto_scroll: bool,
    search_text: String,
    level_filter: LevelFilter,
    paused: bool,
    export_path: String,
}

impl LogsPanel {
    fn ensure_defaults(&mut self) {
        if self.max_lines == 0 {
            self.max_lines = DEFAULT_MAX_LINES;
        }
        if self.max_lines_text.is_empty() {
            self.max_lines_text = self.max_lines.to_string();
        }
        if self.export_path.is_empty() {
            self.export_path = default_export_path();
        }
        if !self.auto_scroll {
            self.auto_scroll = true;
        }
    }

    fn drain_runtime_logs(&mut self) {
        let chunks = runtime_bridge::drain_log_chunks(LOG_DRAIN_BATCH);
        for chunk in chunks {
            self.absorb_chunk(&chunk);
        }
    }

    fn absorb_chunk(&mut self, chunk: &str) {
        self.pending_fragment.push_str(chunk);
        let mut completed = Vec::new();
        let mut rest = self.pending_fragment.as_str();
        while let Some(newline_idx) = rest.find('\n') {
            let mut line = &rest[..newline_idx];
            if line.ends_with('\r') {
                line = &line[..line.len().saturating_sub(1)];
            }
            completed.push(line.to_string());
            rest = &rest[newline_idx + 1..];
        }
        self.pending_fragment = rest.to_string();

        for line in completed {
            self.push_line(line);
        }
    }

    fn push_line(&mut self, line: String) {
        let clean_line = strip_ansi_sequences(&line);
        let entry = LogEntry {
            level: parse_level(&clean_line),
            line: clean_line,
        };
        self.entries.push_back(entry);
        while self.entries.len() > self.max_lines {
            self.entries.pop_front();
            self.dropped_lines = self.dropped_lines.saturating_add(1);
        }
    }

    fn apply_max_lines_from_text(&mut self) -> Result<(), String> {
        let parsed = self
            .max_lines_text
            .trim()
            .parse::<usize>()
            .map_err(|_| "max lines must be an integer".to_string())?;
        if !(MIN_MAX_LINES..=MAX_MAX_LINES).contains(&parsed) {
            return Err(format!(
                "max lines must be between {MIN_MAX_LINES} and {MAX_MAX_LINES}"
            ));
        }
        self.max_lines = parsed;
        while self.entries.len() > self.max_lines {
            self.entries.pop_front();
            self.dropped_lines = self.dropped_lines.saturating_add(1);
        }
        Ok(())
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.pending_fragment.clear();
        self.dropped_lines = 0;
    }

    fn export_all(&self) -> Result<PathBuf, String> {
        let path = expand_tilde(self.export_path.trim());
        if path.as_os_str().is_empty() {
            return Err("export path cannot be empty".to_string());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("failed to create dir: {err}"))?;
        }
        let mut output = String::new();
        for entry in &self.entries {
            output.push_str(&entry.line);
            output.push('\n');
        }
        fs::write(&path, output).map_err(|err| format!("failed to write log file: {err}"))?;
        Ok(path)
    }
}

impl PanelRenderer for LogsPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_defaults();
        if !self.paused {
            self.drain_runtime_logs();
        }
        ui.ctx().request_repaint_after(LOG_POLL_INTERVAL);

        ui.heading(ctx.tab_title);
        ui.label("Live process logs from tracing output");
        ui.separator();

        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.level_filter.trace, "trace");
            ui.checkbox(&mut self.level_filter.debug, "debug");
            ui.checkbox(&mut self.level_filter.info, "info");
            ui.checkbox(&mut self.level_filter.warn, "warn");
            ui.checkbox(&mut self.level_filter.error, "error");
            ui.checkbox(&mut self.level_filter.unknown, "unknown");
        });

        ui.horizontal(|ui| {
            ui.label("Search");
            ui.add(egui::TextEdit::singleline(&mut self.search_text).desired_width(220.0));
            ui.checkbox(&mut self.paused, "Pause stream");
            ui.checkbox(&mut self.auto_scroll, "Auto-scroll");
            if ui.button("Clear").clicked() {
                self.clear();
                notifications.success("Log buffer cleared");
            }
        });

        ui.horizontal(|ui| {
            ui.label("Max lines");
            ui.add(egui::TextEdit::singleline(&mut self.max_lines_text).desired_width(90.0));
            if ui.button("Apply").clicked() {
                match self.apply_max_lines_from_text() {
                    Ok(()) => notifications.success("Log capacity updated"),
                    Err(err) => notifications.error(err),
                }
            }
            ui.separator();
            ui.label("Export path");
            ui.add(egui::TextEdit::singleline(&mut self.export_path).desired_width(320.0));
            if ui.button("Export").clicked() {
                match self.export_all() {
                    Ok(path) => {
                        notifications.success(format!("Logs exported to {}", path.display()))
                    }
                    Err(err) => notifications.error(err),
                }
            }
        });

        let visible = visible_entries(&self.entries, &self.level_filter, &self.search_text);
        ui.label(format!(
            "Buffered: {} | Visible: {} | Dropped: {}",
            self.entries.len(),
            visible.len(),
            self.dropped_lines
        ));
        ui.separator();

        egui::ScrollArea::vertical()
            .id_salt("logs-panel-scroll")
            .auto_shrink([false, false])
            .stick_to_bottom(self.auto_scroll && !self.paused)
            .show(ui, |ui| {
                for entry in visible {
                    let color = match entry.level {
                        ParsedLevel::Warn => ui.visuals().warn_fg_color,
                        ParsedLevel::Error => ui.visuals().error_fg_color,
                        _ => ui.visuals().text_color(),
                    };
                    ui.label(egui::RichText::new(&entry.line).monospace().color(color));
                }
            });
    }
}

fn visible_entries<'a>(
    entries: &'a VecDeque<LogEntry>,
    filter: &LevelFilter,
    search_text: &str,
) -> Vec<&'a LogEntry> {
    let search = search_text.trim().to_ascii_lowercase();
    entries
        .iter()
        .filter(|entry| filter.matches(entry.level))
        .filter(|entry| {
            if search.is_empty() {
                return true;
            }
            entry.line.to_ascii_lowercase().contains(&search)
        })
        .collect()
}

fn parse_level(line: &str) -> ParsedLevel {
    let lower = line.to_ascii_lowercase();
    if lower.contains("error") || lower.contains(" level=error") {
        ParsedLevel::Error
    } else if lower.contains("warn") || lower.contains(" level=warn") {
        ParsedLevel::Warn
    } else if lower.contains("info") || lower.contains(" level=info") {
        ParsedLevel::Info
    } else if lower.contains("debug") || lower.contains(" level=debug") {
        ParsedLevel::Debug
    } else if lower.contains("trace") || lower.contains(" level=trace") {
        ParsedLevel::Trace
    } else {
        ParsedLevel::Unknown
    }
}

fn strip_ansi_sequences(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut iter = input.chars().peekable();

    while let Some(ch) = iter.next() {
        if ch != '\u{1b}' {
            output.push(ch);
            continue;
        }

        let Some(next) = iter.peek().copied() else {
            break;
        };

        match next {
            '[' => {
                // CSI sequence: ESC [ ... final-byte
                let _ = iter.next();
                for c in iter.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            ']' => {
                // OSC sequence: ESC ] ... BEL or ST(ESC \)
                let _ = iter.next();
                let mut prev = '\0';
                for c in iter.by_ref() {
                    if c == '\u{7}' || (prev == '\u{1b}' && c == '\\') {
                        break;
                    }
                    prev = c;
                }
            }
            _ => {
                // Fallback for short ESC sequence (drop ESC + next char).
                let _ = iter.next();
            }
        }
    }

    output
}

fn default_export_path() -> String {
    expand_tilde("~/.klaw/logs/gui-live.log")
        .to_string_lossy()
        .to_string()
}

fn expand_tilde(raw: &str) -> PathBuf {
    if !raw.starts_with("~/") {
        return PathBuf::from(raw);
    }
    let Some(home) = std::env::var_os("HOME") else {
        return PathBuf::from(raw);
    };
    PathBuf::from(home).join(raw.trim_start_matches("~/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_capacity_discards_oldest_lines() {
        let mut panel = LogsPanel {
            max_lines: 2,
            max_lines_text: "2".to_string(),
            ..Default::default()
        };

        panel.absorb_chunk("a\nb\nc\n");
        assert_eq!(panel.entries.len(), 2);
        assert_eq!(panel.entries[0].line, "b");
        assert_eq!(panel.entries[1].line, "c");
        assert_eq!(panel.dropped_lines, 1);
    }

    #[test]
    fn visible_entries_apply_level_and_search_filters() {
        let mut entries = VecDeque::new();
        entries.push_back(LogEntry {
            level: ParsedLevel::Info,
            line: "info boot complete".to_string(),
        });
        entries.push_back(LogEntry {
            level: ParsedLevel::Error,
            line: "error network failed".to_string(),
        });
        let filter = LevelFilter {
            info: false,
            ..LevelFilter::default()
        };

        let visible = visible_entries(&entries, &filter, "network");
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].level, ParsedLevel::Error);
    }

    #[test]
    fn parse_level_detects_known_levels() {
        assert_eq!(parse_level("INFO started"), ParsedLevel::Info);
        assert_eq!(parse_level("WARN cache miss"), ParsedLevel::Warn);
        assert_eq!(parse_level("ERROR panic"), ParsedLevel::Error);
        assert_eq!(parse_level("DEBUG cmd"), ParsedLevel::Debug);
        assert_eq!(parse_level("TRACE frame"), ParsedLevel::Trace);
        assert_eq!(parse_level("custom line"), ParsedLevel::Unknown);
    }

    #[test]
    fn strip_ansi_sequences_removes_terminal_escape_codes() {
        let raw = "\u{1b}[2mwarn\u{1b}[0m plain \u{1b}[31merror\u{1b}[0m";
        let clean = strip_ansi_sequences(raw);
        assert_eq!(clean, "warn plain error");
    }
}
