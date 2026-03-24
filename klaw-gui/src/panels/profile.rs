use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use egui::{Color32, FontId, RichText, TextFormat, text::LayoutJob};
use egui_extras::{Column, Size, StripBuilder, TableBuilder};
use egui_phosphor::regular;
use klaw_config::{AppConfig, ConfigStore};
use klaw_core::{SkillPromptEntry, build_runtime_system_prompt};
use klaw_skill::{
    FileSystemSkillStore, InstalledSkill, RegistrySource, ReqwestSkillFetcher, SkillSourceKind,
    SkillsManager, open_default_skills_manager,
};
use klaw_util::default_workspace_dir;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::UNIX_EPOCH;
use tokio::runtime::Builder;

const MIN_EDITOR_HEIGHT: f32 = 320.0;
const FOOTER_HEIGHT: f32 = 48.0;
const DOCS_SECTION_MIN_HEIGHT: f32 = 180.0;
const DOCS_SECTION_MAX_HEIGHT: f32 = 320.0;
const SYSTEM_PROMPT_PREVIEW_MIN_HEIGHT: f32 = 260.0;

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

#[derive(Debug, Clone, Default)]
struct WorkspaceMarkdownPreview {
    file_name: String,
    path: PathBuf,
    content: String,
    open: bool,
}

#[derive(Debug, Clone, Default)]
struct WorkspaceFileCreateForm {
    file_name: String,
    body: String,
    open: bool,
}

#[derive(Default)]
pub struct ProfilePanel {
    workspace_dir: Option<PathBuf>,
    docs: Vec<WorkspaceMarkdownDoc>,
    selected_doc: Option<String>,
    system_prompt_preview: String,
    editor: Option<WorkspaceMarkdownEditor>,
    preview: Option<WorkspaceMarkdownPreview>,
    create_form: WorkspaceFileCreateForm,
    loaded: bool,
    pending_default_confirm: Option<String>,
    pending_delete_doc: Option<WorkspaceMarkdownDoc>,
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
                if let Some(selected_doc) = self.selected_doc.as_deref()
                    && !self.docs.iter().any(|doc| doc.file_name == selected_doc)
                {
                    self.selected_doc = None;
                }
                self.system_prompt_preview = load_system_prompt_preview();
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

    fn open_preview(&mut self, doc: &WorkspaceMarkdownDoc, notifications: &mut NotificationCenter) {
        match fs::read_to_string(&doc.path) {
            Ok(content) => {
                self.preview = Some(WorkspaceMarkdownPreview {
                    file_name: doc.file_name.clone(),
                    path: doc.path.clone(),
                    content,
                    open: true,
                });
            }
            Err(err) => {
                notifications.error(format!("Failed to read {}: {err}", doc.path.display()))
            }
        }
    }

    fn render_docs_section(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        let mut edit_target = None;
        let mut preview_target = None;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Workspace Markdown Files");
                ui.label(format!("({})", self.docs.len()));
            });
            ui.add_space(6.0);

            if self.docs.is_empty() {
                ui.label("No markdown files found in the workspace directory.");
                return;
            }

            let available_height = ui.available_height();
            TableBuilder::new(ui)
                .striped(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::auto().at_least(140.0))
                .column(Column::remainder().at_least(220.0))
                .column(Column::auto().at_least(90.0))
                .column(Column::auto().at_least(110.0))
                .column(Column::remainder().at_least(260.0))
                .min_scrolled_height(0.0)
                .max_scroll_height(available_height)
                .sense(egui::Sense::click())
                .header(22.0, |mut header| {
                    header.col(|ui| {
                        ui.strong("Name");
                    });
                    header.col(|ui| {
                        ui.strong("Summary");
                    });
                    header.col(|ui| {
                        ui.strong("Size");
                    });
                    header.col(|ui| {
                        ui.strong("Modified");
                    });
                    header.col(|ui| {
                        ui.strong("Path");
                    });
                })
                .body(|body| {
                    body.rows(22.0, self.docs.len(), |mut row| {
                        let idx = row.index();
                        let doc = &self.docs[idx];
                        let is_selected = self.selected_doc.as_deref() == Some(&doc.file_name);
                        row.set_selected(is_selected);

                        row.col(|ui| {
                            ui.label(&doc.file_name);
                        });
                        row.col(|ui| {
                            ui.label(&doc.summary);
                        });
                        row.col(|ui| {
                            ui.label(format_bytes(doc.size_bytes));
                        });
                        row.col(|ui| {
                            ui.label(&doc.modified_label);
                        });
                        row.col(|ui| {
                            ui.label(doc.path.display().to_string());
                        });

                        let response = row.response();
                        if response.clicked() {
                            self.selected_doc = if is_selected {
                                None
                            } else {
                                Some(doc.file_name.clone())
                            };
                        }

                        let doc_clone = doc.clone();
                        response.context_menu(|ui| {
                            if ui.button(format!("{} Preview", regular::EYE)).clicked() {
                                preview_target = Some(doc_clone.clone());
                                ui.close();
                            }
                            if ui
                                .button(format!("{} Edit", regular::PENCIL_SIMPLE))
                                .clicked()
                            {
                                edit_target = Some(doc_clone.clone());
                                ui.close();
                            }
                            ui.separator();
                            if ui
                                .add(egui::Button::new(
                                    RichText::new(format!("{} Delete", regular::TRASH))
                                        .color(egui::Color32::RED),
                                ))
                                .clicked()
                            {
                                self.pending_delete_doc = Some(doc_clone.clone());
                                ui.close();
                            }
                        });
                    });
                });
        });

        if let Some(doc) = edit_target {
            self.open_editor(&doc, notifications);
        }
        if let Some(doc) = preview_target {
            self.open_preview(&doc, notifications);
        }
    }

    fn render_system_prompt_preview(&self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.strong("System Prompt Preview");
            ui.small("Rendered from current workspace prompt docs and installed skills.");
            ui.add_space(6.0);

            egui::ScrollArea::vertical()
                .id_salt("system-prompt-preview-scroll")
                .max_height(ui.available_height())
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    render_markdown(ui, &self.system_prompt_preview);
                });
        });
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

    fn render_create_file_dialog(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        if !self.create_form.open {
            return;
        }

        let Some(workspace_dir) = self.workspace_dir.clone() else {
            notifications.error("Workspace path is unavailable.");
            self.create_form.open = false;
            return;
        };

        let viewport_height = ctx.input(|input| {
            input
                .viewport()
                .inner_rect
                .map(|rect| rect.height())
                .unwrap_or(760.0)
        });
        let window_max_height = (viewport_height - 120.0).clamp(360.0, 680.0);
        let mut create_clicked = false;
        let mut cancel_clicked = false;

        egui::Window::new("Create workspace file")
            .open(&mut self.create_form.open)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .default_width(720.0)
            .default_height(window_max_height.min(480.0))
            .max_height(window_max_height)
            .show(ctx, |ui| {
                ui.label(format!("Workspace Path: {}", workspace_dir.display()));
                ui.small("The file will be created directly under the workspace directory.");
                ui.add_space(8.0);

                ui.label("File Name");
                ui.add(
                    egui::TextEdit::singleline(&mut self.create_form.file_name)
                        .desired_width(f32::INFINITY)
                        .hint_text("example.md"),
                );
                ui.add_space(8.0);

                ui.label("Body");
                egui::ScrollArea::vertical()
                    .id_salt("workspace-create-body-scroll")
                    .max_height((window_max_height - 180.0).max(180.0))
                    .show(ui, |ui| {
                        ui.add_sized(
                            [ui.available_width(), ui.available_height().max(200.0)],
                            egui::TextEdit::multiline(&mut self.create_form.body)
                                .font(egui::TextStyle::Monospace)
                                .desired_rows(16)
                                .desired_width(f32::INFINITY)
                                .code_editor(),
                        );
                    });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Create").clicked() {
                        create_clicked = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel_clicked = true;
                    }
                });
            });

        if create_clicked {
            match create_workspace_file(
                &workspace_dir,
                &self.create_form.file_name,
                &self.create_form.body,
            ) {
                Ok(path) => {
                    notifications.success(format!("Created {}", path.display()));
                    self.reload(notifications);
                    self.create_form = WorkspaceFileCreateForm::default();
                }
                Err(err) => notifications.error(err),
            }
        }

        if cancel_clicked || !self.create_form.open {
            self.create_form = WorkspaceFileCreateForm::default();
        }
    }

    fn render_preview_window(&mut self, ctx: &egui::Context) {
        let Some(preview) = self.preview.as_mut() else {
            return;
        };

        egui::Window::new(format!("Preview {}", preview.file_name))
            .open(&mut preview.open)
            .resizable(true)
            .default_width(860.0)
            .default_height(620.0)
            .show(ctx, |ui| {
                ui.label(format!("Path: {}", preview.path.display()));
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt(("workspace-markdown-preview", &preview.file_name))
                    .auto_shrink([false, false])
                    .show(ui, |ui| render_markdown(ui, &preview.content));
            });

        if self.preview.as_ref().is_some_and(|preview| !preview.open) {
            self.preview = None;
        }
    }

    fn render_delete_confirm_dialog(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        let Some(doc) = self.pending_delete_doc.clone() else {
            return;
        };

        let mut confirmed = false;
        let mut cancelled = false;
        egui::Window::new("Delete workspace file")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("Delete {} ?", doc.file_name));
                ui.small(format!("Path: {}", doc.path.display()));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(
                            RichText::new(format!("{} Delete", regular::TRASH))
                                .color(egui::Color32::RED),
                        ))
                        .clicked()
                    {
                        confirmed = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                    }
                });
            });

        if confirmed {
            match fs::remove_file(&doc.path) {
                Ok(()) => {
                    if self.selected_doc.as_deref() == Some(&doc.file_name) {
                        self.selected_doc = None;
                    }
                    if self
                        .preview
                        .as_ref()
                        .is_some_and(|preview| preview.path == doc.path)
                    {
                        self.preview = None;
                    }
                    notifications.success(format!("Deleted {}", doc.file_name));
                    self.reload(notifications);
                }
                Err(err) => {
                    notifications.error(format!("Failed to delete {}: {err}", doc.path.display()));
                }
            }
            self.pending_delete_doc = None;
        }

        if cancelled {
            self.pending_delete_doc = None;
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
            if ui.button("Create File").clicked() {
                self.create_form.open = true;
            }
        });
        ui.separator();
        let docs_height =
            (ui.available_height() * 0.38).clamp(DOCS_SECTION_MIN_HEIGHT, DOCS_SECTION_MAX_HEIGHT);
        StripBuilder::new(ui)
            .size(Size::exact(docs_height))
            .size(Size::remainder().at_least(SYSTEM_PROMPT_PREVIEW_MIN_HEIGHT))
            .vertical(|mut strip| {
                strip.cell(|ui| self.render_docs_section(ui, notifications));
                strip.cell(|ui| self.render_system_prompt_preview(ui));
            });

        self.render_editor_window(ui.ctx(), notifications);
        self.render_default_confirm_dialog(ui.ctx(), notifications);
        self.render_create_file_dialog(ui.ctx(), notifications);
        self.render_preview_window(ui.ctx());
        self.render_delete_confirm_dialog(ui.ctx(), notifications);
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

fn validate_workspace_file_name(file_name: &str) -> Result<String, String> {
    use std::path::Component;

    let trimmed = file_name.trim();
    if trimmed.is_empty() {
        return Err("File name is required.".to_string());
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Err("File name must be relative to the workspace directory.".to_string());
    }

    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(trimmed.to_string()),
        _ => Err("File name must not contain directory separators.".to_string()),
    }
}

fn create_workspace_file(
    workspace_dir: &Path,
    file_name: &str,
    body: &str,
) -> Result<PathBuf, String> {
    let file_name = validate_workspace_file_name(file_name)?;
    fs::create_dir_all(workspace_dir).map_err(|err| {
        format!(
            "Unable to create workspace dir {}: {err}",
            workspace_dir.display()
        )
    })?;

    let path = workspace_dir.join(&file_name);
    if path.exists() {
        return Err(format!("{} already exists.", path.display()));
    }

    fs::write(&path, body).map_err(|err| format!("Failed to create {}: {err}", path.display()))?;
    Ok(path)
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

fn load_system_prompt_preview() -> String {
    let config = match ConfigStore::open(None) {
        Ok(store) => store.snapshot().config,
        Err(err) => {
            return format!(
                "# System Prompt Preview Unavailable\n\nFailed to load config.toml: {err}"
            );
        }
    };

    let skills = load_runtime_skill_prompt_entries(config).unwrap_or_default();
    build_runtime_system_prompt(skills).unwrap_or_else(|| {
        "# System Prompt Preview Unavailable\n\nFailed to assemble the runtime system prompt."
            .to_string()
    })
}

fn load_runtime_skill_prompt_entries(config: AppConfig) -> Result<Vec<SkillPromptEntry>, String> {
    let join = thread::spawn(move || {
        let store = open_default_skills_manager()
            .map_err(|err| format!("failed to open skills manager: {err}"))?;
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("failed to build runtime: {err}"))?;
        runtime.block_on(async move {
            sync_registry_installed_skills(&store, &config).await;

            let skills = store
                .load_all_installed_skill_markdowns()
                .await
                .map_err(|err| format!("failed to load installed skills: {err}"))?;
            Ok(skills
                .into_iter()
                .map(|skill| SkillPromptEntry {
                    name: skill.name,
                    path: skill.local_path.display().to_string(),
                    description: extract_skill_short_description(&skill.content),
                    source: format_skill_source(
                        &skill.source_kind,
                        skill.registry.as_deref(),
                        skill.stale,
                    ),
                })
                .collect())
        })
    });

    match join.join() {
        Ok(result) => result,
        Err(_) => Err("skill prompt loader thread panicked".to_string()),
    }
}

async fn sync_registry_installed_skills(
    store: &FileSystemSkillStore<ReqwestSkillFetcher>,
    config: &AppConfig,
) {
    let sources: Vec<RegistrySource> = config
        .skills
        .registries
        .iter()
        .map(|(name, registry)| RegistrySource {
            name: name.clone(),
            address: registry.address.clone(),
        })
        .collect();
    let installed: Vec<InstalledSkill> = config
        .skills
        .registries
        .iter()
        .flat_map(|(registry_name, registry)| {
            registry.installed.iter().map(|skill_name| InstalledSkill {
                registry: registry_name.clone(),
                name: skill_name.clone(),
            })
        })
        .collect();

    let _ = store
        .sync_registry_installed_skills(&sources, &installed, config.skills.sync_timeout)
        .await;
}

fn extract_skill_short_description(markdown: &str) -> String {
    const MAX_LEN: usize = 180;
    extract_skill_frontmatter_description(markdown)
        .or_else(|| extract_skill_body_description(markdown))
        .map(|description| truncate_skill_description(&description, MAX_LEN))
        .unwrap_or_else(|| "no description".to_string())
}

fn extract_skill_frontmatter_description(markdown: &str) -> Option<String> {
    frontmatter_lines(markdown)?
        .find_map(|line| line.trim().strip_prefix("description:").map(str::trim))
        .filter(|value| !value.is_empty())
        .map(trim_matching_quotes)
        .map(str::to_string)
}

fn extract_skill_body_description(markdown: &str) -> Option<String> {
    let body = strip_frontmatter(markdown).unwrap_or(markdown);
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#') && *line != "---")
        .map(str::to_string)
}

fn strip_frontmatter(markdown: &str) -> Option<&str> {
    let mut lines = markdown.lines();
    if lines.next().map(str::trim) != Some("---") {
        return None;
    }

    let mut offset = markdown.find('\n')? + 1;
    for line in lines {
        let line_end = offset + line.len();
        let next_offset = if markdown.as_bytes().get(line_end) == Some(&b'\n') {
            line_end + 1
        } else {
            line_end
        };
        if line.trim() == "---" {
            return Some(&markdown[next_offset..]);
        }
        offset = next_offset;
    }

    None
}

fn frontmatter_lines(markdown: &str) -> Option<impl Iterator<Item = &str>> {
    let frontmatter = markdown
        .strip_prefix("---\n")
        .or_else(|| markdown.strip_prefix("---\r\n"))?;
    let (frontmatter, _) = frontmatter
        .split_once("\n---\n")
        .or_else(|| frontmatter.split_once("\r\n---\r\n"))?;
    Some(frontmatter.lines())
}

fn trim_matching_quotes(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            return &value[1..value.len() - 1];
        }
    }

    value
}

fn truncate_skill_description(description: &str, max_len: usize) -> String {
    if description.chars().count() <= max_len {
        return description.to_string();
    }

    let mut trimmed = description.chars().take(max_len).collect::<String>();
    trimmed.push_str("...");
    trimmed
}

fn format_skill_source(
    source_kind: &SkillSourceKind,
    registry: Option<&str>,
    stale: Option<bool>,
) -> String {
    let mut source = match source_kind {
        SkillSourceKind::Local => "workspace/local".to_string(),
        SkillSourceKind::Registry => format!(
            "managed/{}",
            registry
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("registry")
        ),
    };
    if stale.unwrap_or(false) {
        source.push_str(" (stale)");
    }
    source
}

fn render_markdown(ui: &mut egui::Ui, markdown: &str) {
    let mut in_code_block = false;
    let mut code_block = String::new();

    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            if in_code_block {
                ui.add_sized(
                    [ui.available_width(), 220.0],
                    egui::TextEdit::multiline(&mut code_block)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace)
                        .interactive(false),
                );
                code_block.clear();
                in_code_block = false;
            } else {
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            code_block.push_str(line);
            code_block.push('\n');
            continue;
        }

        if let Some(text) = line.strip_prefix("# ") {
            ui.heading(text);
            continue;
        }
        if let Some(text) = line.strip_prefix("## ") {
            ui.add_space(6.0);
            ui.strong(text);
            continue;
        }
        if let Some(text) = line.strip_prefix("- ") {
            ui.horizontal(|ui| {
                ui.label("-");
                ui.label(text);
            });
            continue;
        }
        if line.is_empty() {
            ui.add_space(4.0);
            continue;
        }

        ui.label(line);
    }

    if in_code_block && !code_block.is_empty() {
        ui.add_sized(
            [ui.available_width(), 220.0],
            egui::TextEdit::multiline(&mut code_block)
                .desired_width(f32::INFINITY)
                .font(egui::TextStyle::Monospace)
                .interactive(false),
        );
    }
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

    #[test]
    fn validate_workspace_file_name_rejects_nested_paths() {
        let err = validate_workspace_file_name("nested/file.md").expect_err("nested path");
        assert!(err.contains("directory separators"));
    }

    #[test]
    fn create_workspace_file_writes_requested_body() {
        let workspace_dir = temp_workspace_dir();
        fs::create_dir_all(&workspace_dir).expect("workspace dir");

        let path = create_workspace_file(&workspace_dir, "PROFILE.md", "# Title\nbody")
            .expect("create file");

        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("PROFILE.md")
        );
        assert_eq!(
            fs::read_to_string(&path).expect("read file"),
            "# Title\nbody"
        );

        let _ = fs::remove_dir_all(workspace_dir);
    }

    #[test]
    fn create_workspace_file_rejects_existing_target() {
        let workspace_dir = temp_workspace_dir();
        fs::create_dir_all(&workspace_dir).expect("workspace dir");
        fs::write(workspace_dir.join("PROFILE.md"), "old").expect("seed file");

        let err =
            create_workspace_file(&workspace_dir, "PROFILE.md", "new").expect_err("existing file");

        assert!(err.contains("already exists"));

        let _ = fs::remove_dir_all(workspace_dir);
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
