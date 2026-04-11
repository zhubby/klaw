use eframe::egui::{
    self, vec2, Align, Align2, Button, Color32, ComboBox, Context, Frame, Image, Key, Layout,
    RichText, ScrollArea, Stroke, TextEdit, TextStyle, TopBottomPanel, WidgetText,
};
use egui_phosphor::regular;

use crate::{
    connection_action_label, delete_confirmation_body, derive_page_mode,
    normalize_gateway_token_input, should_activate_session_window, ConnectionState, MessageRole,
    PageMode,
};

use super::{
    app::ChatApp,
    markdown::{render_markdown, render_plain_message, MarkdownCache},
    session::{
        current_timestamp_ms, format_message_timestamp, format_relative_time, session_window_id,
        ChatMessage, BUBBLE_MAX_WIDTH, INPUT_PANEL_HEIGHT, SESSION_LIST_WIDTH,
        SESSION_WINDOW_DEFAULT_HEIGHT, SESSION_WINDOW_DEFAULT_WIDTH, SESSION_WINDOW_MIN_HEIGHT,
        SESSION_WINDOW_MIN_WIDTH,
    },
};

impl ChatApp {
    fn render_top_bar(&mut self, ctx: &Context) {
        TopBottomPanel::top("klaw-webui-toolbar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(format!("{} Window", regular::APP_WINDOW), |ui| {
                    if ui
                        .button(format!("{} Tile Windows", regular::APP_WINDOW))
                        .clicked()
                    {
                        self.tile_open_sessions();
                        ui.close();
                    }
                    if ui
                        .button(format!("{} Reset Layout", regular::ARROWS_OUT))
                        .clicked()
                    {
                        self.reset_window_layout();
                        ui.close();
                    }
                });

                ui.menu_button(format!("{} Connection", regular::PLUG), |ui| {
                    let connection_action =
                        connection_action_label(&self.connection_state.borrow().clone());
                    if ui
                        .button(format!("{} Gateway Token", regular::KEY))
                        .clicked()
                    {
                        self.show_gateway_dialog = true;
                        ui.close();
                    }
                    if ui
                        .button(format!("{} {connection_action}", regular::ARROWS_CLOCKWISE))
                        .clicked()
                    {
                        self.request_workspace_connection();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            matches!(
                                *self.connection_state.borrow(),
                                ConnectionState::Connected | ConnectionState::Connecting
                            ),
                            Button::new(format!("{} Disconnect", regular::SIGN_OUT)),
                        )
                        .clicked()
                    {
                        self.disconnect_and_clear_token();
                        ui.close();
                    }
                });

                let row_height = ui.spacing().interact_size.y;
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), row_height),
                    Layout::right_to_left(Align::Center),
                    |ui| {
                        let state = self.connection_state.borrow().clone();
                        let (dot, label) = match state {
                            ConnectionState::Connected => {
                                (Color32::from_rgb(41, 163, 90), "Connected")
                            }
                            ConnectionState::Connecting => {
                                (Color32::from_rgb(214, 149, 33), "Connecting…")
                            }
                            ConnectionState::Disconnected => {
                                (Color32::from_rgb(208, 67, 67), "Disconnected")
                            }
                            ConnectionState::Error(_) => (Color32::from_rgb(208, 67, 67), "Error"),
                        };
                        ui.label(RichText::new(label).small().strong());
                        ui.label(RichText::new("●").color(dot));
                    },
                );
            });
        });
    }

    fn render_status_bar(&mut self, ctx: &Context) {
        let mut requested_theme = None;
        TopBottomPanel::bottom("klaw-webui-status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Theme Mode:");
                let current_theme = self.ctx.options(|opt| opt.theme_preference);
                ComboBox::from_id_salt("klaw-webui-theme-mode")
                    .width(110.0)
                    .selected_text(theme_preference_label(current_theme))
                    .show_ui(ui, |ui| {
                        for mode in [
                            egui::ThemePreference::System,
                            egui::ThemePreference::Light,
                            egui::ThemePreference::Dark,
                        ] {
                            if ui
                                .selectable_label(
                                    current_theme == mode,
                                    theme_preference_label(mode),
                                )
                                .clicked()
                            {
                                requested_theme = Some(mode);
                                ui.close();
                            }
                        }
                    });
                ui.separator();
                ui.label(format!("Agents: {}", self.sessions.len()));

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(self.connection_state.borrow().status_text());
                    if let Some(active_session_key) = self.active_session_key.as_deref()
                        && let Some(session) = self
                            .sessions
                            .iter()
                            .find(|session| session.session_key == active_session_key)
                        && self.is_workspace_ready()
                    {
                        ui.separator();
                        ui.label(&session.title);
                    } else {
                        ui.separator();
                        ui.label("No active agent");
                    }
                });
            });
        });

        if let Some(theme_mode) = requested_theme {
            self.set_theme_mode(theme_mode);
        }
    }

    fn session_list_order(&self) -> Vec<String> {
        self.sessions
            .iter()
            .map(|session| session.session_key.clone())
            .collect()
    }

    fn render_session_list(&mut self, ctx: &Context) {
        if !self.is_workspace_ready() {
            return;
        }
        let mut remove_session_key = None;
        let mut focus_session_key = None;
        let mut rename_session_key = None;
        let mut copy_session_key = None;
        let mut create_session = false;

        egui::SidePanel::left("klaw-webui-sessions")
            .resizable(true)
            .default_width(SESSION_LIST_WIDTH)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(format!("{} Agents", regular::ROBOT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add_enabled(self.is_workspace_ready(), Button::new(regular::PLUS))
                            .clicked()
                        {
                            create_session = true;
                        }
                    });
                });
                ui.separator();

                if self.sessions.is_empty() {
                    ui.label("No agents yet.");
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
                        let now_ms = current_timestamp_ms();
                        let relative_time = format_relative_time(session.created_at_ms, now_ms);
                        let compact_title = compact_sidebar_title(&session.title);
                        let card = Frame::group(ui.style())
                            .fill(if is_active {
                                ui.visuals().faint_bg_color
                            } else {
                                ui.visuals().widgets.noninteractive.bg_fill
                            })
                            .stroke(if is_active {
                                ui.visuals().selection.stroke
                            } else {
                                ui.visuals().widgets.noninteractive.bg_stroke
                            })
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    ui.label(regular::ROBOT);
                                    ui.label(RichText::new(compact_title).strong());
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(RichText::new(relative_time).small().weak());
                                    });
                                });
                            });
                        let response =
                            card.response
                                .interact(egui::Sense::click())
                                .on_hover_text(format!(
                                    "{}\n{}\n{}",
                                    session.title,
                                    session.session_key,
                                    if session.open {
                                        "Window visible"
                                    } else {
                                        "Window hidden"
                                    }
                                ));
                        if response.clicked() {
                            focus_session_key = Some(session.session_key.clone());
                        }
                        response.context_menu(|ui| {
                            if ui
                                .button(format!("{} Rename", regular::PENCIL_SIMPLE))
                                .clicked()
                            {
                                rename_session_key = Some(session.session_key.clone());
                                ui.close();
                            }
                            if ui.button(format!("{} Copy ID", regular::COPY)).clicked() {
                                copy_session_key = Some(session.session_key.clone());
                                ui.close();
                            }
                            if ui
                                .add(Button::new(
                                    RichText::new(format!("{} Delete", regular::TRASH))
                                        .color(ui.visuals().error_fg_color),
                                ))
                                .clicked()
                            {
                                remove_session_key = Some(session.session_key.clone());
                                ui.close();
                            }
                        });
                        ui.add_space(4.0);
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
        if let Some(session_key) = copy_session_key {
            ctx.output_mut(|o| {
                o.commands
                    .push(egui::OutputCommand::CopyText(session_key.clone()));
            });
            self.toasts.borrow_mut().success("Agent ID copied");
        }
        if let Some(session_key) = remove_session_key {
            self.delete_session_key = Some(session_key);
        }
        if create_session {
            self.create_session();
        }
    }

    fn render_rename_dialog(&mut self, ctx: &Context) {
        let Some(session_key) = self.rename_session_key.clone() else {
            return;
        };

        let mut open = true;
        let mut submit = false;
        let mut cancel = false;

        egui::Window::new("Rename Agent")
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                let response = ui.add(
                    TextEdit::singleline(&mut self.rename_session_input)
                        .desired_width(f32::INFINITY)
                        .hint_text("Agent name"),
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
            let trimmed = self.rename_session_input.trim().to_string();
            if !trimmed.is_empty() {
                self.rename_session(&session_key, &trimmed);
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
                        self.persist_workspace_state();
                    }
                });
            });

        self.show_gateway_dialog = open;

        if reconnect_all {
            self.gateway_token = normalize_gateway_token_input(&self.gateway_token_input);
            self.persist_workspace_state();
            self.reconnect_all_sessions();
            self.show_gateway_dialog = false;
        }
    }

    fn render_delete_dialog(&mut self, ctx: &Context) {
        let Some(session_key) = self.delete_session_key.clone() else {
            return;
        };
        let Some(index) = self.session_index(&session_key) else {
            self.delete_session_key = None;
            return;
        };
        let session_title = self.sessions[index].title.clone();

        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;

        egui::Window::new("Delete Agent")
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(380.0);
                ui.label(delete_confirmation_body(&session_title));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(Button::new(
                            RichText::new("Delete").color(ui.visuals().error_fg_color),
                        ))
                        .clicked()
                    {
                        confirm = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if confirm {
            self.delete_session(&session_key);
            self.delete_session_key = None;
            return;
        }

        if cancel || !open {
            self.delete_session_key = None;
        }
    }

    fn render_session_window(&mut self, ctx: &Context, session_key: &str) {
        let Some(index) = self.session_index(session_key) else {
            return;
        };

        let mut trigger_send = false;
        let mut set_active = false;
        {
            let session = &mut self.sessions[index];
            let messages = session.buffers.messages.borrow().clone();
            let mut open = session.open;

            let window = egui::Window::new(&session.title)
                .id(session_window_id(&session.session_key))
                .default_pos(session.window_anchor.to_pos2())
                .default_size([SESSION_WINDOW_DEFAULT_WIDTH, SESSION_WINDOW_DEFAULT_HEIGHT])
                .collapsible(false)
                .min_width(SESSION_WINDOW_MIN_WIDTH)
                .min_height(SESSION_WINDOW_MIN_HEIGHT)
                .open(&mut open);

            if let Some(inner) = window.show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&session.session_key).small().weak());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(self.connection_state.borrow().status_text());
                    });
                });
                ui.separator();

                let messages_height = (ui.available_height() - INPUT_PANEL_HEIGHT).max(140.0);
                ui.allocate_ui(vec2(ui.available_width(), messages_height), |ui| {
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            if messages.is_empty() {
                                render_empty_state(ui, &self.connection_state.borrow());
                                return;
                            }

                            for message in &messages {
                                render_message(ui, &mut session.markdown_cache, message);
                                ui.add_space(8.0);
                            }
                        });
                });

                ui.separator();
                ui.vertical(|ui| {
                    let input = TextEdit::multiline(&mut session.draft)
                        .desired_rows(3)
                        .hint_text(self.connection_state.borrow().composer_hint_text())
                        .interactive(self.connection_state.borrow().can_send());
                    let response = ui.add_sized([ui.available_width(), 72.0], input);

                    let send_on_enter = response.has_focus()
                        && ui.input(|input| {
                            input.key_pressed(Key::Enter) && !input.modifiers.command
                        });
                    let insert_newline = response.has_focus()
                        && ui.input(|input| {
                            input.key_pressed(Key::Enter) && input.modifiers.command
                        });

                    if insert_newline {
                        session.draft.push('\n');
                    }

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let helper_text = if self.connection_state.borrow().can_send() {
                            "Enter to send, Cmd+Enter for newline"
                        } else {
                            self.connection_state.borrow().composer_hint_text()
                        };
                        ui.label(RichText::new(helper_text).small().weak());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let send_button = ui.add_enabled(
                                self.connection_state.borrow().can_send(),
                                Button::new(format!("{} Send", regular::PAPER_PLANE)),
                            );
                            if send_button.clicked() || send_on_enter {
                                trigger_send = true;
                            }
                        });
                    });
                });
            }) {
                set_active = should_activate_session_window(
                    inner.response.contains_pointer(),
                    ctx.input(|input| input.pointer.primary_pressed()),
                );
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
        if trigger_send {
            self.send_session_draft(session_key);
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
        let page_mode = {
            let connection_state = self.connection_state.borrow().clone();
            derive_page_mode(&connection_state, self.workspace_loaded)
        };
        match page_mode {
            PageMode::ConnectionGuide => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() / 2.0 - 140.0);
                        if let Some(origin) = &self.gateway_origin {
                            ui.add(
                                Image::from_uri(format!("{origin}/images/crab.png"))
                                    .max_size(vec2(120.0, 120.0)),
                            );
                        }
                        ui.add_space(16.0);
                        ui.heading("Connect to Klaw Gateway");
                        ui.add_space(4.0);
                        ui.label("Connect successfully before loading agents.");
                        ui.add_space(12.0);
                        if ui.button("Connect").clicked() {
                            self.request_workspace_connection();
                        }
                    });
                });
                return;
            }
            PageMode::LoadingWorkspace => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label("Loading agents from Klaw gateway…");
                    });
                });
                return;
            }
            PageMode::Workspace => {}
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.sessions.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("No agents yet. Click New Agent to start.");
                });
                return;
            }

            ui.label(RichText::new("Agent Workspace").strong());
            ui.label(
                RichText::new("Each agent opens as its own egui window.")
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
        self.process_pending_frames();
        self.render_top_bar(ctx);
        self.render_status_bar(ctx);
        self.render_session_list(ctx);
        self.render_workbench(ctx);
        self.render_gateway_dialog(ctx);
        self.render_rename_dialog(ctx);
        self.render_delete_dialog(ctx);
        self.toasts.borrow_mut().show(ctx);
    }
}

/// Inner max width for a user bubble: shrink-wrap short plain text, cap at [`BUBBLE_MAX_WIDTH`].
fn user_bubble_inner_max_width(
    ui: &egui::Ui,
    message: &ChatMessage,
    role_label: &str,
    time_label: &str,
) -> f32 {
    const SLACK: f32 = 8.0;
    const MIN_INNER: f32 = 72.0;

    let header_w = WidgetText::from(RichText::new(format!("{role_label}  {time_label}")).strong())
        .into_galley(ui, None, f32::INFINITY, TextStyle::Body)
        .size()
        .x;

    let body_w = {
        let t = message.text.as_str();
        let looks_structured = t.contains("```")
            || t.contains('\n')
            || t.trim_start().starts_with('#')
            || t.contains("**");
        if looks_structured {
            BUBBLE_MAX_WIDTH
        } else {
            WidgetText::from(RichText::new(t))
                .into_galley(ui, None, f32::INFINITY, TextStyle::Body)
                .size()
                .x
        }
    };

    (header_w.max(body_w) + SLACK).clamp(MIN_INNER, BUBBLE_MAX_WIDTH)
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

fn render_message(ui: &mut egui::Ui, markdown_cache: &mut MarkdownCache, message: &ChatMessage) {
    let time_label = format_message_timestamp(message.timestamp_ms);
    match message.role {
        MessageRole::System => {
            ui.vertical_centered(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("System").small().strong().weak());
                    ui.label(RichText::new(time_label).small().weak());
                });
                render_plain_message(ui, &message.text, ui.visuals().weak_text_color());
            });
        }
        MessageRole::Assistant | MessageRole::User => {
            let role_label = match message.role {
                MessageRole::Assistant => "Klaw",
                MessageRole::User => "You",
                MessageRole::System => "System",
            };
            let dark_mode = ui.visuals().dark_mode;
            let is_user = matches!(message.role, MessageRole::User);
            let (bubble_fill, bubble_stroke, heading_color, body_color, link_color) =
                match message.role {
                    MessageRole::User if dark_mode => (
                        Color32::from_rgb(49, 102, 214),
                        Stroke::new(1.0, Color32::from_rgb(96, 145, 245)),
                        Color32::WHITE,
                        Color32::WHITE,
                        Color32::from_rgb(219, 233, 255),
                    ),
                    MessageRole::User => (
                        Color32::from_rgb(229, 239, 255),
                        Stroke::new(1.0, Color32::from_rgb(170, 196, 250)),
                        Color32::from_rgb(24, 55, 124),
                        Color32::from_rgb(32, 43, 67),
                        Color32::from_rgb(20, 83, 181),
                    ),
                    _ => (
                        ui.visuals().widgets.noninteractive.bg_fill,
                        ui.visuals().widgets.noninteractive.bg_stroke,
                        ui.visuals().strong_text_color(),
                        ui.visuals().text_color(),
                        ui.visuals().hyperlink_color,
                    ),
                };

            let inner_w_user = if is_user {
                Some(user_bubble_inner_max_width(
                    ui,
                    message,
                    role_label,
                    &time_label,
                ))
            } else {
                None
            };
            let mut show_bubble = |ui: &mut egui::Ui, inner_max_width: f32| {
                Frame::group(ui.style())
                    .fill(bubble_fill)
                    .stroke(bubble_stroke)
                    .inner_margin(if is_user { 10.0 } else { 8.0 })
                    .outer_margin(2.0)
                    .corner_radius(if is_user { 12.0 } else { 6.0 })
                    .show(ui, |ui| {
                        ui.set_max_width(inner_max_width);
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(role_label).strong().color(heading_color));
                                ui.add_space(6.0);
                                ui.label(RichText::new(&time_label).small().color(heading_color));
                            });
                            ui.add_space(4.0);
                            render_markdown(
                                ui,
                                markdown_cache,
                                &message.text,
                                body_color,
                                link_color,
                            );
                        });
                    });
            };

            if let Some(inner_w) = inner_w_user {
                let row_w = ui.available_width();
                ui.allocate_ui_with_layout(
                    vec2(row_w, 0.0),
                    Layout::right_to_left(Align::TOP),
                    |ui| {
                        ui.allocate_ui(vec2(inner_w.min(ui.available_width()), 0.0), |ui| {
                            show_bubble(ui, inner_w);
                        });
                        ui.add_space(ui.available_width());
                    },
                );
            } else {
                show_bubble(ui, BUBBLE_MAX_WIDTH);
            }
        }
    }
}

fn compact_sidebar_title(title: &str) -> String {
    const MAX_CHARS: usize = 24;
    let count = title.chars().count();
    if count <= MAX_CHARS {
        return title.to_string();
    }

    let shortened = title.chars().take(MAX_CHARS - 1).collect::<String>();
    format!("{shortened}…")
}

fn theme_preference_label(theme: egui::ThemePreference) -> &'static str {
    match theme {
        egui::ThemePreference::System => "System",
        egui::ThemePreference::Light => "Light",
        egui::ThemePreference::Dark => "Dark",
    }
}
