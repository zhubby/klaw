use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use klaw_session::{
    SessionError, SessionIndex, SessionListQuery, SessionManager, SqliteSessionManager,
};
use std::future::Future;
use std::thread;
use tokio::runtime::Builder;

#[derive(Default)]
pub struct SessionPanel {
    loaded: bool,
    sessions: Vec<SessionIndex>,
    limit_text: String,
    offset_text: String,
}

impl SessionPanel {
    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        if self.limit_text.is_empty() {
            self.limit_text = "100".to_string();
        }
        self.refresh(notifications);
    }

    fn refresh(&mut self, notifications: &mut NotificationCenter) {
        let query = SessionListQuery {
            limit: self.limit_text.trim().parse::<i64>().unwrap_or(100),
            offset: self.offset_text.trim().parse::<i64>().unwrap_or(0),
        };

        match run_session_task(move |manager| async move { manager.list_sessions(query).await }) {
            Ok(sessions) => {
                self.sessions = sessions;
                self.loaded = true;
            }
            Err(err) => notifications.error(format!("Failed to load sessions: {err}")),
        }
    }
}

impl PanelRenderer for SessionPanel {
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
            ui.label(format!("Sessions: {}", self.sessions.len()));
        });

        ui.separator();
        egui::Grid::new("session-filter-grid")
            .num_columns(4)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label("limit");
                ui.text_edit_singleline(&mut self.limit_text);
                ui.label("offset");
                ui.text_edit_singleline(&mut self.offset_text);
                ui.end_row();
            });

        if ui.button("Apply").clicked() {
            self.refresh(notifications);
        }

        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.sessions.is_empty() {
                ui.label("No sessions found.");
                return;
            }

            egui::Grid::new("session-table-grid")
                .striped(true)
                .num_columns(9)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.strong("Session Key");
                    ui.strong("Chat ID");
                    ui.strong("Channel");
                    ui.strong("Active Session");
                    ui.strong("Provider");
                    ui.strong("Model");
                    ui.strong("Turns");
                    ui.strong("Updated(ms)");
                    ui.strong("JSONL Path");
                    ui.end_row();

                    for session in &self.sessions {
                        ui.label(&session.session_key);
                        ui.label(&session.chat_id);
                        ui.label(&session.channel);
                        ui.label(session.active_session_key.as_deref().unwrap_or(""));
                        ui.label(session.model_provider.as_deref().unwrap_or(""));
                        ui.label(session.model.as_deref().unwrap_or(""));
                        ui.label(session.turn_count.to_string());
                        ui.label(session.updated_at_ms.to_string());
                        ui.label(&session.jsonl_path);
                        ui.end_row();
                    }
                });
        });
    }
}

fn run_session_task<T, F, Fut>(op: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(Box<dyn SessionManager>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, SessionError>> + Send + 'static,
{
    let join = thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("failed to build runtime: {err}"))?;

        runtime.block_on(async move {
            let manager: Box<dyn SessionManager> = Box::new(
                SqliteSessionManager::open_default()
                    .await
                    .map_err(|err| format!("failed to open session manager: {err}"))?,
            );
            op(manager)
                .await
                .map_err(|err| format!("session operation failed: {err}"))
        })
    });

    match join.join() {
        Ok(result) => result,
        Err(_) => Err("session operation thread panicked".to_string()),
    }
}
