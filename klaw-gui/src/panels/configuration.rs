use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use egui::{
    Color32, FontId, RichText, TextFormat,
    text::{CCursor, CCursorRange, LayoutJob},
};
use egui_extras::{Size, StripBuilder};
use egui_phosphor::regular;
use klaw_config::{ConfigSnapshot, ConfigStore};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfirmAction {
    Reset,
    Migrate,
}

#[derive(Default)]
pub struct ConfigurationPanel {
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    editor_raw: String,
    saved_raw: String,
    search_query: String,
    search_match_index: usize,
    pending_search_range: Option<(usize, usize)>,
    pending_confirm: Option<ConfirmAction>,
}

impl ConfigurationPanel {
    const EDITOR_ID_SALT: &'static str = "configuration-editor";
    const RESET_TEXT_COLOR: Color32 = Color32::from_rgb(200, 150, 50);

    fn ensure_store_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.store.is_some() {
            return;
        }
        match ConfigStore::open(None) {
            Ok(store) => {
                let snapshot = store.snapshot();
                self.store = Some(store);
                self.apply_snapshot(snapshot);
                notifications.success("Configuration loaded from disk");
            }
            Err(err) => {
                notifications.error(format!("Failed to load config: {err}"));
            }
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config_path = Some(snapshot.path);
        self.editor_raw = snapshot.raw_toml.clone();
        self.saved_raw = snapshot.raw_toml;
    }

    fn is_dirty(&self) -> bool {
        self.editor_raw != self.saved_raw
    }

    fn handle_save(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };
        match store.save_raw_toml(&self.editor_raw) {
            Ok(snapshot) => {
                self.apply_snapshot(snapshot);
                notifications.success("Configuration saved");
            }
            Err(err) => {
                notifications.error(format!("Save failed: {err}"));
            }
        }
    }

    fn handle_validate(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };
        match store.validate_raw_toml(&self.editor_raw) {
            Ok(()) => notifications.success("Configuration is valid"),
            Err(err) => notifications.error(format!("Validation failed: {err}")),
        }
    }

    fn request_or_execute(
        &mut self,
        action: ConfirmAction,
        notifications: &mut NotificationCenter,
    ) {
        if self.is_dirty() {
            self.pending_confirm = Some(action);
            return;
        }
        self.execute_action(action, notifications);
    }

    fn confirm_pending_action(&mut self, notifications: &mut NotificationCenter) {
        let Some(action) = self.pending_confirm.take() else {
            return;
        };
        self.execute_action(action, notifications);
    }

    fn execute_action(&mut self, action: ConfirmAction, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };
        let result = match action {
            ConfirmAction::Reset => store.reset_to_defaults(),
            ConfirmAction::Migrate => store.migrate_with_defaults(),
        };
        match result {
            Ok(snapshot) => {
                self.apply_snapshot(snapshot);
                notifications.success(match action {
                    ConfirmAction::Reset => "Configuration reset to defaults",
                    ConfirmAction::Migrate => "Configuration migrated with defaults",
                });
            }
            Err(err) => {
                notifications.error(format!("Operation failed: {err}"));
            }
        }
    }

    fn try_reload(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };
        match store.reload() {
            Ok(snapshot) => {
                self.apply_snapshot(snapshot);
                notifications.success("Configuration reloaded from disk");
            }
            Err(err) => {
                notifications.error(format!("Reload failed: {err}"));
            }
        }
    }

    fn status_label(path: Option<&Path>) -> String {
        match path {
            Some(path) => format!("Path: {}", path.display()),
            None => "Path: (not loaded)".to_string(),
        }
    }

    fn find_matches(text: &str, query: &str) -> Vec<(usize, usize)> {
        if query.is_empty() {
            return Vec::new();
        }

        text.match_indices(query)
            .map(|(byte_start, matched)| {
                let start = text[..byte_start].chars().count();
                let end = start + matched.chars().count();
                (start, end)
            })
            .collect()
    }

    fn search_matches(&self) -> Vec<(usize, usize)> {
        Self::find_matches(&self.editor_raw, self.search_query.trim())
    }

    fn set_search_match(&mut self, matches: &[(usize, usize)], index: usize) {
        if matches.is_empty() {
            self.search_match_index = 0;
            self.pending_search_range = None;
            return;
        }

        let index = index % matches.len();
        self.search_match_index = index;
        self.pending_search_range = Some(matches[index]);
    }

    fn sync_search_with_query(&mut self) {
        let matches = self.search_matches();
        self.pending_search_range = None;
        if matches.is_empty() {
            self.search_match_index = 0;
            return;
        }

        self.search_match_index = self.search_match_index.min(matches.len() - 1);
    }

    fn jump_to_next_search_match(&mut self, notifications: &mut NotificationCenter) {
        let matches = self.search_matches();
        if matches.is_empty() {
            if !self.search_query.trim().is_empty() {
                notifications.error("No matches found in configuration");
            }
            return;
        }

        let next_index = (self.search_match_index + 1) % matches.len();
        self.set_search_match(&matches, next_index);
    }

    fn jump_to_first_search_match(&mut self, notifications: &mut NotificationCenter) {
        let matches = self.search_matches();
        if matches.is_empty() {
            if !self.search_query.trim().is_empty() {
                notifications.error("No matches found in configuration");
            }
            return;
        }

        self.set_search_match(&matches, 0);
    }

    fn jump_to_previous_search_match(&mut self, notifications: &mut NotificationCenter) {
        let matches = self.search_matches();
        if matches.is_empty() {
            if !self.search_query.trim().is_empty() {
                notifications.error("No matches found in configuration");
            }
            return;
        }

        let previous_index = self
            .search_match_index
            .checked_sub(1)
            .unwrap_or(matches.len() - 1);
        self.set_search_match(&matches, previous_index);
    }

    fn syntax_highlight_job(code: &str) -> LayoutJob {
        let mut job = LayoutJob::default();
        for line in code.split_inclusive('\n') {
            Self::highlight_line(&mut job, line);
        }
        if code.is_empty() {
            Self::append(&mut job, "", Self::fmt_default());
        }
        job
    }

    fn highlight_line(job: &mut LayoutJob, line: &str) {
        let (body, has_newline) = match line.strip_suffix('\n') {
            Some(stripped) => (stripped, true),
            None => (line, false),
        };
        let trimmed = body.trim_start();
        if trimmed.starts_with('#') {
            Self::append(job, body, Self::fmt_comment());
        } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let indent_len = body.len() - trimmed.len();
            let indent = &body[..indent_len];
            if !indent.is_empty() {
                Self::append(job, indent, Self::fmt_default());
            }
            Self::append(job, trimmed, Self::fmt_section());
        } else if let Some(eq_idx) = body.find('=') {
            let key_segment = &body[..eq_idx];
            let value_segment = &body[eq_idx + 1..];
            Self::highlight_key_segment(job, key_segment);
            Self::append(job, "=", Self::fmt_operator());
            Self::highlight_value_and_comment(job, value_segment);
        } else {
            Self::append(job, body, Self::fmt_default());
        }
        if has_newline {
            Self::append(job, "\n", Self::fmt_default());
        }
    }

    fn highlight_key_segment(job: &mut LayoutJob, key_segment: &str) {
        let trimmed_start = key_segment.trim_start();
        let leading_len = key_segment.len() - trimmed_start.len();
        let (key_name, trailing) = match trimmed_start.trim_end().is_empty() {
            true => (trimmed_start, ""),
            false => {
                let trimmed_end = trimmed_start.trim_end();
                let trailing_len = trimmed_start.len() - trimmed_end.len();
                (
                    trimmed_end,
                    &trimmed_start[trimmed_start.len() - trailing_len..],
                )
            }
        };
        if leading_len > 0 {
            Self::append(job, &key_segment[..leading_len], Self::fmt_default());
        }
        Self::append(job, key_name, Self::fmt_key());
        if !trailing.is_empty() {
            Self::append(job, trailing, Self::fmt_default());
        }
    }

    fn highlight_value_and_comment(job: &mut LayoutJob, value_segment: &str) {
        let Some(comment_start) = Self::find_comment_start(value_segment) else {
            Self::highlight_value_segment(job, value_segment);
            return;
        };
        let value = &value_segment[..comment_start];
        let comment = &value_segment[comment_start..];
        Self::highlight_value_segment(job, value);
        Self::append(job, comment, Self::fmt_comment());
    }

    fn highlight_value_segment(job: &mut LayoutJob, value: &str) {
        let bytes_len = value.len();
        let mut idx = 0;
        while idx < bytes_len {
            let ch = value[idx..].chars().next().unwrap_or_default();
            if ch.is_whitespace() {
                let start = idx;
                idx += ch.len_utf8();
                while idx < bytes_len {
                    let next = value[idx..].chars().next().unwrap_or_default();
                    if !next.is_whitespace() {
                        break;
                    }
                    idx += next.len_utf8();
                }
                Self::append(job, &value[start..idx], Self::fmt_default());
                continue;
            }
            if ch == '"' || ch == '\'' {
                let start = idx;
                let quote = ch;
                idx += ch.len_utf8();
                let mut escaped = false;
                while idx < bytes_len {
                    let next = value[idx..].chars().next().unwrap_or_default();
                    idx += next.len_utf8();
                    if quote == '"' && next == '\\' && !escaped {
                        escaped = true;
                        continue;
                    }
                    if next == quote && !escaped {
                        break;
                    }
                    escaped = false;
                }
                Self::append(job, &value[start..idx], Self::fmt_string());
                continue;
            }
            if ch.is_ascii_digit() || ch == '-' {
                let start = idx;
                idx += ch.len_utf8();
                while idx < bytes_len {
                    let next = value[idx..].chars().next().unwrap_or_default();
                    if !(next.is_ascii_digit() || matches!(next, '.' | '_' | 'e' | 'E' | '+' | '-'))
                    {
                        break;
                    }
                    idx += next.len_utf8();
                }
                Self::append(job, &value[start..idx], Self::fmt_number());
                continue;
            }
            if ch.is_ascii_alphabetic() {
                let start = idx;
                idx += ch.len_utf8();
                while idx < bytes_len {
                    let next = value[idx..].chars().next().unwrap_or_default();
                    if !(next.is_ascii_alphanumeric() || next == '_') {
                        break;
                    }
                    idx += next.len_utf8();
                }
                let token = &value[start..idx];
                let fmt = if token == "true" || token == "false" {
                    Self::fmt_bool()
                } else {
                    Self::fmt_default()
                };
                Self::append(job, token, fmt);
                continue;
            }
            let next_idx = idx + ch.len_utf8();
            Self::append(job, &value[idx..next_idx], Self::fmt_default());
            idx = next_idx;
        }
    }

    fn find_comment_start(value: &str) -> Option<usize> {
        let mut in_double_quotes = false;
        let mut in_single_quotes = false;
        let mut escaped = false;
        for (idx, ch) in value.char_indices() {
            if ch == '"' && !in_single_quotes && !escaped {
                in_double_quotes = !in_double_quotes;
            } else if ch == '\'' && !in_double_quotes && !escaped {
                in_single_quotes = !in_single_quotes;
            } else if ch == '#' && !in_double_quotes && !in_single_quotes {
                return Some(idx);
            }
            escaped = ch == '\\' && in_double_quotes && !escaped;
        }
        None
    }

    fn append(job: &mut LayoutJob, text: &str, format: TextFormat) {
        job.append(text, 0.0, format);
    }

    fn fmt_default() -> TextFormat {
        TextFormat::simple(FontId::monospace(13.0), Color32::LIGHT_GRAY)
    }

    fn fmt_comment() -> TextFormat {
        TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(120, 145, 95))
    }

    fn fmt_section() -> TextFormat {
        TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(125, 174, 241))
    }

    fn fmt_key() -> TextFormat {
        TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(242, 201, 76))
    }

    fn fmt_operator() -> TextFormat {
        TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(210, 210, 210))
    }

    fn fmt_string() -> TextFormat {
        TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(158, 212, 158))
    }

    fn fmt_number() -> TextFormat {
        TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(255, 170, 120))
    }

    fn fmt_bool() -> TextFormat {
        TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(214, 154, 255))
    }
}

impl PanelRenderer for ConfigurationPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        const MIN_TOTAL_HEIGHT: f32 = 520.0;
        const MIN_EDITOR_HEIGHT: f32 = 320.0;

        self.ensure_store_loaded(notifications);

        let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
            let mut job = Self::syntax_highlight_job(text.as_str());
            job.wrap.max_width = wrap_width;
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        };

        let mut render_strip = |ui: &mut egui::Ui, this: &mut ConfigurationPanel| {
            ui.heading(ctx.tab_title);
            ui.label(Self::status_label(this.config_path.as_deref()));
            ui.horizontal(|ui| {
                let dirty = this.is_dirty();
                let dirty_label = if dirty { "Dirty: yes" } else { "Dirty: no" };
                let color = if dirty {
                    Color32::YELLOW
                } else {
                    Color32::LIGHT_GREEN
                };
                ui.colored_label(color, dirty_label);
            });
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(format!("{} Save", regular::FLOPPY_DISK))
                    .clicked()
                {
                    this.handle_save(notifications);
                }
                if ui.button(format!("{} Validate", regular::CHECK)).clicked() {
                    this.handle_validate(notifications);
                }
                if ui
                    .button(
                        RichText::new(format!("{} Reset", regular::ARROW_COUNTER_CLOCKWISE))
                            .color(Self::RESET_TEXT_COLOR),
                    )
                    .clicked()
                {
                    this.request_or_execute(ConfirmAction::Reset, notifications);
                }
                if ui.button(format!("{} Migrate", regular::BROOM)).clicked() {
                    this.request_or_execute(ConfirmAction::Migrate, notifications);
                }
                if ui
                    .button(format!("{} Reload", regular::ARROW_CLOCKWISE))
                    .clicked()
                {
                    this.try_reload(notifications);
                }
            });
            ui.horizontal(|ui| {
                ui.label(format!("{} Find", regular::MAGNIFYING_GLASS));
                let search_response = ui.add(
                    egui::TextEdit::singleline(&mut this.search_query)
                        .desired_width(220.0)
                        .hint_text("Search TOML"),
                );
                if search_response.changed() {
                    this.sync_search_with_query();
                }
                if search_response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    this.jump_to_first_search_match(notifications);
                }

                let matches = this.search_matches();
                let has_matches = !matches.is_empty();
                if has_matches && this.search_match_index >= matches.len() {
                    this.search_match_index = matches.len() - 1;
                } else if !has_matches {
                    this.search_match_index = 0;
                }
                let status_text = if this.search_query.trim().is_empty() {
                    "Type to search".to_string()
                } else if has_matches {
                    format!("{}/{}", this.search_match_index + 1, matches.len())
                } else {
                    "0 matches".to_string()
                };
                let status_color = if this.search_query.trim().is_empty() {
                    ui.visuals().weak_text_color()
                } else if has_matches {
                    Color32::LIGHT_GREEN
                } else {
                    Self::RESET_TEXT_COLOR
                };
                ui.label(RichText::new(status_text).color(status_color));

                if ui
                    .add_enabled(
                        has_matches,
                        egui::Button::new(format!("{} Prev", regular::ARROW_UP)),
                    )
                    .clicked()
                {
                    this.jump_to_previous_search_match(notifications);
                }
                if ui
                    .add_enabled(
                        has_matches,
                        egui::Button::new(format!("{} Next", regular::ARROW_DOWN)),
                    )
                    .clicked()
                {
                    this.jump_to_next_search_match(notifications);
                }
            });

            ui.separator();

            StripBuilder::new(ui)
                .size(Size::remainder().at_least(MIN_EDITOR_HEIGHT))
                .vertical(|mut strip| {
                    strip.cell(|ui| {
                        let editor_height = ui.available_height();
                        let editor_id = ui.make_persistent_id(Self::EDITOR_ID_SALT);
                        if let Some((start, end)) = this.pending_search_range.take() {
                            let mut state =
                                egui::TextEdit::load_state(ui.ctx(), editor_id).unwrap_or_default();
                            state.cursor.set_char_range(Some(CCursorRange::two(
                                CCursor::new(start),
                                CCursor::new(end),
                            )));
                            egui::TextEdit::store_state(ui.ctx(), editor_id, state);
                            ui.memory_mut(|mem| mem.request_focus(editor_id));
                        }
                        egui::ScrollArea::both()
                            .id_salt("configuration-editor-scroll")
                            .auto_shrink([false, false])
                            .max_height(editor_height)
                            .show(ui, |ui| {
                                let editor_width = ui.available_width();
                                egui::TextEdit::multiline(&mut this.editor_raw)
                                    .id(editor_id)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_rows(26)
                                    .desired_width(f32::INFINITY)
                                    .min_size(egui::vec2(editor_width, editor_height))
                                    .code_editor()
                                    .layouter(&mut layouter)
                                    .show(ui);
                            });
                    });
                });
        };

        let parent_height = ui.available_height();
        if parent_height < MIN_TOTAL_HEIGHT {
            egui::ScrollArea::vertical()
                .id_salt("configuration-strip-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_height(MIN_TOTAL_HEIGHT);
                    render_strip(ui, self);
                });
        } else {
            render_strip(ui, self);
        }

        if let Some(action) = self.pending_confirm {
            egui::Window::new("Unsaved changes")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label("Current edits are not saved. Continue and overwrite editor content?");
                    ui.horizontal(|ui| {
                        if ui.button("Continue").clicked() {
                            self.pending_confirm = Some(action);
                            self.confirm_pending_action(notifications);
                        }
                        if ui.button("Cancel").clicked() {
                            self.pending_confirm = None;
                        }
                    });
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::NotificationCenter;
    use std::{
        env, fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn write_test_config(path: &Path, model: &str) {
        let raw = format!(
            r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "{model}"
env_key = "OPENAI_API_KEY"
"#
        );
        fs::write(path, raw).expect("should write config");
    }

    #[test]
    fn dirty_state_tracks_editor_changes() {
        let snapshot = ConfigSnapshot {
            path: PathBuf::from("/tmp/config.toml"),
            config: klaw_config::AppConfig::default(),
            raw_toml: "model_provider = \"openai\"\n".to_string(),
            revision: 1,
        };
        let mut panel = ConfigurationPanel::default();
        panel.apply_snapshot(snapshot);
        assert!(!panel.is_dirty());
        panel.editor_raw.push_str("# change\n");
        assert!(panel.is_dirty());
    }

    #[test]
    fn save_success_and_failure_behaviors() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = env::temp_dir().join(format!("klaw-gui-config-panel-test-{suffix}"));
        let path = root.join("config.toml");
        fs::create_dir_all(&root).expect("should create temp root");
        write_test_config(&path, "gpt-4o-mini");

        let store = ConfigStore::open(Some(&path)).expect("store should open");
        let mut panel = ConfigurationPanel {
            store: Some(store.clone()),
            ..Default::default()
        };
        panel.apply_snapshot(store.snapshot());
        let mut notifications = NotificationCenter::default();

        panel.editor_raw = panel.editor_raw.replace("gpt-4o-mini", "gpt-4.1-mini");
        panel.handle_save(&mut notifications);
        assert!(!panel.is_dirty());
        assert!(
            fs::read_to_string(&path)
                .expect("saved config should be readable")
                .contains("gpt-4.1-mini")
        );

        panel.editor_raw = "[broken".to_string();
        panel.handle_save(&mut notifications);
        assert!(panel.is_dirty());

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reset_and_migrate_require_confirmation_when_dirty() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = env::temp_dir().join(format!("klaw-gui-config-panel-test-{suffix}"));
        let path = root.join("config.toml");
        fs::create_dir_all(&root).expect("should create temp root");
        write_test_config(&path, "gpt-4.1-mini");

        let store = ConfigStore::open(Some(&path)).expect("store should open");
        let mut panel = ConfigurationPanel {
            store: Some(store.clone()),
            ..Default::default()
        };
        panel.apply_snapshot(store.snapshot());
        let mut notifications = NotificationCenter::default();

        panel.editor_raw.push_str("\n# unsaved");
        panel.request_or_execute(ConfirmAction::Reset, &mut notifications);
        assert_eq!(panel.pending_confirm, Some(ConfirmAction::Reset));

        panel.confirm_pending_action(&mut notifications);
        assert!(panel.pending_confirm.is_none());
        assert!(!panel.is_dirty());
        assert!(
            panel.editor_raw.contains("gpt-4o-mini"),
            "reset should restore defaults"
        );

        panel.editor_raw.push_str("\n# unsaved");
        panel.request_or_execute(ConfirmAction::Migrate, &mut notifications);
        assert_eq!(panel.pending_confirm, Some(ConfirmAction::Migrate));
        panel.confirm_pending_action(&mut notifications);
        assert!(panel.pending_confirm.is_none());
        assert!(!panel.is_dirty());

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn syntax_highlight_handles_sections_keys_values_and_comments() {
        let code = r#"# comment
[storage]
root_dir = "/tmp/klaw" # inline
enabled = true
limit = 42
"#;
        let job = ConfigurationPanel::syntax_highlight_job(code);
        let text = job
            .sections
            .iter()
            .map(|section| {
                let start = section.byte_range.start;
                let end = section.byte_range.end;
                &job.text[start..end]
            })
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(text, code);
        assert!(ConfigurationPanel::find_comment_start(r##""#not-comment" # comment"##).is_some());
    }

    #[test]
    fn search_matches_return_character_ranges() {
        let matches = ConfigurationPanel::find_matches("aβc aβc", "βc");
        assert_eq!(matches, vec![(1, 3), (5, 7)]);
        assert!(ConfigurationPanel::find_matches("abc", "").is_empty());
    }

    #[test]
    fn sync_search_with_query_does_not_schedule_editor_jump() {
        let mut panel = ConfigurationPanel {
            editor_raw: "alpha\nbeta\nalpha\n".to_string(),
            search_query: "alpha".to_string(),
            search_match_index: 0,
            pending_search_range: Some((0, 5)),
            ..Default::default()
        };

        panel.sync_search_with_query();

        assert_eq!(panel.search_match_index, 0);
        assert!(panel.pending_search_range.is_none());
    }

    #[test]
    fn enter_confirmation_targets_first_search_match() {
        let mut panel = ConfigurationPanel {
            editor_raw: "alpha\nbeta\nalpha\n".to_string(),
            search_query: "alpha".to_string(),
            search_match_index: 1,
            ..Default::default()
        };
        let mut notifications = NotificationCenter::default();

        panel.jump_to_first_search_match(&mut notifications);

        assert_eq!(panel.search_match_index, 0);
        assert_eq!(panel.pending_search_range, Some((0, 5)));
    }
}
