use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::runtime_bridge;
use crate::state::{LogsLevelFilterState, LogsPanelState, persistence};
use egui::Color32;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

const LOG_POLL_INTERVAL: Duration = Duration::from_millis(200);
const LOG_DRAIN_BATCH: usize = 512;
const LOG_BACKGROUND_DRAIN_BATCH: usize = LOG_DRAIN_BATCH / 4;
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
        Self::from_persisted(LogsLevelFilterState::default())
    }
}

impl LevelFilter {
    fn from_persisted(state: LogsLevelFilterState) -> Self {
        Self {
            trace: state.trace,
            debug: state.debug,
            info: state.info,
            warn: state.warn,
            error: state.error,
            unknown: state.unknown,
        }
    }

    fn as_persisted(&self) -> LogsLevelFilterState {
        LogsLevelFilterState {
            trace: self.trace,
            debug: self.debug,
            info: self.info,
            warn: self.warn,
            error: self.error,
            unknown: self.unknown,
        }
    }

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
    filtered_indices: Vec<usize>,
    pending_fragment: String,
    dropped_lines: usize,
    max_lines: usize,
    max_lines_text: String,
    auto_scroll: bool,
    search_text: String,
    level_filter: LevelFilter,
    paused: bool,
    export_path: String,
    prefs_loaded: bool,
    visible_cache_dirty: bool,
}

impl LogsPanel {
    fn ensure_defaults(&mut self) {
        if !self.prefs_loaded {
            self.level_filter =
                LevelFilter::from_persisted(persistence::load_ui_state().logs_panel.level_filter);
            self.prefs_loaded = true;
        }
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

    pub fn tick(&mut self, ctx: &egui::Context) {
        self.ensure_defaults();
        if !self.paused {
            self.drain_runtime_logs(LOG_BACKGROUND_DRAIN_BATCH);
        }
        ctx.request_repaint_after(LOG_POLL_INTERVAL);
    }

    fn drain_runtime_logs(&mut self, max_batch: usize) {
        let chunks = runtime_bridge::drain_log_chunks(max_batch);
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
        self.visible_cache_dirty = true;
        while self.entries.len() > self.max_lines {
            self.entries.pop_front();
            self.dropped_lines = self.dropped_lines.saturating_add(1);
            self.visible_cache_dirty = true;
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
        self.visible_cache_dirty = true;
        Ok(())
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.filtered_indices.clear();
        self.pending_fragment.clear();
        self.dropped_lines = 0;
        self.visible_cache_dirty = true;
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

    fn persist_level_filter(&self) -> Result<(), String> {
        persistence::update_ui_state(|state| {
            state.logs_panel = LogsPanelState {
                level_filter: self.level_filter.as_persisted(),
            };
        })
        .map(|_| ())
        .map_err(|err| format!("failed to save log filter preferences: {err}"))
    }

    fn refresh_visible_cache(&mut self) {
        if !self.visible_cache_dirty {
            return;
        }

        self.filtered_indices =
            filtered_entry_indices(&self.entries, &self.level_filter, &self.search_text);
        self.visible_cache_dirty = false;
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
        ui.ctx().request_repaint_after(LOG_POLL_INTERVAL);

        ui.heading(ctx.tab_title);
        ui.label("Live process logs from tracing output");
        ui.separator();

        ui.horizontal_wrapped(|ui| {
            let trace_label =
                egui::RichText::new("trace").color(level_color(ParsedLevel::Trace, ui));
            let debug_label =
                egui::RichText::new("debug").color(level_color(ParsedLevel::Debug, ui));
            let info_label = egui::RichText::new("info").color(level_color(ParsedLevel::Info, ui));
            let warn_label = egui::RichText::new("warn").color(level_color(ParsedLevel::Warn, ui));
            let error_label =
                egui::RichText::new("error").color(level_color(ParsedLevel::Error, ui));
            let unknown_label =
                egui::RichText::new("unknown").color(level_color(ParsedLevel::Unknown, ui));
            let changed = ui
                .checkbox(&mut self.level_filter.trace, trace_label)
                .changed()
                || ui
                    .checkbox(&mut self.level_filter.debug, debug_label)
                    .changed()
                || ui
                    .checkbox(&mut self.level_filter.info, info_label)
                    .changed()
                || ui
                    .checkbox(&mut self.level_filter.warn, warn_label)
                    .changed()
                || ui
                    .checkbox(&mut self.level_filter.error, error_label)
                    .changed()
                || ui
                    .checkbox(&mut self.level_filter.unknown, unknown_label)
                    .changed();
            if changed {
                self.visible_cache_dirty = true;
            }
            if changed && let Err(err) = self.persist_level_filter() {
                notifications.error(err);
            }
        });

        ui.horizontal(|ui| {
            ui.label("Search");
            let search_response =
                ui.add(egui::TextEdit::singleline(&mut self.search_text).desired_width(220.0));
            if search_response.changed() {
                self.visible_cache_dirty = true;
            }
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

        self.refresh_visible_cache();
        let transport_stats = runtime_bridge::log_stats_snapshot();
        ui.label(format!(
            "Buffered: {} | Visible: {} | Panel dropped: {} | Transport dropped: {} | Bridge dropped: {}",
            self.entries.len(),
            self.filtered_indices.len(),
            self.dropped_lines,
            transport_stats.transport_dropped_chunks,
            transport_stats.bridge_dropped_chunks,
        ));
        if transport_stats.transport_dropped_chunks > 0 {
            ui.label(format!(
                "GUI transport has dropped {} chunks ({} bytes). Runtime logging continued, but the GUI sink fell behind.",
                transport_stats.transport_dropped_chunks,
                transport_stats.transport_dropped_bytes
            ));
        }
        ui.separator();

        let row_height = ui.text_style_height(&egui::TextStyle::Monospace).max(1.0);
        egui::ScrollArea::vertical()
            .id_salt("logs-panel-scroll")
            .auto_shrink([false, false])
            .stick_to_bottom(self.auto_scroll && !self.paused)
            .show_rows(
                ui,
                row_height,
                self.filtered_indices.len(),
                |ui, row_range| {
                    for row in row_range {
                        let Some(entry_idx) = self.filtered_indices.get(row).copied() else {
                            continue;
                        };
                        let Some(entry) = self.entries.get(entry_idx) else {
                            continue;
                        };
                        let color = level_color(entry.level, ui);
                        ui.label(egui::RichText::new(&entry.line).monospace().color(color));
                    }
                },
            );
    }
}

fn level_color(level: ParsedLevel, ui: &egui::Ui) -> Color32 {
    match level {
        ParsedLevel::Trace => Color32::from_rgb(140, 140, 140),
        ParsedLevel::Debug => Color32::from_rgb(70, 130, 200),
        ParsedLevel::Info => ui.visuals().text_color(),
        ParsedLevel::Warn => ui.visuals().warn_fg_color,
        ParsedLevel::Error => ui.visuals().error_fg_color,
        ParsedLevel::Unknown => ui.visuals().text_color(),
    }
}

fn filtered_entry_indices(
    entries: &VecDeque<LogEntry>,
    filter: &LevelFilter,
    search_text: &str,
) -> Vec<usize> {
    let search = search_text.trim().to_ascii_lowercase();
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| filter.matches(entry.level))
        .filter(|(_, entry)| {
            if search.is_empty() {
                return true;
            }
            entry.line.to_ascii_lowercase().contains(&search)
        })
        .map(|(idx, _)| idx)
        .collect()
}

fn parse_level(line: &str) -> ParsedLevel {
    for token in line.split_whitespace() {
        if let Some(level) = parse_level_token(token) {
            return level;
        }
    }

    ParsedLevel::Unknown
}

fn parse_level_token(token: &str) -> Option<ParsedLevel> {
    let normalized = token
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '=')
        .to_ascii_lowercase();

    match normalized.as_str() {
        "trace" | "level=trace" => Some(ParsedLevel::Trace),
        "debug" | "level=debug" => Some(ParsedLevel::Debug),
        "info" | "level=info" => Some(ParsedLevel::Info),
        "warn" | "warning" | "level=warn" | "level=warning" => Some(ParsedLevel::Warn),
        "error" | "level=error" => Some(ParsedLevel::Error),
        _ => None,
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
            error: true,
            ..LevelFilter::default()
        };

        let visible = filtered_entry_indices(&entries, &filter, "network");
        assert_eq!(visible, vec![1]);
    }

    #[test]
    fn level_filter_defaults_to_info_only() {
        let filter = LevelFilter::default();
        assert!(!filter.trace);
        assert!(!filter.debug);
        assert!(filter.info);
        assert!(!filter.warn);
        assert!(!filter.error);
        assert!(!filter.unknown);
    }

    #[test]
    fn parse_level_detects_known_levels() {
        assert_eq!(parse_level("INFO started"), ParsedLevel::Info);
        assert_eq!(parse_level("WARN cache miss"), ParsedLevel::Warn);
        assert_eq!(parse_level("ERROR panic"), ParsedLevel::Error);
        assert_eq!(parse_level("DEBUG cmd"), ParsedLevel::Debug);
        assert_eq!(parse_level("TRACE frame"), ParsedLevel::Trace);
        assert_eq!(parse_level("ts level=info booted"), ParsedLevel::Info);
        assert_eq!(parse_level("custom line"), ParsedLevel::Unknown);
    }

    #[test]
    fn parse_level_prefers_explicit_level_token_over_message_text() {
        assert_eq!(
            parse_level(r#"2026-03-27T07:51:56Z INFO channel init metadata={"error":"none"}"#),
            ParsedLevel::Info
        );
        assert_eq!(
            parse_level("INFO media_references=[] last_error_count=0"),
            ParsedLevel::Info
        );
    }

    #[test]
    fn parse_level_does_not_treat_substrings_as_levels() {
        assert_eq!(
            parse_level("custom metadata includes error_details but no level"),
            ParsedLevel::Unknown
        );
        assert_eq!(
            parse_level("request severity=warning-ish"),
            ParsedLevel::Unknown
        );
    }

    #[test]
    fn strip_ansi_sequences_removes_terminal_escape_codes() {
        let raw = "\u{1b}[2mwarn\u{1b}[0m plain \u{1b}[31merror\u{1b}[0m";
        let clean = strip_ansi_sequences(raw);
        assert_eq!(clean, "warn plain error");
    }
}
