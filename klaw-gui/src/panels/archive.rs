use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use klaw_archive::{
    open_default_archive_service, ArchiveMediaKind, ArchiveQuery, ArchiveRecord, ArchiveService,
    ArchiveSourceKind, SqliteArchiveService,
};
use tokio::runtime::{Builder, Runtime};

#[derive(Default)]
pub struct ArchivePanel {
    runtime: Option<Runtime>,
    service: Option<SqliteArchiveService>,
    items: Vec<ArchiveRecord>,
    selected_id: Option<String>,
    session_key_filter: String,
    chat_id_filter: String,
    source_kind_filter: String,
    media_kind_filter: String,
    limit_text: String,
    offset_text: String,
}

impl ArchivePanel {
    fn ensure_service_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.runtime.is_some() && self.service.is_some() {
            return;
        }

        let runtime = match Builder::new_current_thread().enable_all().build() {
            Ok(runtime) => runtime,
            Err(err) => {
                notifications.error(format!("Failed to create runtime: {err}"));
                return;
            }
        };

        let service = match runtime.block_on(open_default_archive_service()) {
            Ok(service) => service,
            Err(err) => {
                notifications.error(format!("Failed to open archive service: {err}"));
                return;
            }
        };

        self.runtime = Some(runtime);
        self.service = Some(service);
        if self.limit_text.is_empty() {
            self.limit_text = "50".to_string();
        }
        self.refresh(notifications);
    }

    fn refresh(&mut self, notifications: &mut NotificationCenter) {
        let (Some(runtime), Some(service)) = (self.runtime.as_ref(), self.service.as_ref()) else {
            notifications.error("Archive service is not available");
            return;
        };

        let source_kind = match parse_source_kind(&self.source_kind_filter) {
            Ok(value) => value,
            Err(err) => {
                notifications.error(err);
                return;
            }
        };
        let media_kind = match parse_media_kind(&self.media_kind_filter) {
            Ok(value) => value,
            Err(err) => {
                notifications.error(err);
                return;
            }
        };

        let limit = self.limit_text.trim().parse::<i64>().unwrap_or(50).max(1);
        let offset = self.offset_text.trim().parse::<i64>().unwrap_or(0).max(0);

        let query = ArchiveQuery {
            session_key: optional_trimmed(&self.session_key_filter),
            chat_id: optional_trimmed(&self.chat_id_filter),
            source_kind,
            media_kind,
            limit,
            offset,
        };

        match runtime.block_on(service.find(query)) {
            Ok(items) => {
                self.items = items;
            }
            Err(err) => notifications.error(format!("Failed to query archives: {err}")),
        }
    }

    fn selected_item(&self) -> Option<&ArchiveRecord> {
        let selected_id = self.selected_id.as_deref()?;
        self.items.iter().find(|item| item.id == selected_id)
    }
}

impl PanelRenderer for ArchivePanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_service_loaded(notifications);

        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh(notifications);
            }
            ui.label(format!("Items: {}", self.items.len()));
        });

        ui.separator();
        egui::Grid::new("archive-filter-grid")
            .num_columns(4)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label("session_key");
                ui.text_edit_singleline(&mut self.session_key_filter);
                ui.label("chat_id");
                ui.text_edit_singleline(&mut self.chat_id_filter);
                ui.end_row();

                ui.label("source_kind");
                ui.text_edit_singleline(&mut self.source_kind_filter);
                ui.label("media_kind");
                ui.text_edit_singleline(&mut self.media_kind_filter);
                ui.end_row();

                ui.label("limit");
                ui.text_edit_singleline(&mut self.limit_text);
                ui.label("offset");
                ui.text_edit_singleline(&mut self.offset_text);
                ui.end_row();
            });

        if ui.button("Apply Filters").clicked() {
            self.refresh(notifications);
        }

        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.items.is_empty() {
                ui.label("No archive records found.");
            } else {
                egui::Grid::new("archive-list-grid")
                    .striped(true)
                    .num_columns(8)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.strong("ID");
                        ui.strong("Source");
                        ui.strong("Media");
                        ui.strong("Filename");
                        ui.strong("MIME");
                        ui.strong("Size");
                        ui.strong("Created(ms)");
                        ui.strong("Actions");
                        ui.end_row();

                        let items = self.items.clone();
                        for item in items {
                            ui.label(&item.id);
                            ui.label(item.source_kind.as_str());
                            ui.label(item.media_kind.as_str());
                            ui.label(item.original_filename.as_deref().unwrap_or(""));
                            ui.label(item.mime_type.as_deref().unwrap_or(""));
                            ui.label(item.size_bytes.to_string());
                            ui.label(item.created_at_ms.to_string());
                            if ui.button("Details").clicked() {
                                self.selected_id = Some(item.id.clone());
                            }
                            ui.end_row();
                        }
                    });
            }
        });

        if let Some(item) = self.selected_item().cloned() {
            egui::Window::new("Archive Details")
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
                    ui.label(format!("size_bytes: {}", item.size_bytes));
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
                    ui.label(format!("created_at_ms: {}", item.created_at_ms));
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
                        self.selected_id = None;
                    }
                });
        }
    }
}

fn optional_trimmed(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn parse_source_kind(value: &str) -> Result<Option<ArchiveSourceKind>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    ArchiveSourceKind::parse(trimmed).map(Some).ok_or_else(|| {
        "source_kind must be one of: user_upload, channel_inbound, model_generated".to_string()
    })
}

fn parse_media_kind(value: &str) -> Result<Option<ArchiveMediaKind>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    ArchiveMediaKind::parse(trimmed)
        .map(Some)
        .ok_or_else(|| "media_kind must be one of: pdf, image, video, audio, other".to_string())
}
