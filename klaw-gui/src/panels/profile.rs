use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use egui::{text::LayoutJob, Color32, FontId, TextFormat};
use egui_extras::{Size, StripBuilder};
use klaw_util::default_workspace_dir;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const MIN_EDITOR_HEIGHT: f32 = 320.0;
const FOOTER_HEIGHT: f32 = 48.0;
const CARD_MIN_WIDTH: f32 = 320.0;
const CARD_SPACING_X: f32 = 12.0;

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceMarkdownDoc {
    file_name: String,
    path: PathBuf,
    summary: String,
    modified_label: String,
    size_bytes: u64,
}

#[derive(Debug, Clone)]
struct WorkspaceMarkdownEditor {
    file_name: String,
    path: PathBuf,
    original_raw: String,
    editor_raw: String,
    open: bool,
}

#[derive(Default)]
pub struct ProfilePanel {
    workspace_dir: Option<PathBuf>,
    docs: Vec<WorkspaceMarkdownDoc>,
    editor: Option<WorkspaceMarkdownEditor>,
    loaded: bool,
    pending_default_confirm: Option<String>,
}

impl ProfilePanel {
    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        self.reload(notifications);
    }

    fn reload(&mut self, notifications: &mut NotificationCenter) {
        match load_workspace_markdown_docs() {
            Ok((workspace_dir, docs)) => {
                self.workspace_dir = Some(workspace_dir);
                self.docs = docs;
                self.loaded = true;
            }
            Err(err) => {
                notifications.error(format!("Failed to load workspace markdown files: {err}"))
            }
        }
    }

    fn open_editor(&mut self, doc: &WorkspaceMarkdownDoc, notifications: &mut NotificationCenter) {
        match fs::read_to_string(&doc.path) {
            Ok(content) => {
                self.editor = Some(WorkspaceMarkdownEditor {
                    file_name: doc.file_name.clone(),
                    path: doc.path.clone(),
                    original_raw: content.clone(),
                    editor_raw: content,
                    open: true,
                });
            }
            Err(err) => {
                notifications.error(format!("Failed to read {}: {err}", doc.path.display()))
            }
        }
    }

    fn render_doc_card(ui: &mut egui::Ui, doc: &WorkspaceMarkdownDoc) -> bool {
        let mut edit_clicked = false;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_width(320.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.strong(&doc.file_name);
                    ui.add_space(8.0);
                    ui.colored_label(Color32::LIGHT_BLUE, "markdown");
                });
                ui.add_space(4.0);
                ui.label(&doc.summary);
                ui.add_space(6.0);
                ui.small(format!("Modified: {}", doc.modified_label));
                ui.small(format!("Size: {}", format_bytes(doc.size_bytes)));
                ui.small(format!("Path: {}", doc.path.display()));
                ui.add_space(8.0);
                if ui.button("Edit").clicked() {
                    edit_clicked = true;
                }
            });
        });
        edit_clicked
    }

    fn card_column_count(available_width: f32) -> usize {
        let full_card_width = CARD_MIN_WIDTH + CARD_SPACING_X;
        let columns = ((available_width + CARD_SPACING_X) / full_card_width).floor() as usize;
        columns.max(1)
    }

    fn render_editor_window(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        let Some(editor) = self.editor.as_mut() else {
            return;
        };

        let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
            let mut job = markdown_highlight_job(text.as_str());
            job.wrap.max_width = wrap_width;
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        };

        let viewport_height = ctx.input(|input| {
            input
                .viewport()
                .inner_rect
                .map(|rect| rect.height())
                .unwrap_or(760.0)
        });
        let window_max_height = (viewport_height - 96.0).clamp(420.0, 760.0);
        let mut save_clicked = false;
        let mut cancel_clicked = false;
        let mut reset_clicked = false;
        let mut default_clicked = false;
        let dirty = editor.editor_raw != editor.original_raw;
        let mut saved_file_name = None;
        let has_default_template =
            klaw_core::get_default_template_content(&editor.file_name).is_some();

        egui::Window::new(format!("Edit {}", editor.file_name))
            .open(&mut editor.open)
            .resizable(true)
            .default_width(860.0)
            .default_height(window_max_height.min(560.0))
            .max_height(window_max_height)
            .show(ctx, |ui| {
                ui.label(format!("Path: {}", editor.path.display()));
                ui.horizontal(|ui| {
                    let label = if dirty { "Dirty: yes" } else { "Dirty: no" };
                    let color = if dirty {
                        Color32::YELLOW
                    } else {
                        Color32::LIGHT_GREEN
                    };
                    ui.colored_label(color, label);
                    ui.label("Workspace markdown editor");
                });
                ui.separator();

                StripBuilder::new(ui)
                    .size(Size::remainder().at_least(MIN_EDITOR_HEIGHT))
                    .size(Size::exact(FOOTER_HEIGHT))
                    .vertical(|mut strip| {
                        strip.cell(|ui| {
                            let editor_height = ui.available_height();
                            egui::ScrollArea::both()
                                .id_salt(("workspace-markdown-editor", &editor.file_name))
                                .auto_shrink([false, false])
                                .max_height(editor_height)
                                .show(ui, |ui| {
                                    let editor_width = ui.available_width();
                                    ui.add_sized(
                                        [editor_width, editor_height],
                                        egui::TextEdit::multiline(&mut editor.editor_raw)
                                            .font(egui::TextStyle::Monospace)
                                            .desired_rows(24)
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
                                    save_clicked = true;
                                }
                                if ui.button("Cancel").clicked() {
                                    cancel_clicked = true;
                                }
                                if ui.button("Reset").clicked() {
                                    reset_clicked = true;
                                }
                                if has_default_template && ui.button("Default").clicked() {
                                    default_clicked = true;
                                }
                            });
                        });
                    });
            });

        if reset_clicked {
            editor.editor_raw = editor.original_raw.clone();
        }

        if default_clicked {
            self.pending_default_confirm = Some(editor.file_name.clone());
        }

        if save_clicked {
            match fs::write(&editor.path, &editor.editor_raw) {
                Ok(()) => {
                    editor.original_raw = editor.editor_raw.clone();
                    saved_file_name = Some(editor.file_name.clone());
                }
                Err(err) => {
                    notifications.error(format!("Failed to save {}: {err}", editor.path.display()));
                }
            }
        }

        let should_close = cancel_clicked || !editor.open;
        if let Some(file_name) = saved_file_name {
            notifications.success(format!("Saved {file_name}"));
            self.reload(notifications);
        }
        if should_close {
            self.editor = None;
            self.pending_default_confirm = None;
        }
    }

    fn render_default_confirm_dialog(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        let Some(file_name) = self.pending_default_confirm.clone() else {
            return;
        };
        let mut confirmed = false;
        let mut cancelled = false;
        egui::Window::new("Reset to default template")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Reset {} to the built-in default template? This will replace the current editor content.",
                    file_name
                ));
                ui.horizontal(|ui| {
                    if ui.button("Reset to default").clicked() {
                        confirmed = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                    }
                });
            });

        if confirmed {
            if let Some(default_content) = klaw_core::get_default_template_content(&file_name) {
                if let Some(editor) = self.editor.as_mut() {
                    editor.editor_raw = default_content.to_string();
                    notifications.success(format!("Reset {} to default template", file_name));
                }
            }
            self.pending_default_confirm = None;
        }
        if cancelled {
            self.pending_default_confirm = None;
        }
    }
}

impl PanelRenderer for ProfilePanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_loaded(notifications);

        ui.heading(ctx.tab_title);
        match self.workspace_dir.as_deref() {
            Some(workspace_dir) => ui.label(format!("Workspace Path: {}", workspace_dir.display())),
            None => ui.label("Workspace Path: (not loaded)"),
        };
        ui.horizontal(|ui| {
            ui.label(format!("Markdown Files: {}", self.docs.len()));
            if ui.button("Reload").clicked() {
                self.reload(notifications);
            }
        });
        ui.separator();

        let mut edit_target = None;
        egui::ScrollArea::vertical()
            .id_salt("workspace-markdown-card-scroll")
            .show(ui, |ui| {
                if self.docs.is_empty() {
                    ui.label("No markdown files found in the workspace directory.");
                    return;
                }

                let columns = Self::card_column_count(ui.available_width());
                egui::Grid::new("workspace-markdown-card-grid")
                    .num_columns(columns)
                    .spacing([CARD_SPACING_X, 12.0])
                    .show(ui, |ui| {
                        for (index, doc) in self.docs.iter().enumerate() {
                            if Self::render_doc_card(ui, doc) {
                                edit_target = Some(doc.clone());
                            }
                            if (index + 1) % columns == 0 {
                                ui.end_row();
                            }
                        }
                    });
            });

        if let Some(doc) = edit_target {
            self.open_editor(&doc, notifications);
        }
        self.render_editor_window(ui.ctx(), notifications);
        self.render_default_confirm_dialog(ui.ctx(), notifications);
    }
}

fn load_workspace_markdown_docs() -> Result<(PathBuf, Vec<WorkspaceMarkdownDoc>), String> {
    let workspace_dir = resolve_workspace_dir()?;
    fs::create_dir_all(&workspace_dir).map_err(|err| {
        format!(
            "Unable to create workspace dir {}: {err}",
            workspace_dir.display()
        )
    })?;

    let mut docs = Vec::new();
    let entries = fs::read_dir(&workspace_dir).map_err(|err| {
        format!(
            "Unable to read workspace dir {}: {err}",
            workspace_dir.display()
        )
    })?;
    for entry_result in entries {
        let entry = entry_result.map_err(|err| {
            format!(
                "Unable to enumerate workspace dir {}: {err}",
                workspace_dir.display()
            )
        })?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| format!("Unable to inspect {}: {err}", path.display()))?;
        if !file_type.is_file() || !is_markdown_path(&path) {
            continue;
        }

        let metadata = entry
            .metadata()
            .map_err(|err| format!("Unable to read metadata for {}: {err}", path.display()))?;
        let content = fs::read_to_string(&path)
            .map_err(|err| format!("Unable to read {}: {err}", path.display()))?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("Invalid file name for {}", path.display()))?
            .to_string();
        docs.push(WorkspaceMarkdownDoc {
            file_name,
            path,
            summary: summarize_markdown(&content),
            modified_label: format_modified_time(metadata.modified().ok()),
            size_bytes: metadata.len(),
        });
    }

    docs.sort_by(|left, right| left.file_name.cmp(&right.file_name));
    Ok((workspace_dir, docs))
}

fn resolve_workspace_dir() -> Result<PathBuf, String> {
    default_workspace_dir().ok_or_else(|| "HOME is unavailable".to_string())
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

fn summarize_markdown(content: &str) -> String {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("```") {
            continue;
        }
        let candidate = trimmed
            .trim_start_matches('#')
            .trim_start_matches('-')
            .trim_start_matches('*')
            .trim();
        if !candidate.is_empty() {
            return truncate_text(candidate, 96);
        }
    }
    "Empty markdown file.".to_string()
}

fn truncate_text(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (index, ch) in input.chars().enumerate() {
        if index >= max_chars {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

fn format_modified_time(value: Option<std::time::SystemTime>) -> String {
    let Some(value) = value else {
        return "unknown".to_string();
    };
    let Ok(duration) = value.duration_since(UNIX_EPOCH) else {
        return "unknown".to_string();
    };
    format!("{}", duration.as_secs())
}

fn markdown_highlight_job(markdown: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    for line in markdown.split_inclusive('\n') {
        highlight_markdown_line(&mut job, line);
    }
    if markdown.is_empty() {
        append_text(&mut job, "", fmt_md_default());
    }
    job
}

fn highlight_markdown_line(job: &mut LayoutJob, line: &str) {
    let (body, has_newline) = match line.strip_suffix('\n') {
        Some(stripped) => (stripped, true),
        None => (line, false),
    };
    let trimmed = body.trim_start();

    if trimmed.starts_with("```") {
        append_text(job, body, fmt_md_code());
    } else if trimmed.starts_with('#') {
        append_text(job, body, fmt_md_heading());
    } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        append_text(job, body, fmt_md_list());
    } else if trimmed.starts_with('>') {
        append_text(job, body, fmt_md_quote());
    } else {
        highlight_markdown_inline(job, body);
    }

    if has_newline {
        append_text(job, "\n", fmt_md_default());
    }
}

fn highlight_markdown_inline(job: &mut LayoutJob, line: &str) {
    let mut rest = line;
    while let Some(start) = rest.find('`') {
        let (prefix, after_prefix) = rest.split_at(start);
        if !prefix.is_empty() {
            append_text(job, prefix, fmt_md_default());
        }

        let after_tick = &after_prefix[1..];
        if let Some(end) = after_tick.find('`') {
            let code = &after_prefix[..end + 2];
            append_text(job, code, fmt_md_code());
            rest = &after_tick[end + 1..];
        } else {
            append_text(job, after_prefix, fmt_md_default());
            return;
        }
    }

    if !rest.is_empty() {
        append_text(job, rest, fmt_md_default());
    }
}

fn append_text(job: &mut LayoutJob, text: &str, format: TextFormat) {
    job.append(text, 0.0, format);
}

fn fmt_md_default() -> TextFormat {
    TextFormat::simple(FontId::monospace(13.0), Color32::LIGHT_GRAY)
}

fn fmt_md_heading() -> TextFormat {
    TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(132, 197, 255))
}

fn fmt_md_code() -> TextFormat {
    TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(255, 196, 126))
}

fn fmt_md_list() -> TextFormat {
    TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(159, 216, 159))
}

fn fmt_md_quote() -> TextFormat {
    TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(180, 180, 255))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_workspace_dir() -> PathBuf {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("klaw-gui-profile-test-{suffix}"))
    }

    fn load_workspace_markdown_docs_in_dir(
        workspace_dir: &Path,
    ) -> Result<Vec<WorkspaceMarkdownDoc>, String> {
        let mut docs = Vec::new();
        let entries = fs::read_dir(workspace_dir).map_err(|err| {
            format!(
                "Unable to read workspace dir {}: {err}",
                workspace_dir.display()
            )
        })?;
        for entry_result in entries {
            let entry = entry_result.map_err(|err| err.to_string())?;
            let path = entry.path();
            if !entry.file_type().map_err(|err| err.to_string())?.is_file()
                || !is_markdown_path(&path)
            {
                continue;
            }
            let metadata = entry.metadata().map_err(|err| err.to_string())?;
            let content = fs::read_to_string(&path).map_err(|err| err.to_string())?;
            docs.push(WorkspaceMarkdownDoc {
                file_name: path.file_name().unwrap().to_string_lossy().to_string(),
                path,
                summary: summarize_markdown(&content),
                modified_label: format_modified_time(metadata.modified().ok()),
                size_bytes: metadata.len(),
            });
        }
        docs.sort_by(|left, right| left.file_name.cmp(&right.file_name));
        Ok(docs)
    }

    #[test]
    fn summarize_markdown_uses_first_meaningful_line() {
        let summary = summarize_markdown("\n# Title\n\nbody");
        assert_eq!(summary, "Title");
    }

    #[test]
    fn summarize_markdown_skips_empty_and_fence_only_lines() {
        let summary = summarize_markdown("\n```rust\n\n- bullet item");
        assert_eq!(summary, "bullet item");
    }

    #[test]
    fn workspace_doc_loader_filters_non_markdown_files() {
        let workspace_dir = temp_workspace_dir();
        fs::create_dir_all(&workspace_dir).expect("workspace dir");
        fs::write(workspace_dir.join("AGENTS.md"), "# Agents").expect("write markdown");
        fs::write(workspace_dir.join("notes.txt"), "ignore").expect("write txt");

        let docs = load_workspace_markdown_docs_in_dir(&workspace_dir).expect("load docs");

        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].file_name, "AGENTS.md");

        let _ = fs::remove_dir_all(workspace_dir);
    }

    #[test]
    fn truncate_text_adds_ellipsis_when_needed() {
        assert_eq!(truncate_text("abcdef", 3), "abc...");
        assert_eq!(truncate_text("abc", 3), "abc");
    }

    #[test]
    fn card_column_count_respects_available_width() {
        assert_eq!(ProfilePanel::card_column_count(200.0), 1);
        assert_eq!(ProfilePanel::card_column_count(700.0), 2);
        assert_eq!(ProfilePanel::card_column_count(1100.0), 3);
    }
}

fn format_bytes(value: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let raw = value as f64;
    if raw >= GB {
        format!("{:.2} GB", raw / GB)
    } else if raw >= MB {
        format!("{:.2} MB", raw / MB)
    } else if raw >= KB {
        format!("{:.2} KB", raw / KB)
    } else {
        format!("{value} B")
    }
}
