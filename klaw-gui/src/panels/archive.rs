use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_millis;
use egui::{ColorImage, TextureHandle};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_archive::{
    ArchiveBlob, ArchiveError, ArchiveMediaKind, ArchiveQuery, ArchiveRecord, ArchiveService,
    ArchiveSourceKind, SqliteArchiveService, open_default_archive_service,
};
use klaw_util::command_search_path;
use std::ffi::OsStr;
use std::fs;
use std::future::Future;
use std::path::Path;
use std::thread;
use tokio::runtime::Builder;
use uuid::Uuid;

const MAX_PREVIEW_TEXT_CHARS: usize = 200_000;
const FILTER_INPUT_WIDTH: f32 = 220.0;
const PAGING_INPUT_WIDTH: f32 = 50.0;

#[derive(Default)]
pub struct ArchivePanel {
    loaded: bool,
    items: Vec<ArchiveRecord>,
    selected_archive: Option<String>,
    detail_id: Option<String>,
    session_keys: Vec<String>,
    session_key_filter: Option<String>,
    chat_id_filter: String,
    source_kind_filter: Option<ArchiveSourceKind>,
    media_kind_filter: Option<ArchiveMediaKind>,
    filename_filter: String,
    page: i64,
    size: i64,
    preview: Option<ArchivePreview>,
}

impl ArchivePanel {
    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        if self.size == 0 {
            self.size = 100;
        }
        self.load_filters(notifications);
        self.refresh(notifications);
    }

    fn load_filters(&mut self, notifications: &mut NotificationCenter) {
        match run_archive_task(move |service| async move { service.list_session_keys().await }) {
            Ok(session_keys) => {
                self.session_keys = session_keys;
            }
            Err(err) => notifications.error(format!("Failed to load filters: {err}")),
        }
    }

    fn refresh(&mut self, notifications: &mut NotificationCenter) {
        let size = self.size.max(1);
        let page = self.page.max(1);
        let offset = (page - 1) * size;

        let query = ArchiveQuery {
            session_key: self.session_key_filter.clone(),
            chat_id: optional_trimmed(&self.chat_id_filter),
            source_kind: self.source_kind_filter,
            media_kind: self.media_kind_filter,
            filename: optional_trimmed(&self.filename_filter),
            limit: size,
            offset,
        };

        match run_archive_task(move |service| async move { service.find(query).await }) {
            Ok(items) => {
                self.items = items;
                self.loaded = true;
            }
            Err(err) => notifications.error(format!("Failed to query archives: {err}")),
        }
    }

    fn selected_item(&self) -> Option<&ArchiveRecord> {
        let detail_id = self.detail_id.as_deref()?;
        self.items.iter().find(|item| item.id == detail_id)
    }

    fn open_preview(
        &mut self,
        ctx: &egui::Context,
        item: &ArchiveRecord,
        notifications: &mut NotificationCenter,
    ) {
        let Some(capability) = preview_capability_for_record(item) else {
            return;
        };

        self.preview = None;
        let archive_id = item.id.clone();
        match run_archive_task(
            move |service| async move { service.open_download(&archive_id).await },
        ) {
            Ok(blob) => match build_preview(ctx, blob, capability) {
                Ok(preview) => self.preview = Some(preview),
                Err(err) => notifications.error(format!("Failed to preview archive: {err}")),
            },
            Err(err) => notifications.error(format!("Failed to open archive for preview: {err}")),
        }
    }
}

impl PanelRenderer for ArchivePanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_loaded(notifications);

        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh(notifications);
            }
            ui.label(format!("Items: {}", self.items.len()));
        });

        ui.separator();
        let mut need_refresh = false;
        ui.horizontal(|ui| {
            ui.label("session_key");
            let selected_text = self.session_key_filter.as_deref().unwrap_or("All");
            let combo_resp = egui::ComboBox::from_id_salt("session_key_filter")
                .selected_text(selected_text)
                .width(FILTER_INPUT_WIDTH)
                .show_ui(ui, |ui| {
                    let mut changed = false;
                    if ui
                        .selectable_value(&mut self.session_key_filter, None, "All")
                        .changed()
                    {
                        changed = true;
                    }
                    for key in &self.session_keys {
                        if ui
                            .selectable_value(&mut self.session_key_filter, Some(key.clone()), key)
                            .changed()
                        {
                            changed = true;
                        }
                    }
                    changed
                });
            if combo_resp.inner.unwrap_or(false) {
                need_refresh = true;
            }
            ui.label("chat_id");
            if ui
                .add_sized(
                    [FILTER_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut self.chat_id_filter),
                )
                .changed()
            {
                need_refresh = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("source_kind");
            let selected_text = self.source_kind_filter.map_or("All", |s| s.as_str());
            let combo_resp = egui::ComboBox::from_id_salt("source_kind_filter")
                .selected_text(selected_text)
                .width(FILTER_INPUT_WIDTH)
                .show_ui(ui, |ui| {
                    let mut changed = false;
                    if ui
                        .selectable_value(&mut self.source_kind_filter, None, "All")
                        .changed()
                    {
                        changed = true;
                    }
                    for kind in [
                        ArchiveSourceKind::UserUpload,
                        ArchiveSourceKind::ChannelInbound,
                        ArchiveSourceKind::ModelGenerated,
                    ] {
                        if ui
                            .selectable_value(
                                &mut self.source_kind_filter,
                                Some(kind),
                                kind.as_str(),
                            )
                            .changed()
                        {
                            changed = true;
                        }
                    }
                    changed
                });
            if combo_resp.inner.unwrap_or(false) {
                need_refresh = true;
            }
            ui.label("media_kind");
            let selected_text = self.media_kind_filter.map_or("All", |s| s.as_str());
            let combo_resp = egui::ComboBox::from_id_salt("media_kind_filter")
                .selected_text(selected_text)
                .width(FILTER_INPUT_WIDTH)
                .show_ui(ui, |ui| {
                    let mut changed = false;
                    if ui
                        .selectable_value(&mut self.media_kind_filter, None, "All")
                        .changed()
                    {
                        changed = true;
                    }
                    for kind in [
                        ArchiveMediaKind::Pdf,
                        ArchiveMediaKind::Image,
                        ArchiveMediaKind::Video,
                        ArchiveMediaKind::Audio,
                        ArchiveMediaKind::Other,
                    ] {
                        if ui
                            .selectable_value(
                                &mut self.media_kind_filter,
                                Some(kind),
                                kind.as_str(),
                            )
                            .changed()
                        {
                            changed = true;
                        }
                    }
                    changed
                });
            if combo_resp.inner.unwrap_or(false) {
                need_refresh = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("filename");
            if ui
                .add_sized(
                    [FILTER_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut self.filename_filter),
                )
                .changed()
            {
                need_refresh = true;
            }
            ui.label("page");
            if ui
                .add_sized(
                    [PAGING_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut self.page).range(1..=i64::MAX),
                )
                .changed()
            {
                need_refresh = true;
            }
            ui.label("size");
            if ui
                .add_sized(
                    [PAGING_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut self.size).range(1..=1000),
                )
                .changed()
            {
                need_refresh = true;
            }
        });
        if need_refresh {
            self.refresh(notifications);
        }

        ui.separator();
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.items.is_empty() {
                    ui.label("No archive records found.");
                } else {
                    let table_width = ui.available_width();
                    ui.set_min_width(table_width);
                    let available_height = ui.available_height();
                    let mut view_detail_id: Option<String> = None;
                    let mut preview_item: Option<ArchiveRecord> = None;

                    TableBuilder::new(ui)
                        .striped(true)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(Column::auto().at_least(80.0))
                        .column(Column::auto().at_least(70.0))
                        .column(Column::auto().at_least(60.0))
                        .column(Column::auto().at_least(120.0))
                        .column(Column::auto().at_least(100.0))
                        .column(Column::auto().at_least(80.0))
                        .column(Column::remainder().at_least(120.0))
                        .min_scrolled_height(0.0)
                        .max_scroll_height(available_height)
                        .sense(egui::Sense::click())
                        .header(20.0, |mut header| {
                            header.col(|ui| {
                                ui.strong("ID");
                            });
                            header.col(|ui| {
                                ui.strong("Source");
                            });
                            header.col(|ui| {
                                ui.strong("Media");
                            });
                            header.col(|ui| {
                                ui.strong("Filename");
                            });
                            header.col(|ui| {
                                ui.strong("MIME");
                            });
                            header.col(|ui| {
                                ui.strong("Size");
                            });
                            header.col(|ui| {
                                ui.strong("Created At");
                            });
                        })
                        .body(|body| {
                            body.rows(20.0, self.items.len(), |mut row| {
                                let idx = row.index();
                                let item = &self.items[idx];
                                let is_selected =
                                    self.selected_archive.as_deref() == Some(&item.id);

                                row.set_selected(is_selected);

                                row.col(|ui| {
                                    ui.label(&item.id);
                                });
                                row.col(|ui| {
                                    ui.label(item.source_kind.as_str());
                                });
                                row.col(|ui| {
                                    ui.label(item.media_kind.as_str());
                                });
                                row.col(|ui| {
                                    ui.label(item.original_filename.as_deref().unwrap_or(""));
                                });
                                row.col(|ui| {
                                    ui.label(item.mime_type.as_deref().unwrap_or(""));
                                });
                                row.col(|ui| {
                                    ui.label(format_bytes(item.size_bytes));
                                });
                                row.col(|ui| {
                                    ui.label(format_timestamp_millis(item.created_at_ms));
                                });

                                let response = row.response();
                                let can_preview = preview_capability_for_record(item).is_some();
                                let interaction = handle_archive_row_interaction(
                                    is_selected,
                                    item.id.clone(),
                                    response.clicked(),
                                    response.double_clicked(),
                                    can_preview,
                                );
                                self.selected_archive = interaction.selected_id;
                                if interaction.open_preview {
                                    preview_item = Some(item.clone());
                                }

                                let item_id = item.id.clone();
                                response.context_menu(|ui| {
                                    if can_preview
                                        && ui.button(format!("{} Preview", regular::EYE)).clicked()
                                    {
                                        preview_item = Some(item.clone());
                                        ui.close();
                                    }
                                    if ui
                                        .button(format!("{} Details", regular::FILE_TEXT))
                                        .clicked()
                                    {
                                        view_detail_id = Some(item_id.clone());
                                        ui.close();
                                    }
                                    if can_preview {
                                        ui.separator();
                                    }
                                    if ui.button(format!("{} Copy ID", regular::COPY)).clicked() {
                                        ui.ctx().output_mut(|o| {
                                            o.commands.push(egui::OutputCommand::CopyText(
                                                item.id.clone(),
                                            ));
                                        });
                                        ui.close();
                                    }
                                });
                            });
                        });

                    if let Some(id) = view_detail_id {
                        self.detail_id = Some(id);
                    }
                    if let Some(item) = preview_item {
                        self.open_preview(ui.ctx(), &item, notifications);
                    }
                }
            });

        if let Some(item) = self.selected_item().cloned() {
            egui::Window::new("Archive Details")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(true)
                .show(ui.ctx(), |ui| {
                    ui.set_min_width(620.0);
                    ui.label(format!("ID: {}", item.id));
                    ui.label(format!("source_kind: {}", item.source_kind.as_str()));
                    ui.label(format!("media_kind: {}", item.media_kind.as_str()));
                    ui.label(format!("mime_type: {}", item.mime_type.unwrap_or_default()));
                    ui.label(format!(
                        "original_filename: {}",
                        item.original_filename.unwrap_or_default()
                    ));
                    ui.label(format!("size: {}", format_bytes(item.size_bytes)));
                    ui.label(format!("storage_rel_path: {}", item.storage_rel_path));
                    ui.label(format!(
                        "session_key: {}",
                        item.session_key.unwrap_or_default()
                    ));
                    ui.label(format!("chat_id: {}", item.chat_id.unwrap_or_default()));
                    ui.label(format!(
                        "message_id: {}",
                        item.message_id.unwrap_or_default()
                    ));
                    ui.label(format!(
                        "created_at: {}",
                        format_timestamp_millis(item.created_at_ms)
                    ));
                    ui.separator();
                    ui.label("metadata_json");
                    let mut metadata_json = item.metadata_json;
                    ui.add(
                        egui::TextEdit::multiline(&mut metadata_json)
                            .desired_rows(10)
                            .desired_width(f32::INFINITY)
                            .interactive(false),
                    );
                    if ui.button("Close").clicked() {
                        self.detail_id = None;
                    }
                });
        }

        if let Some(preview) = &mut self.preview {
            let mut open = true;
            egui::Window::new(format!("Preview: {}", preview.title))
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .default_width(840.0)
                .default_height(640.0)
                .resizable(true)
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    ui.label(format!("ID: {}", preview.archive_id));
                    if let Some(mime_type) = &preview.mime_type {
                        ui.label(format!("MIME: {mime_type}"));
                    }
                    ui.label(format!("Path: {}", preview.storage_rel_path));
                    if let Some(note) = &preview.note {
                        ui.label(note);
                    }
                    ui.separator();

                    match &mut preview.content {
                        ArchivePreviewContent::Text(text) => {
                            egui::ScrollArea::both()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(text)
                                            .desired_width(f32::INFINITY)
                                            .desired_rows(30)
                                            .interactive(false),
                                    );
                                });
                        }
                        ArchivePreviewContent::Image(texture) => {
                            let source_size = texture.size_vec2();
                            egui::ScrollArea::both()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let available = ui.available_size();
                                    let width_scale = if source_size.x > 0.0 {
                                        available.x / source_size.x
                                    } else {
                                        1.0
                                    };
                                    let scale = width_scale.clamp(0.1, 1.0);
                                    let desired_size = source_size * scale;
                                    ui.add(
                                        egui::Image::from_texture(&*texture)
                                            .fit_to_exact_size(desired_size),
                                    );
                                });
                        }
                    }
                });
            if !open {
                self.preview = None;
            }
        }
    }
}

struct ArchivePreview {
    archive_id: String,
    title: String,
    mime_type: Option<String>,
    storage_rel_path: String,
    note: Option<String>,
    content: ArchivePreviewContent,
}

enum ArchivePreviewContent {
    Text(String),
    Image(TextureHandle),
}

#[derive(Clone, Copy)]
enum PreviewCapability {
    Text,
    Image,
    QuickLookThumbnail,
}

fn build_preview(
    ctx: &egui::Context,
    blob: ArchiveBlob,
    capability: PreviewCapability,
) -> Result<ArchivePreview, String> {
    let archive_id = blob.record.id.clone();
    let title = preview_title(&blob.record);
    let mime_type = blob.record.mime_type.clone();
    let storage_rel_path = blob.record.storage_rel_path.clone();

    let (content, note) = match capability {
        PreviewCapability::Text => {
            let text = String::from_utf8(blob.bytes)
                .map_err(|_| "archive file is not valid UTF-8 text".to_string())?;
            let (content, truncated) = truncate_preview_text(&text, MAX_PREVIEW_TEXT_CHARS);
            let note = truncated
                .then(|| format!("Text preview truncated to {MAX_PREVIEW_TEXT_CHARS} characters."));
            (ArchivePreviewContent::Text(content), note)
        }
        PreviewCapability::Image => (
            ArchivePreviewContent::Image(load_texture_from_bytes(
                ctx,
                &format!("archive-preview-{archive_id}"),
                &blob.bytes,
            )?),
            None,
        ),
        PreviewCapability::QuickLookThumbnail => {
            let thumbnail = load_quick_look_thumbnail(&blob.absolute_path)?;
            (
                ArchivePreviewContent::Image(load_texture_from_bytes(
                    ctx,
                    &format!("archive-preview-{archive_id}-quicklook"),
                    &thumbnail,
                )?),
                Some("Preview generated from a system thumbnail.".to_string()),
            )
        }
    };

    Ok(ArchivePreview {
        archive_id,
        title,
        mime_type,
        storage_rel_path,
        note,
        content,
    })
}

fn load_texture_from_bytes(
    ctx: &egui::Context,
    texture_name: &str,
    bytes: &[u8],
) -> Result<TextureHandle, String> {
    let image = image::load_from_memory(bytes)
        .map_err(|err| format!("failed to decode preview image: {err}"))?
        .into_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color_image = ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    Ok(ctx.load_texture(
        texture_name.to_string(),
        color_image,
        egui::TextureOptions::LINEAR,
    ))
}

fn preview_capability_for_record(record: &ArchiveRecord) -> Option<PreviewCapability> {
    if is_text_previewable(record) {
        return Some(PreviewCapability::Text);
    }
    if is_image_previewable(record) {
        return Some(PreviewCapability::Image);
    }
    is_quick_look_previewable(record).then_some(PreviewCapability::QuickLookThumbnail)
}

fn is_text_previewable(record: &ArchiveRecord) -> bool {
    if record
        .mime_type
        .as_deref()
        .map(|mime| {
            mime.starts_with("text/")
                || matches!(
                    mime,
                    "application/json"
                        | "application/ld+json"
                        | "application/xml"
                        | "application/x-yaml"
                        | "application/yaml"
                        | "application/toml"
                        | "application/x-toml"
                        | "application/javascript"
                )
        })
        .unwrap_or(false)
    {
        return true;
    }

    matches!(
        record.extension.as_deref().map(normalized_extension),
        Some(
            "txt"
                | "md"
                | "markdown"
                | "json"
                | "yaml"
                | "yml"
                | "toml"
                | "xml"
                | "csv"
                | "tsv"
                | "log"
                | "ini"
                | "conf"
                | "cfg"
                | "sql"
                | "html"
                | "htm"
                | "css"
                | "js"
                | "jsx"
                | "ts"
                | "tsx"
                | "rs"
                | "py"
                | "sh"
                | "bash"
                | "zsh"
                | "java"
                | "kt"
                | "swift"
                | "go"
                | "rb"
                | "php"
                | "c"
                | "h"
                | "cpp"
                | "hpp"
                | "m"
                | "mm"
        )
    )
}

fn is_image_previewable(record: &ArchiveRecord) -> bool {
    record.media_kind == ArchiveMediaKind::Image
        || record
            .mime_type
            .as_deref()
            .map(|mime| mime.starts_with("image/"))
            .unwrap_or(false)
        || matches!(
            record.extension.as_deref().map(normalized_extension),
            Some(
                "png"
                    | "jpg"
                    | "jpeg"
                    | "gif"
                    | "bmp"
                    | "webp"
                    | "tif"
                    | "tiff"
                    | "ico"
                    | "heic"
                    | "heif"
            )
        )
}

#[cfg(target_os = "macos")]
fn is_quick_look_previewable(record: &ArchiveRecord) -> bool {
    if matches!(
        record.media_kind,
        ArchiveMediaKind::Pdf | ArchiveMediaKind::Video | ArchiveMediaKind::Audio
    ) {
        return true;
    }

    matches!(
        record.extension.as_deref().map(normalized_extension),
        Some(
            "pdf"
                | "doc"
                | "docx"
                | "ppt"
                | "pptx"
                | "xls"
                | "xlsx"
                | "pages"
                | "numbers"
                | "key"
                | "epub"
                | "rtf"
                | "rtfd"
                | "mov"
                | "mp4"
                | "m4v"
                | "avi"
                | "mkv"
                | "webm"
                | "mp3"
                | "wav"
                | "m4a"
                | "aac"
                | "flac"
                | "ogg"
        )
    )
}

#[cfg(not(target_os = "macos"))]
fn is_quick_look_previewable(_record: &ArchiveRecord) -> bool {
    false
}

fn normalized_extension(extension: &str) -> &str {
    extension.trim().trim_start_matches('.')
}

fn preview_title(record: &ArchiveRecord) -> String {
    record
        .original_filename
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| record.id.clone())
}

fn truncate_preview_text(text: &str, max_chars: usize) -> (String, bool) {
    let mut chars = text.chars();
    let content: String = chars.by_ref().take(max_chars).collect();
    (content, chars.next().is_some())
}

#[cfg(target_os = "macos")]
fn load_quick_look_thumbnail(path: &Path) -> Result<Vec<u8>, String> {
    use std::process::Command;

    let output_dir = std::env::temp_dir().join(format!("klaw-archive-preview-{}", Uuid::new_v4()));
    fs::create_dir_all(&output_dir)
        .map_err(|err| format!("failed to create preview temp dir: {err}"))?;

    let mut command = Command::new("qlmanage");
    if let Some(path) = command_search_path() {
        command.env("PATH", path);
    }
    let result = command
        .arg("-t")
        .arg("-s")
        .arg("1600")
        .arg("-o")
        .arg(&output_dir)
        .arg(path)
        .output()
        .map_err(|err| format!("failed to run qlmanage: {err}"));

    let outcome = match result {
        Ok(output) if output.status.success() => {
            let preview_path = first_preview_image_path(&output_dir)
                .ok_or_else(|| "system preview did not produce an image".to_string())?;
            fs::read(&preview_path)
                .map_err(|err| format!("failed to read generated preview: {err}"))
        }
        Ok(output) => Err(format!(
            "system preview failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(err) => Err(err),
    };

    let _ = fs::remove_dir_all(&output_dir);
    outcome
}

#[cfg(not(target_os = "macos"))]
fn load_quick_look_thumbnail(_path: &Path) -> Result<Vec<u8>, String> {
    Err("system thumbnail preview is not available on this platform".to_string())
}

#[cfg(target_os = "macos")]
fn first_preview_image_path(dir: &Path) -> Option<std::path::PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.is_file()
                && matches!(
                    path.extension()
                        .and_then(OsStr::to_str)
                        .map(normalized_extension),
                    Some("png" | "jpg" | "jpeg")
                )
        })
}

fn run_archive_task<T, F, Fut>(op: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(SqliteArchiveService) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, ArchiveError>> + Send + 'static,
{
    let join = thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("failed to build runtime: {err}"))?;

        runtime.block_on(async move {
            let service = open_default_archive_service()
                .await
                .map_err(|err| format!("failed to open archive service: {err}"))?;
            op(service)
                .await
                .map_err(|err| format!("archive operation failed: {err}"))
        })
    });

    match join.join() {
        Ok(result) => result,
        Err(_) => Err("archive operation thread panicked".to_string()),
    }
}

fn format_bytes(value: i64) -> String {
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

fn optional_trimmed(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

struct ArchiveRowInteraction {
    selected_id: Option<String>,
    open_preview: bool,
}

fn handle_archive_row_interaction(
    is_selected: bool,
    item_id: String,
    clicked: bool,
    double_clicked: bool,
    can_preview: bool,
) -> ArchiveRowInteraction {
    if double_clicked {
        return ArchiveRowInteraction {
            selected_id: Some(item_id),
            open_preview: can_preview,
        };
    }

    let selected_id = if clicked {
        if is_selected { None } else { Some(item_id) }
    } else if is_selected {
        Some(item_id)
    } else {
        None
    };

    ArchiveRowInteraction {
        selected_id,
        open_preview: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_with(
        media_kind: ArchiveMediaKind,
        mime_type: Option<&str>,
        extension: Option<&str>,
    ) -> ArchiveRecord {
        ArchiveRecord {
            id: "arch-1".to_string(),
            source_kind: ArchiveSourceKind::UserUpload,
            media_kind,
            mime_type: mime_type.map(ToOwned::to_owned),
            extension: extension.map(ToOwned::to_owned),
            original_filename: Some("sample".to_string()),
            content_sha256: "sha".to_string(),
            size_bytes: 128,
            storage_rel_path: "archives/2026-03-20/arch-1.bin".to_string(),
            session_key: None,
            channel: None,
            chat_id: None,
            message_id: None,
            metadata_json: "{}".to_string(),
            created_at_ms: 0,
        }
    }

    #[test]
    fn text_preview_supports_utf8_like_files() {
        let record = record_with(
            ArchiveMediaKind::Other,
            Some("application/json"),
            Some("json"),
        );
        assert!(matches!(
            preview_capability_for_record(&record),
            Some(PreviewCapability::Text)
        ));
    }

    #[test]
    fn image_preview_supports_image_records() {
        let record = record_with(ArchiveMediaKind::Image, Some("image/png"), Some("png"));
        assert!(matches!(
            preview_capability_for_record(&record),
            Some(PreviewCapability::Image)
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn quick_look_preview_supports_pdf() {
        let record = record_with(ArchiveMediaKind::Pdf, Some("application/pdf"), Some("pdf"));
        assert!(matches!(
            preview_capability_for_record(&record),
            Some(PreviewCapability::QuickLookThumbnail)
        ));
    }

    #[test]
    fn preview_hides_unknown_binary_types() {
        let record = record_with(
            ArchiveMediaKind::Other,
            Some("application/octet-stream"),
            Some("bin"),
        );
        assert!(preview_capability_for_record(&record).is_none());
    }

    #[test]
    fn double_click_selects_row_and_opens_preview_when_supported() {
        let interaction =
            handle_archive_row_interaction(false, "arch-1".to_string(), true, true, true);

        assert_eq!(interaction.selected_id.as_deref(), Some("arch-1"));
        assert!(interaction.open_preview);
    }

    #[test]
    fn double_click_selects_row_without_preview_for_unsupported_items() {
        let interaction =
            handle_archive_row_interaction(false, "arch-1".to_string(), true, true, false);

        assert_eq!(interaction.selected_id.as_deref(), Some("arch-1"));
        assert!(!interaction.open_preview);
    }

    #[test]
    fn single_click_toggles_selection_without_opening_preview() {
        let deselect_interaction =
            handle_archive_row_interaction(true, "arch-1".to_string(), true, false, true);
        assert!(deselect_interaction.selected_id.is_none());
        assert!(!deselect_interaction.open_preview);

        let select_interaction =
            handle_archive_row_interaction(false, "arch-1".to_string(), true, false, true);
        assert_eq!(select_interaction.selected_id.as_deref(), Some("arch-1"));
        assert!(!select_interaction.open_preview);
    }
}
