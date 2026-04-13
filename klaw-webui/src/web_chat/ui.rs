use eframe::egui::{
    self, Align, Align2, Button, Color32, ComboBox, Context, Frame, Image, Key, Layout, RichText,
    ScrollArea, Stroke, TextEdit, TextStyle, TopBottomPanel, WidgetText,
    text_edit::TextEditState, vec2,
};
use egui_phosphor::regular;
use klaw_ui_kit::{ThemeSwitch, text_animator::TextAnimator};
use klaw_ui_kit::toggle::toggle;
use std::collections::HashMap;

use crate::{
    ActiveSlashCommand, ConnectionState, MessageRole, PageMode, SlashCommandCompletion,
    apply_slash_completion, attachment_action_in_progress, can_trigger_file_picker,
    connection_action_label, delete_confirmation_body, derive_page_mode,
    detect_active_slash_command, normalize_gateway_token_input,
    should_activate_session_window, slash_command_matches,
};

use super::{
    app::ChatApp,
    markdown::{MarkdownCache, render_markdown, render_plain_message},
    session::{
        BUBBLE_MAX_WIDTH, ChatMessage, INPUT_PANEL_HEIGHT, SESSION_LIST_WIDTH, SessionWindow,
        SESSION_WINDOW_DEFAULT_HEIGHT, SESSION_WINDOW_DEFAULT_WIDTH, SESSION_WINDOW_MIN_HEIGHT,
        SESSION_WINDOW_MIN_WIDTH, current_timestamp_ms, format_datetime, format_message_timestamp,
        format_relative_time, session_window_id,
    },
};

const ABOUT_GITHUB_URL: &str = "https://github.com/zhubby/klaw";

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

                ui.menu_button(format!("{} Help", regular::QUESTION), |ui| {
                    if ui.button(format!("{} About", regular::INFO)).clicked() {
                        self.show_about_dialog = true;
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
        let mut stream_changed = false;
        let open_sessions = self.sessions.iter().filter(|session| session.open).count();
        TopBottomPanel::bottom("klaw-webui-status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Theme Mode:");
                let current_theme = self.ctx.options(|opt| opt.theme_preference);
                let mut next_theme = current_theme;
                if ui.add(ThemeSwitch::new(&mut next_theme)).changed() {
                    requested_theme = Some(next_theme);
                }
                ui.separator();
                ui.label(format!("Agents: {}/{}", self.sessions.len(), open_sessions))
                    .on_hover_text("Total agent windows / currently open windows.");
                ui.separator();
                ui.label("Stream");
                let response = ui.add(toggle(&mut self.stream_enabled)).on_hover_text(
                    "On: stream replies live. Off: wait for a full reply and play fade-in.",
                );
                if response.changed() {
                    stream_changed = true;
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let fps = live_fps(ctx);
                    ui.label(RichText::new(format!("{fps:.0} FPS")).small().weak())
                        .on_hover_text("Approximate live frame rate from the latest egui frame delta.");
                    if let Some(session) = self.active_session() {
                        ui.separator();
                        if let Some(activity) = session_activity_label(session) {
                            ui.label(RichText::new(activity).small().weak())
                                .on_hover_text("Current activity for the active agent.");
                            ui.separator();
                        }

                        let message_count = session.buffers.messages.borrow().len();
                        ui.label(RichText::new(format!("{message_count} msgs")).small().weak())
                            .on_hover_text("Messages currently loaded in the active agent window.");
                        ui.separator();

                        let route = session_route_label(session);
                        ui.label(RichText::new(compact_status_text(&route, 28)).small().weak())
                            .on_hover_text(route);
                        ui.separator();

                        ui.label(compact_status_text(&session.title, 24))
                            .on_hover_text(&session.title);
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
        if stream_changed {
            self.persist_workspace_state();
        }
    }

    fn render_about_dialog(&mut self, ctx: &Context) {
        if !self.show_about_dialog {
            return;
        }

        let mut open = self.show_about_dialog;
        let mut close_requested = false;
        egui::Window::new("About Klaw")
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);
                    ui.label(RichText::new("Klaw").strong().size(22.0));
                    ui.add_space(18.0);

                    if let Some(origin) = &self.gateway_origin {
                        ui.add(
                            Image::from_uri(format!("{origin}/images/crab.png"))
                                .max_size(vec2(160.0, 160.0)),
                        );
                        ui.add_space(12.0);
                    }

                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.add_space(4.0);
                    ui.hyperlink_to(ABOUT_GITHUB_URL, ABOUT_GITHUB_URL);
                    ui.add_space(12.0);

                    if ui.button("Close").clicked() {
                        close_requested = true;
                    }
                });
            });
        self.show_about_dialog = open && !close_requested;
    }

    fn active_session(&self) -> Option<&SessionWindow> {
        let active_session_key = self.active_session_key.as_deref()?;
        self.sessions
            .iter()
            .find(|session| session.session_key == active_session_key)
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
                        let is_open = session.open;
                        let now_ms = current_timestamp_ms();
                        let relative_time = format_relative_time(session.created_at_ms, now_ms);
                        let compact_title = compact_sidebar_title(&session.title);
                        let card = Frame::group(ui.style())
                            .fill(if is_open {
                                ui.visuals().faint_bg_color
                            } else {
                                ui.visuals().widgets.noninteractive.bg_fill
                            })
                            .stroke(if is_open {
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
        let mut trigger_file_picker = false;
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
                        ui.label(
                            RichText::new(format_datetime(session.created_at_ms))
                                .small()
                                .weak(),
                        );
                    });
                });
                ui.separator();

                let messages_height = (ui.available_height() - INPUT_PANEL_HEIGHT).max(140.0);
                ui.allocate_ui(vec2(ui.available_width(), messages_height), |ui| {
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            if *session.buffers.history_loading.borrow() && messages.is_empty() {
                                render_history_loading_state(ui);
                                return;
                            }
                            if messages.is_empty() {
                                render_empty_state(ui, &self.connection_state.borrow());
                                return;
                            }

                            session.prune_finished_animations();
                            for message in &messages {
                                render_message(
                                    ui,
                                    &mut session.markdown_cache,
                                    &mut session.fade_in_messages,
                                    message,
                                );
                                ui.add_space(8.0);
                            }
                        });
                });

                ui.separator();
                ui.vertical(|ui| {
                    let selecting_file = *session.selecting_file.borrow();
                    let uploading_file = *session.uploading_file.borrow();
                    let attachment_busy =
                        attachment_action_in_progress(selecting_file, uploading_file);
                    let can_send = self.connection_state.borrow().can_send();
                    ui.label(
                        RichText::new("Type / to open command completion.")
                            .small()
                            .weak(),
                    );
                    ui.add_space(4.0);
                    let previous_slash_state = session.slash_completer.clone();
                    let mut input_output = ui
                        .allocate_ui_with_layout(
                            vec2(ui.available_width(), 80.0),
                            Layout::left_to_right(Align::Min),
                            |ui| {
                                TextEdit::multiline(&mut session.draft)
                                    .desired_rows(4)
                                    .desired_width(ui.available_width())
                                    .hint_text(self.connection_state.borrow().composer_hint_text())
                                    .interactive(can_send && !attachment_busy)
                                    .show(ui)
                            },
                        )
                        .inner;
                    let response = &input_output.response;

                    let raw_slash_trigger = input_output.cursor_range.and_then(|cursor_range| {
                        detect_active_slash_command(&session.draft, cursor_range.primary.index)
                    });
                    let slash_trigger = raw_slash_trigger.as_ref().and_then(|trigger| {
                        let dismissed = session.slash_completer.dismissed_query.as_deref()
                            == Some(trigger.query.as_str())
                            && session.slash_completer.dismissed_start
                                == Some(trigger.replace_range.start);
                        (!dismissed).then_some(trigger.clone())
                    });
                    let slash_matches =
                        slash_trigger.as_ref().map(|trigger| slash_command_matches(&trigger.query));
                    if let Some(trigger) = slash_trigger.as_ref() {
                        update_slash_selection_state(
                            &mut session.slash_completer,
                            &trigger.query,
                            trigger.replace_range.clone(),
                            slash_matches.as_ref().map_or(0, Vec::len),
                        );
                    } else if raw_slash_trigger.is_none() {
                        session.slash_completer.dismissed_query = None;
                        session.slash_completer.dismissed_start = None;
                        session.slash_completer.selected_index = 0;
                        session.slash_completer.last_query.clear();
                        session.slash_completer.replace_range = None;
                    } else {
                        session.slash_completer.selected_index = 0;
                        session.slash_completer.last_query.clear();
                        session.slash_completer.replace_range = None;
                    }

                    let mut slash_completion_accepted = false;
                    let complete_on_enter = response.has_focus()
                        && ui.input(|input| input.key_pressed(Key::Enter) && !input.modifiers.command)
                        && slash_trigger.is_none()
                        && previous_slash_state.replace_range.is_some();
                    let insert_newline = response.has_focus()
                        && ui.input(|input| {
                            input.key_pressed(Key::Enter) && input.modifiers.command
                        });

                    if insert_newline {
                        session.draft.push('\n');
                    }

                    if response.has_focus() {
                        if let (Some(trigger), Some(matches)) =
                            (slash_trigger.as_ref(), slash_matches.as_ref())
                        {
                            let popup_pos = input_output
                                .cursor_range
                                .map(|cursor_range| {
                                    let cursor_rect =
                                        input_output.galley.pos_from_cursor(cursor_range.primary);
                                    response.rect.min
                                        + cursor_rect.left_bottom().to_vec2()
                                        + vec2(0.0, 6.0)
                                })
                                .unwrap_or_else(|| {
                                    egui::pos2(response.rect.left(), response.rect.bottom() + 4.0)
                                });
                            slash_completion_accepted = handle_slash_completion_keyboard(
                                ui,
                                &mut session.draft,
                                trigger,
                                matches,
                                &mut session.slash_completer.selected_index,
                                &mut input_output.state,
                                response.id,
                            );
                            if slash_completion_accepted {
                                clear_slash_completion_state(
                                    &mut session.slash_completer,
                                    Some(trigger),
                                );
                            }
                            if !slash_completion_accepted
                                && render_slash_completion_popup(
                                    ui,
                                    popup_pos,
                                    response.id,
                                    response.rect.width(),
                                    &mut session.draft,
                                    trigger,
                                    matches,
                                    &mut session.slash_completer.selected_index,
                                    &mut input_output.state,
                                )
                            {
                                clear_slash_completion_state(
                                    &mut session.slash_completer,
                                    Some(trigger),
                                );
                                ui.ctx().request_repaint();
                            }
                        } else if complete_on_enter
                            && let Some(replace_range) =
                                previous_slash_state.replace_range.clone()
                        {
                            let matches = slash_command_matches(&previous_slash_state.last_query);
                            if let Some(completion) = matches
                                .get(
                                    previous_slash_state
                                        .selected_index
                                        .min(matches.len().saturating_sub(1)),
                                )
                                .copied()
                            {
                                if session.draft[replace_range.end..].starts_with('\n') {
                                    session
                                        .draft
                                        .replace_range(replace_range.end..replace_range.end + 1, "");
                                }
                                apply_slash_completion_selection(
                                    &mut session.draft,
                                    &ActiveSlashCommand {
                                        replace_range: replace_range.clone(),
                                        query: previous_slash_state.last_query.clone(),
                                    },
                                    completion,
                                    &mut input_output.state,
                                    response.id,
                                    ui.ctx(),
                                );
                                clear_slash_completion_state(
                                    &mut session.slash_completer,
                                    Some(&ActiveSlashCommand {
                                        replace_range,
                                        query: previous_slash_state.last_query.clone(),
                                    }),
                                );
                                slash_completion_accepted = true;
                            }
                        }
                    }

                    let send_on_enter = !slash_completion_accepted
                        && response.has_focus()
                        && ui.input(|input| {
                            input.key_pressed(Key::Enter) && !input.modifiers.command
                        });

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let provider_width = 140.0 * 2.0 / 3.0;
                        let model_width = 180.0 * 2.0 / 3.0;
                        let control_height = ui.spacing().interact_size.y;
                        if ui
                            .add_enabled(
                                can_trigger_file_picker(can_send, selecting_file, uploading_file),
                                egui::Button::new(regular::PAPERCLIP).small(),
                            )
                            .on_hover_text("Attach file")
                            .clicked()
                        {
                            trigger_file_picker = true;
                        }

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let send_button = ui.add_enabled(
                                can_send && !attachment_busy,
                                Button::new(format!("{} Send", regular::PAPER_PLANE)),
                            );
                            if send_button.clicked() || send_on_enter {
                                trigger_send = true;
                            }

                            ui.add_space(6.0);
                            ui.add_enabled_ui(can_send && !attachment_busy, |ui| {
                                ui.add_sized(
                                    [model_width, control_height],
                                    TextEdit::singleline(&mut session.selected_route.model)
                                        .hint_text("Model"),
                                );
                                let provider_changed =
                                    ComboBox::from_id_salt(("session-model-provider", session_key))
                                        .width(provider_width)
                                        .selected_text(
                                            if session.selected_route.model_provider.is_empty() {
                                                "Provider".to_string()
                                            } else {
                                                session.selected_route.model_provider.clone()
                                            },
                                        )
                                        .show_ui(ui, |ui| {
                                            let mut changed = false;
                                            for provider in &self.provider_catalog.providers {
                                                changed |= ui
                                                    .selectable_value(
                                                        &mut session.selected_route.model_provider,
                                                        provider.id.clone(),
                                                        &provider.id,
                                                    )
                                                    .changed();
                                            }
                                            changed
                                        })
                                        .inner
                                        .unwrap_or(false);
                                if provider_changed {
                                    session.reset_selected_model_to_provider_default(
                                        &self.provider_catalog,
                                    );
                                }
                            });

                            if session.selected_archive_id.borrow().is_some() {
                                ui.label(
                                    RichText::new(format!("{} File attached", regular::CHECK))
                                        .small()
                                        .color(ui.visuals().hyperlink_color),
                                );
                                if ui.small_button("✕").on_hover_text("Remove file").clicked() {
                                    *session.selected_archive_id.borrow_mut() = None;
                                }
                            } else if selecting_file {
                                ui.spinner();
                                ui.label(RichText::new("Selecting file...").small().weak());
                            } else if uploading_file {
                                ui.spinner();
                                ui.label(RichText::new("Uploading...").small().weak());
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
        if trigger_file_picker {
            self.trigger_file_picker(session_key);
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
        self.render_about_dialog(ctx);
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

fn render_history_loading_state(ui: &mut egui::Ui) {
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.spinner();
        ui.add_space(8.0);
        ui.label(
            RichText::new("Loading conversation history…")
                .heading()
                .strong(),
        );
        ui.add_space(4.0);
        ui.label(RichText::new("Fetching messages from Klaw gateway.").weak());
    });
}

fn render_message(
    ui: &mut egui::Ui,
    markdown_cache: &mut MarkdownCache,
    fade_in_messages: &mut HashMap<String, TextAnimator>,
    message: &ChatMessage,
) {
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
                            render_message_body(
                                ui,
                                markdown_cache,
                                fade_in_messages,
                                message,
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

fn render_message_body(
    ui: &mut egui::Ui,
    markdown_cache: &mut MarkdownCache,
    fade_in_messages: &mut HashMap<String, TextAnimator>,
    message: &ChatMessage,
    body_color: Color32,
    link_color: Color32,
) {
    let mut should_remove_animation = false;
    if matches!(message.role, MessageRole::Assistant)
        && let Some(animator) = fade_in_messages.get_mut(&message.id)
    {
        animator.font = TextStyle::Body.resolve(ui.style());
        animator.color = body_color;
        animator.process_animation(ui.ctx());
        animator.render(ui);
        if animator.is_animation_finished() {
            should_remove_animation = true;
        } else {
            ui.ctx().request_repaint();
        }
    } else {
        render_markdown(ui, markdown_cache, &message.text, body_color, link_color);
    }

    if should_remove_animation {
        fade_in_messages.remove(&message.id);
    }
}

fn compact_sidebar_title(title: &str) -> String {
    const MAX_CHARS: usize = 24;
    compact_status_text(title, MAX_CHARS)
}

fn compact_status_text(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }

    let shortened = text.chars().take(max_chars - 1).collect::<String>();
    format!("{shortened}…")
}

fn update_slash_selection_state(
    state: &mut super::session::SlashCompleterState,
    query: &str,
    replace_range: std::ops::Range<usize>,
    match_count: usize,
) {
    if state.last_query != query {
        state.last_query = query.to_string();
        state.selected_index = 0;
    }
    state.replace_range = Some(replace_range);
    if match_count == 0 {
        state.selected_index = 0;
    } else {
        state.selected_index = state.selected_index.min(match_count - 1);
    }
}

fn clear_slash_completion_state(
    state: &mut super::session::SlashCompleterState,
    dismissed_trigger: Option<&ActiveSlashCommand>,
) {
    state.selected_index = 0;
    state.last_query.clear();
    state.replace_range = None;
    state.dismissed_query = dismissed_trigger.map(|trigger| trigger.query.clone());
    state.dismissed_start = dismissed_trigger.map(|trigger| trigger.replace_range.start);
}

fn handle_slash_completion_keyboard(
    ui: &egui::Ui,
    draft: &mut String,
    trigger: &ActiveSlashCommand,
    matches: &[SlashCommandCompletion],
    selected_index: &mut usize,
    text_edit_state: &mut TextEditState,
    text_edit_id: egui::Id,
) -> bool {
    if matches.is_empty() {
        return ui.input(|input| input.key_pressed(Key::Escape));
    }

    if ui.input(|input| input.key_pressed(Key::Escape)) {
        return true;
    }
    if ui.input(|input| input.key_pressed(Key::ArrowDown)) {
        *selected_index = (*selected_index + 1) % matches.len();
    }
    if ui.input(|input| input.key_pressed(Key::ArrowUp)) {
        *selected_index = if *selected_index == 0 {
            matches.len() - 1
        } else {
            *selected_index - 1
        };
    }
    if ui.input(|input| input.key_pressed(Key::Tab) || input.key_pressed(Key::Enter)) {
        apply_slash_completion_selection(
            draft,
            trigger,
            matches[*selected_index],
            text_edit_state,
            text_edit_id,
            ui.ctx(),
        );
        return true;
    }
    false
}

fn render_slash_completion_popup(
    ui: &mut egui::Ui,
    popup_pos: egui::Pos2,
    text_edit_id: egui::Id,
    response_width: f32,
    draft: &mut String,
    trigger: &ActiveSlashCommand,
    matches: &[SlashCommandCompletion],
    selected_index: &mut usize,
    text_edit_state: &mut TextEditState,
) -> bool {
    if matches.is_empty() {
        return false;
    }

    let mut accepted = false;
    let popup_id = text_edit_id.with("slash-completer");
    egui::Area::new(popup_id)
        .order(egui::Order::Foreground)
        .fixed_pos(popup_pos)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(response_width.clamp(240.0, 420.0));
                ui.spacing_mut().item_spacing.y = 2.0;
                for (index, completion) in matches.iter().copied().enumerate() {
                    let selected = *selected_index == index;
                    let row = ui.selectable_label(
                        selected,
                        format!("{:<18} {}", completion.command, completion.description),
                    );
                    if row.hovered() {
                        *selected_index = index;
                    }
                    if row.clicked() {
                        apply_slash_completion_selection(
                            draft,
                            trigger,
                            completion,
                            text_edit_state,
                            text_edit_id,
                            ui.ctx(),
                        );
                        accepted = true;
                    }
                }
            });
        });
    accepted
}

fn apply_slash_completion_selection(
    draft: &mut String,
    trigger: &ActiveSlashCommand,
    completion: SlashCommandCompletion,
    text_edit_state: &mut TextEditState,
    text_edit_id: egui::Id,
    ctx: &Context,
) {
    let cursor_char_index = apply_slash_completion(draft, trigger.replace_range.clone(), completion);
    let cursor = egui::text::CCursor::new(cursor_char_index);
    text_edit_state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(cursor)));
    text_edit_state.clone().store(ctx, text_edit_id);
    ctx.request_repaint();
}

fn live_fps(ctx: &Context) -> f32 {
    let dt = ctx.input(|input| input.unstable_dt);
    if dt.is_finite() && dt > f32::EPSILON {
        1.0 / dt
    } else {
        0.0
    }
}

fn session_route_label(session: &SessionWindow) -> String {
    let provider = session.selected_route.model_provider.trim();
    let model = session.selected_route.model.trim();
    match (provider.is_empty(), model.is_empty()) {
        (true, true) => "Route: default".to_string(),
        (false, true) => format!("Route: {provider}"),
        (true, false) => format!("Route: {model}"),
        (false, false) => format!("Route: {provider}/{model}"),
    }
}

fn session_activity_label(session: &SessionWindow) -> Option<&'static str> {
    if *session.buffers.history_loading.borrow() {
        Some("History")
    } else if *session.uploading_file.borrow() {
        Some("Uploading")
    } else if *session.selecting_file.borrow() {
        Some("Picking File")
    } else if session
        .buffers
        .active_stream_request_id
        .borrow()
        .as_deref()
        .is_some()
    {
        Some("Streaming")
    } else if session.selected_archive_id.borrow().as_deref().is_some() {
        Some("File Attached")
    } else {
        None
    }
}
