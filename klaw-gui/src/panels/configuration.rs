use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use egui::{text::LayoutJob, Color32, FontId, TextFormat};
use egui_extras::{Size, StripBuilder};
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
    revision: Option<u64>,
    pending_confirm: Option<ConfirmAction>,
}

impl ConfigurationPanel {
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
        self.revision = Some(snapshot.revision);
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
        const FOOTER_HEIGHT: f32 = 48.0;

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
                ui.label(format!("Revision: {}", this.revision.unwrap_or_default()));
                let dirty = this.is_dirty();
                let dirty_label = if dirty { "Dirty: yes" } else { "Dirty: no" };
                let color = if dirty {
                    Color32::YELLOW
                } else {
                    Color32::LIGHT_GREEN
                };
                ui.colored_label(color, dirty_label);
            });

            ui.separator();

            StripBuilder::new(ui)
                .size(Size::remainder().at_least(MIN_EDITOR_HEIGHT))
                .size(Size::exact(FOOTER_HEIGHT))
                .vertical(|mut strip| {
                    strip.cell(|ui| {
                        let editor_height = ui.available_height();
                        egui::ScrollArea::both()
                            .id_salt("configuration-editor-scroll")
                            .auto_shrink([false, false])
                            .max_height(editor_height)
                            .show(ui, |ui| {
                                let editor_width = ui.available_width();
                                ui.add_sized(
                                    [editor_width, editor_height],
                                    egui::TextEdit::multiline(&mut this.editor_raw)
                                        .font(egui::TextStyle::Monospace)
                                        .desired_rows(26)
                                        .desired_width(f32::INFINITY)
                                        .code_editor()
                                        .layouter(&mut layouter),
                                );
                            });
                    });

                    strip.cell(|ui| {
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("Save").clicked() {
                                this.handle_save(notifications);
                            }
                            if ui.button("Validate").clicked() {
                                this.handle_validate(notifications);
                            }
                            if ui.button("Reset").clicked() {
                                this.request_or_execute(ConfirmAction::Reset, notifications);
                            }
                            if ui.button("Migrate").clicked() {
                                this.request_or_execute(ConfirmAction::Migrate, notifications);
                            }
                            if ui.button("Reload").clicked() {
                                this.try_reload(notifications);
                            }
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
        let mut panel = ConfigurationPanel::default();
        panel.store = Some(store.clone());
        panel.apply_snapshot(store.snapshot());
        let mut notifications = NotificationCenter::default();

        panel.editor_raw = panel.editor_raw.replace("gpt-4o-mini", "gpt-4.1-mini");
        panel.handle_save(&mut notifications);
        assert!(!panel.is_dirty());
        assert!(fs::read_to_string(&path)
            .expect("saved config should be readable")
            .contains("gpt-4.1-mini"));

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
        let mut panel = ConfigurationPanel::default();
        panel.store = Some(store.clone());
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
}
