use eframe::egui::{
    self, Align, Align2, Button, ComboBox, Context, Frame, Id, Key, Layout, RichText, ScrollArea,
    TextEdit, TopBottomPanel, vec2,
};
use egui_phosphor::regular;

use crate::{
    ConnectionState, MessageRole, ThemeMode, normalize_gateway_token_input, toolbar_title,
};

use super::{
    app::ChatApp,
    session::{
        BUBBLE_MAX_WIDTH, ChatMessage, INPUT_PANEL_HEIGHT, SESSION_LIST_WIDTH,
        SESSION_WINDOW_DEFAULT_HEIGHT, SESSION_WINDOW_DEFAULT_WIDTH, SESSION_WINDOW_MIN_HEIGHT,
        SESSION_WINDOW_MIN_WIDTH, WindowAnchor, format_message_timestamp,
    },
};

impl ChatApp {
    fn render_top_bar(&mut self, ctx: &Context) {
        TopBottomPanel::top("klaw-webui-toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("New Session").clicked() {
                    self.create_session();
                }
                if ui.button("Tile Windows").clicked() {
                    self.tile_open_sessions();
                }
                if ui.button("Reset Layout").clicked() {
                    self.reset_window_layout();
                }
                if ui.button("Gateway Token").clicked() {
                    self.show_gateway_dialog = true;
                }
                if ui.button("Reconnect All").clicked() {
                    self.reconnect_all_sessions();
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(toolbar_title()).strong());
                });
            });
        });
    }

    fn render_status_bar(&mut self, ctx: &Context) {
        let mut requested_theme = None;
        TopBottomPanel::bottom("klaw-webui-status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Theme Mode:");
                ComboBox::from_id_salt("klaw-webui-theme-mode")
                    .width(110.0)
                    .selected_text(self.theme_mode.label())
                    .show_ui(ui, |ui| {
                        for mode in [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark] {
                            if ui
                                .selectable_label(self.theme_mode == mode, mode.label())
                                .clicked()
                            {
                                requested_theme = Some(mode);
                                ui.close();
                            }
                        }
                    });
                ui.separator();
                ui.label(format!("Sessions: {}", self.sessions.len()));

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(active_session_key) = self.active_session_key.as_deref() {
                        if let Some(session) = self
                            .sessions
                            .iter()
                            .find(|session| session.session_key == active_session_key)
                        {
                            ui.label(session.connection_state().status_text());
                            ui.separator();
                            ui.label(&session.title);
                        }
                    } else {
                        ui.label("No active session");
                    }
                });
            });
        });

        if let Some(theme_mode) = requested_theme {
            self.set_theme_mode(theme_mode);
        }
    }

    fn session_list_order(&self) -> Vec<String> {
        let active = self.active_session_key.as_deref();
        let mut visible = Vec::new();
        let mut hidden = Vec::new();

        for session in &self.sessions {
            if active == Some(session.session_key.as_str()) {
                continue;
            }
            if session.open {
                visible.push(session.session_key.clone());
            } else {
                hidden.push(session.session_key.clone());
            }
        }

        let mut ordered = Vec::with_capacity(self.sessions.len());
        if let Some(active_session) = self
            .sessions
            .iter()
            .find(|session| active == Some(session.session_key.as_str()))
        {
            ordered.push(active_session.session_key.clone());
        }
        ordered.extend(visible);
        ordered.extend(hidden);
        ordered
    }

    fn render_session_list(&mut self, ctx: &Context) {
        let mut remove_session_key = None;
        let mut focus_session_key = None;
        let mut rename_session_key = None;

        egui::SidePanel::left("klaw-webui-sessions")
            .resizable(true)
            .default_width(SESSION_LIST_WIDTH)
            .show(ctx, |ui| {
                ui.heading("Sessions");
                ui.separator();

                if self.sessions.is_empty() {
                    ui.label("No sessions yet.");
                    return;
                }

                ScrollArea::vertical().show(ui, |ui| {
                    for session_key in self.session_list_order() {
                        let Some(index) = self.session_index(&session_key) else {
                            continue;
                        };
                        let session = &self.sessions[index];
                        let is_active = self.active_session_key.as_deref()
                            == Some(session.session_key.as_str());
                        let card = Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(regular::APP_WINDOW);
                                ui.label(
                                    RichText::new(&session.title).strong().size(if is_active {
                                        15.0
                                    } else {
                                        14.0
                                    }),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if is_active {
                                        ui.label(RichText::new("Active").small().strong());
                                    }
                                });
                            });
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(session.connection_state().status_text()).small(),
                                );
                                ui.separator();
                                ui.label(
                                    RichText::new(if session.open { "Visible" } else { "Hidden" })
                                        .small(),
                                );
                            });
                            ui.add_space(2.0);
                            ui.label(RichText::new(&session.session_key).small().weak());
                        });
                        if card.response.clicked() {
                            focus_session_key = Some(session.session_key.clone());
                        }
                        card.response.context_menu(|ui| {
                            if ui
                                .button(format!("{} Rename", regular::PENCIL_SIMPLE))
                                .clicked()
                            {
                                rename_session_key = Some(session.session_key.clone());
                                ui.close();
                            }
                            if ui.button(format!("{} Delete", regular::TRASH)).clicked() {
                                remove_session_key = Some(session.session_key.clone());
                                ui.close();
                            }
                        });
                        ui.add_space(6.0);
                    }
                });
            });

        if let Some(session_key) = focus_session_key {
            self.focus_session(&session_key);
        }
        if let Some(session_key) = rename_session_key
            && let Some(index) = self.session_index(&session_key)
        {
            self.rename_session_key = Some(session_key);
            self.rename_session_input = self.sessions[index].title.clone();
        }
        if let Some(session_key) = remove_session_key {
            self.remove_session(&session_key);
        }
    }

    fn render_rename_dialog(&mut self, ctx: &Context) {
        let Some(session_key) = self.rename_session_key.clone() else {
            return;
        };

        let mut open = true;
        let mut submit = false;
        let mut cancel = false;

        egui::Window::new("Rename Session")
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                let response = ui.add(
                    TextEdit::singleline(&mut self.rename_session_input)
                        .desired_width(f32::INFINITY)
                        .hint_text("Session name"),
                );
                let submit_with_enter =
                    response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() || submit_with_enter {
                        submit = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if submit {
            let trimmed = self.rename_session_input.trim();
            if !trimmed.is_empty()
                && let Some(index) = self.session_index(&session_key)
            {
                self.sessions[index].title = trimmed.to_string();
                self.persist_workspace_state();
            }
            self.rename_session_key = None;
            self.rename_session_input.clear();
            return;
        }

        if cancel || !open {
            self.rename_session_key = None;
            self.rename_session_input.clear();
        }
    }

    fn render_gateway_dialog(&mut self, ctx: &Context) {
        if !self.show_gateway_dialog {
            return;
        }

        let mut open = self.show_gateway_dialog;
        let mut reconnect_all = false;

        egui::Window::new("Gateway Token")
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.label("If gateway auth is enabled, enter the token here.");
                ui.label(
                    RichText::new("Leave it blank when auth is disabled.")
                        .small()
                        .weak(),
                );
                ui.add_space(8.0);

                let response = ui.add(
                    TextEdit::singleline(&mut self.gateway_token_input)
                        .password(true)
                        .desired_width(f32::INFINITY)
                        .hint_text("Gateway token"),
                );
                let submit_with_enter =
                    response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Save & Reconnect").clicked() || submit_with_enter {
                        reconnect_all = true;
                    }
                    if ui.button("Clear").clicked() {
                        self.gateway_token_input.clear();
                        self.gateway_token = None;
                    }
                });
            });

        self.show_gateway_dialog = open;

        if reconnect_all {
            self.gateway_token = normalize_gateway_token_input(&self.gateway_token_input);
            self.reconnect_all_sessions();
            self.show_gateway_dialog = false;
        }
    }

    fn render_session_window(&mut self, ctx: &Context, session_key: &str) {
        let Some(index) = self.session_index(session_key) else {
            return;
        };

        let mut trigger_send = false;
        let mut trigger_connect = false;
        let mut trigger_disconnect = false;
        let mut set_active = false;
        let mut persist_after_render = false;
        {
            let session = &mut self.sessions[index];
            let state = session.connection_state();
            let messages = session.buffers.messages.borrow().clone();
            let error_text = match &state {
                ConnectionState::Error(message) => Some(message.clone()),
                _ => None,
            };
            let mut open = session.open;

            let window = egui::Window::new(&session.title)
                .id(Id::new(("session-window", &session.session_key)))
                .default_pos(session.window_anchor.to_pos2())
                .default_size([SESSION_WINDOW_DEFAULT_WIDTH, SESSION_WINDOW_DEFAULT_HEIGHT])
                .min_width(SESSION_WINDOW_MIN_WIDTH)
                .min_height(SESSION_WINDOW_MIN_HEIGHT)
                .open(&mut open);

            if let Some(inner) = window.show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&session.session_key).small().weak());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let button_label = if matches!(state, ConnectionState::Connected) {
                            "Disconnect"
                        } else if matches!(state, ConnectionState::Connecting) {
                            "Connecting…"
                        } else {
                            "Connect"
                        };
                        let button = ui.add_enabled(
                            !matches!(state, ConnectionState::Connecting),
                            Button::new(button_label),
                        );
                        if button.clicked() {
                            if matches!(state, ConnectionState::Connected) {
                                trigger_disconnect = true;
                            } else {
                                trigger_connect = true;
                            }
                        }
                        ui.label(state.status_text());
                    });
                });
                if let Some(message) = error_text {
                    ui.label(RichText::new(message).small().weak());
                }
                ui.separator();

                let messages_height = (ui.available_height() - INPUT_PANEL_HEIGHT).max(140.0);
                ui.allocate_ui(vec2(ui.available_width(), messages_height), |ui| {
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            if messages.is_empty() {
                                render_empty_state(ui, &state);
                                return;
                            }

                            for message in &messages {
                                render_message(ui, message);
                                ui.add_space(8.0);
                            }
                        });
                });

                ui.separator();
                ui.vertical(|ui| {
                    let input = TextEdit::multiline(&mut session.draft)
                        .desired_rows(3)
                        .hint_text(state.composer_hint_text())
                        .interactive(state.can_send());
                    let response = ui.add_sized([ui.available_width(), 72.0], input);
                    let shortcut = response.has_focus()
                        && ui.input(|input| {
                            input.key_pressed(Key::Enter)
                                && (input.modifiers.command || input.modifiers.ctrl)
                        });

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let helper_text = if state.can_send() {
                            "Cmd/Ctrl+Enter to send"
                        } else {
                            state.composer_hint_text()
                        };
                        ui.label(RichText::new(helper_text).small().weak());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let send_button = ui.add_enabled(state.can_send(), Button::new("Send"));
                            if send_button.clicked() || shortcut {
                                trigger_send = true;
                            }
                        });
                    });
                });
            }) {
                set_active = inner.response.clicked();
                let next_anchor = WindowAnchor::from_pos2(inner.response.rect.min);
                if session.window_anchor != next_anchor {
                    session.window_anchor = next_anchor;
                    persist_after_render = true;
                }
            }

            session.open = open;
            if !open {
                self.persist_workspace_state();
                return;
            }
        }

        let became_active = set_active && self.active_session_key.as_deref() != Some(session_key);
        let moved_to_front = if set_active {
            self.bring_session_to_front(session_key)
        } else {
            false
        };
        if became_active {
            self.active_session_key = Some(session_key.to_string());
        }
        if became_active || moved_to_front {
            self.persist_workspace_state();
        }
        if trigger_disconnect && let Some(index) = self.session_index(session_key) {
            Self::close_buffers(&self.sessions[index].buffers);
        }
        if trigger_connect {
            self.try_connect_session(session_key);
        }
        if trigger_send {
            self.send_session_draft(session_key);
        }
        if persist_after_render {
            self.persist_workspace_state();
        }
    }

    fn session_render_order(&self) -> Vec<String> {
        let active = self.active_session_key.as_deref();
        let mut ordered = self
            .sessions
            .iter()
            .filter(|session| active != Some(session.session_key.as_str()))
            .map(|session| session.session_key.clone())
            .collect::<Vec<_>>();
        if let Some(active_session) = self
            .sessions
            .iter()
            .find(|session| active == Some(session.session_key.as_str()))
        {
            ordered.push(active_session.session_key.clone());
        }
        ordered
    }

    fn render_workbench(&mut self, ctx: &Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.sessions.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("No sessions open. Click New Session to start.");
                });
                return;
            }

            ui.label(RichText::new("Workbench").strong());
            ui.label(
                RichText::new("Each session opens as its own egui window.")
                    .small()
                    .weak(),
            );
        });

        for session_key in self.session_render_order() {
            if self
                .session_index(&session_key)
                .and_then(|index| self.sessions.get(index))
                .is_some_and(|session| session.open)
            {
                self.render_session_window(ctx, &session_key);
            }
        }
    }
}

impl eframe::App for ChatApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.apply_theme();
        self.maybe_auto_connect_prefilled_token();
        self.render_top_bar(ctx);
        self.render_status_bar(ctx);
        self.render_session_list(ctx);
        self.render_workbench(ctx);
        self.render_gateway_dialog(ctx);
        self.render_rename_dialog(ctx);
        self.toasts.borrow_mut().show(ctx);
    }
}

fn render_empty_state(ui: &mut egui::Ui, state: &ConnectionState) {
    let copy = state.empty_state_copy();
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(copy.title).heading().strong());
        ui.add_space(4.0);
        ui.label(RichText::new(copy.body).weak());
    });
}

fn render_message(ui: &mut egui::Ui, message: &ChatMessage) {
    let time_label = format_message_timestamp(message.timestamp_ms);
    match message.role {
        MessageRole::System => {
            ui.vertical_centered(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("System").small().strong().weak());
                    ui.label(RichText::new(time_label).small().weak());
                });
                ui.label(RichText::new(&message.text).small().weak());
            });
        }
        MessageRole::Assistant | MessageRole::User => {
            let role_label = match message.role {
                MessageRole::Assistant => "Klaw",
                MessageRole::User => "You",
                MessageRole::System => "System",
            };
            let layout = if matches!(message.role, MessageRole::User) {
                Layout::right_to_left(Align::TOP)
            } else {
                Layout::left_to_right(Align::TOP)
            };
            ui.with_layout(layout, |ui| {
                Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_max_width(BUBBLE_MAX_WIDTH);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(role_label).strong());
                        ui.label(RichText::new(time_label).small().weak());
                    });
                    ui.add_space(4.0);
                    ui.label(&message.text);
                });
            });
        }
    }
}
