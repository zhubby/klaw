use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_millis;
use crate::widgets::{ChatBox, ChatMessage, ChatRole};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_session::{
    LlmUsageSummary, SessionError, SessionIndex, SessionListQuery, SessionManager,
    SqliteSessionManager,
};
use std::future::Future;
use std::thread;
use tokio::runtime::Builder;

#[derive(Default)]
pub struct SessionPanel {
    loaded: bool,
    sessions: Vec<SessionRow>,
    limit_text: String,
    offset_text: String,
    selected_session: Option<String>,
    chat_box: Option<ChatBox>,
}

#[derive(Debug, Clone)]
struct SessionRow {
    session: SessionIndex,
    usage: LlmUsageSummary,
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

        match run_session_task(move |manager| async move {
            let sessions = manager.list_sessions(query).await?;
            let mut rows = Vec::with_capacity(sessions.len());
            for session in sessions {
                let usage = manager
                    .sum_llm_usage_by_session(&session.session_key)
                    .await?;
                rows.push(SessionRow { session, usage });
            }
            Ok(rows)
        }) {
            Ok(sessions) => {
                self.sessions = sessions;
                self.loaded = true;
            }
            Err(err) => notifications.error(format!("Failed to load sessions: {err}")),
        }
    }

    fn load_chat_session(&mut self, session_key: &str, notifications: &mut NotificationCenter) {
        let session_key_owned = session_key.to_string();
        match run_session_task(move |manager| async move {
            manager.read_chat_records(&session_key_owned).await
        }) {
            Ok(records) => {
                let messages: Vec<ChatMessage> = records
                    .iter()
                    .map(|r| {
                        ChatMessage::new(ChatRole::from_str(&r.role), &r.content)
                            .with_timestamp(r.ts_ms)
                    })
                    .collect();

                let mut chat_box =
                    ChatBox::new(format!("Chat: {}", session_key)).with_messages(messages);
                chat_box.open();
                self.chat_box = Some(chat_box);
            }
            Err(err) => {
                notifications.error(format!("Failed to load chat records: {err}"));
            }
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

        let mut view_session_key: Option<String> = None;

        let table_width = ui.available_width();
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .max_width(table_width)
            .show(ui, |ui| {
                ui.set_min_width(table_width);
                if self.sessions.is_empty() {
                    ui.label("No sessions found.");
                    return;
                }

                let available_height = ui.available_height();
                TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::auto().at_least(100.0))
                    .column(Column::auto().at_least(80.0))
                    .column(Column::auto().at_least(60.0))
                    .column(Column::auto().at_least(80.0))
                    .column(Column::auto().at_least(80.0))
                    .column(Column::auto().at_least(80.0))
                    .column(Column::auto().at_least(50.0))
                    .column(Column::auto().at_least(70.0))
                    .column(Column::auto().at_least(70.0))
                    .column(Column::auto().at_least(70.0))
                    .column(Column::auto().at_least(100.0))
                    .column(Column::remainder().at_least(100.0))
                    .min_scrolled_height(0.0)
                    .max_scroll_height(available_height)
                    .sense(egui::Sense::click())
                    .header(20.0, |mut header| {
                        header.col(|ui| {
                            ui.strong("Session Key");
                        });
                        header.col(|ui| {
                            ui.strong("Chat ID");
                        });
                        header.col(|ui| {
                            ui.strong("Channel");
                        });
                        header.col(|ui| {
                            ui.strong("Active Session");
                        });
                        header.col(|ui| {
                            ui.strong("Provider");
                        });
                        header.col(|ui| {
                            ui.strong("Model");
                        });
                        header.col(|ui| {
                            ui.strong("Turns");
                        });
                        header.col(|ui| {
                            ui.strong("Input");
                        });
                        header.col(|ui| {
                            ui.strong("Output");
                        });
                        header.col(|ui| {
                            ui.strong("Total");
                        });
                        header.col(|ui| {
                            ui.strong("Updated At");
                        });
                        header.col(|ui| {
                            ui.strong("JSONL Path");
                        });
                    })
                    .body(|body| {
                        body.rows(20.0, self.sessions.len(), |mut row| {
                            let idx = row.index();
                            let session_row = &self.sessions[idx];
                            let session = &session_row.session;
                            let is_selected =
                                self.selected_session.as_deref() == Some(&session.session_key);

                            row.set_selected(is_selected);

                            row.col(|ui| {
                                ui.label(&session.session_key);
                            });
                            row.col(|ui| {
                                ui.label(&session.chat_id);
                            });
                            row.col(|ui| {
                                ui.label(&session.channel);
                            });
                            row.col(|ui| {
                                ui.label(session.active_session_key.as_deref().unwrap_or(""));
                            });
                            row.col(|ui| {
                                ui.label(session.model_provider.as_deref().unwrap_or(""));
                            });
                            row.col(|ui| {
                                ui.label(session.model.as_deref().unwrap_or(""));
                            });
                            row.col(|ui| {
                                ui.label(session.turn_count.to_string());
                            });
                            row.col(|ui| {
                                ui.label(session_row.usage.input_tokens.to_string());
                            });
                            row.col(|ui| {
                                ui.label(session_row.usage.output_tokens.to_string());
                            });
                            row.col(|ui| {
                                ui.label(session_row.usage.total_tokens.to_string());
                            });
                            row.col(|ui| {
                                ui.label(format_timestamp_millis(session.updated_at_ms));
                            });
                            row.col(|ui| {
                                ui.label(&session.jsonl_path);
                            });

                            let response = row.response();

                            if response.clicked() {
                                self.selected_session = if is_selected {
                                    None
                                } else {
                                    Some(session.session_key.clone())
                                };
                            }

                            response.context_menu(|ui| {
                                if ui
                                    .button(format!("{} View Chat", regular::CHATS_CIRCLE))
                                    .clicked()
                                {
                                    view_session_key = Some(session.session_key.clone());
                                    ui.close();
                                }
                                if ui
                                    .button(format!("{} Copy Session Key", regular::KEY))
                                    .clicked()
                                {
                                    ui.ctx().output_mut(|o| {
                                        o.commands.push(egui::OutputCommand::CopyText(
                                            session.session_key.clone(),
                                        ));
                                    });
                                    ui.close();
                                }
                            });
                        });
                    });
            });

        if let Some(session_key) = view_session_key {
            self.load_chat_session(&session_key, notifications);
        }

        if let Some(chat_box) = &mut self.chat_box {
            chat_box.show(ui.ctx());
        }
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
