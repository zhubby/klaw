use eframe::egui::{
    self, Align, Align2, Button, Color32, ComboBox, Context, Frame, Image, Key, Layout, RichText,
    ScrollArea, Stroke, TextEdit, TextStyle, TopBottomPanel, WidgetText, text_edit::TextEditState,
    vec2,
};
use egui_extras::{Column, Size, StripBuilder, TableBuilder};
use egui_phosphor::regular;
use klaw_ui_kit::toggle::toggle;
use klaw_ui_kit::{
    DarkThemePreset, LightThemePreset, LocaleDomain, ThemeMode, ThemeSwitch, Translator,
    UiLanguage, text_animator::TextAnimator, theme_mode_from_preference,
};
use std::collections::{BTreeMap, HashMap};

use crate::{
    ActiveSlashCommand, ConnectionState, ImCard, ImCardAction, ImCardActionKind, ImCardKind,
    MessageRole, PageMode, SlashCommandCompletion, WebArchiveAttachment, WebArchiveResource,
    apply_slash_completion, archive_resource_card_action, archive_resource_is_image,
    archive_resource_is_previewable, attachment_action_in_progress, can_trigger_file_picker,
    content_type_is_image, content_type_is_text, delete_confirmation_body, derive_page_mode,
    detect_active_slash_command, has_exact_slash_command_match, normalize_gateway_token_input,
    resolve_assistant_bubble_palette, resolve_im_card_palette, should_activate_session_window,
    should_show_thinking_placeholder, slash_command_matches,
};

use super::{
    app::{
        ArchivePreviewDialog, ArchivePreviewStatus, ChatApp, web_archive_resource_from_attachment,
    },
    markdown::{MarkdownCache, render_markdown, render_plain_message, render_scrollable_markdown},
    session::{
        BUBBLE_MAX_WIDTH, CardInteractionState, ChatMessage, INPUT_PANEL_HEIGHT,
        PendingHistoryScrollRestore, SESSION_LIST_WIDTH, SESSION_WINDOW_DEFAULT_HEIGHT,
        SESSION_WINDOW_DEFAULT_WIDTH, SESSION_WINDOW_MIN_HEIGHT, SESSION_WINDOW_MIN_WIDTH,
        SessionWindow, current_timestamp_ms, format_datetime, format_message_timestamp,
        format_relative_time, session_window_id,
    },
};

const ABOUT_GITHUB_URL: &str = "https://github.com/zhubby/klaw";

fn rgb(color: [u8; 3]) -> Color32 {
    Color32::from_rgb(color[0], color[1], color[2])
}

impl ChatApp {
    fn render_top_bar(&mut self, ctx: &Context) {
        let translator = Translator::new(LocaleDomain::WebUi, self.ui_language);
        TopBottomPanel::top("klaw-webui-toolbar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(
                    format!("{} {}", regular::FILE, translator.text("menu-file")),
                    |ui| {
                        if ui
                            .button(format!(
                                "{} {}",
                                regular::GEAR,
                                translator.text("menu-settings")
                            ))
                            .clicked()
                        {
                            self.show_settings_dialog = true;
                            ui.close();
                        }
                    },
                );

                ui.menu_button(
                    format!("{} {}", regular::APP_WINDOW, translator.text("menu-window")),
                    |ui| {
                        if ui
                            .button(format!(
                                "{} {}",
                                regular::APP_WINDOW,
                                translator.text("menu-tile-windows")
                            ))
                            .clicked()
                        {
                            self.tile_open_sessions();
                            ui.close();
                        }
                        if ui
                            .button(format!(
                                "{} {}",
                                regular::ARROWS_OUT,
                                translator.text("menu-reset-layout")
                            ))
                            .clicked()
                        {
                            self.reset_window_layout();
                            ui.close();
                        }
                    },
                );

                ui.menu_button(
                    format!("{} {}", regular::PLUG, translator.text("menu-connection")),
                    |ui| {
                        let connection_action = match &*self.connection_state.borrow() {
                            ConnectionState::Connected | ConnectionState::Error(_) => {
                                translator.text("menu-reconnect")
                            }
                            ConnectionState::Connecting | ConnectionState::Disconnected => {
                                translator.text("menu-connect")
                            }
                        };
                        if ui
                            .button(format!(
                                "{} {}",
                                regular::KEY,
                                translator.text("menu-gateway-token")
                            ))
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
                                Button::new(format!(
                                    "{} {}",
                                    regular::SIGN_OUT,
                                    translator.text("menu-disconnect")
                                )),
                            )
                            .clicked()
                        {
                            self.disconnect_and_clear_token();
                            ui.close();
                        }
                    },
                );

                ui.menu_button(
                    format!("{} {}", regular::QUESTION, translator.text("menu-help")),
                    |ui| {
                        if ui
                            .button(format!(
                                "{} {}",
                                regular::INFO,
                                translator.text("menu-about")
                            ))
                            .clicked()
                        {
                            self.show_about_dialog = true;
                            ui.close();
                        }
                    },
                );

                let row_height = ui.spacing().interact_size.y;
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), row_height),
                    Layout::right_to_left(Align::Center),
                    |ui| {
                        let state = self.connection_state.borrow().clone();
                        let (dot, label) = match state {
                            ConnectionState::Connected => (
                                Color32::from_rgb(41, 163, 90),
                                translator.text("status-connected"),
                            ),
                            ConnectionState::Connecting => (
                                Color32::from_rgb(214, 149, 33),
                                translator.text("status-connecting"),
                            ),
                            ConnectionState::Disconnected => (
                                Color32::from_rgb(208, 67, 67),
                                translator.text("status-disconnected"),
                            ),
                            ConnectionState::Error(_) => (
                                Color32::from_rgb(208, 67, 67),
                                translator.text("status-error"),
                            ),
                        };
                        ui.label(RichText::new(label).small().strong());
                        ui.label(RichText::new("●").color(dot));
                    },
                );
            });
        });
    }

    fn render_status_bar(&mut self, ctx: &Context) {
        let translator = Translator::new(LocaleDomain::WebUi, self.ui_language);
        let mut requested_theme = None;
        let mut stream_changed = false;
        let open_sessions = self.sessions.iter().filter(|session| session.open).count();
        TopBottomPanel::bottom("klaw-webui-status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("{}:", translator.text("statusbar-theme-mode")));
                let current_theme = self.ctx.options(|opt| opt.theme_preference);
                let mut next_theme = current_theme;
                if ui.add(ThemeSwitch::new(&mut next_theme)).changed() {
                    requested_theme = Some(theme_mode_from_preference(next_theme));
                }
                ui.separator();
                ui.label(format!("{}:", translator.text("language")));
                if let Some(language) =
                    render_language_combo(ui, "webui-status-language", self.ui_language, 120.0)
                {
                    self.set_ui_language(language);
                }
                ui.separator();
                ui.label(translator.text_args(
                    "statusbar-agents",
                    HashMap::from([
                        ("total", self.sessions.len().to_string()),
                        ("open", open_sessions.to_string()),
                    ]),
                ))
                .on_hover_text(translator.text("statusbar-agents-hover"));
                ui.separator();
                ui.label(translator.text("statusbar-stream"));
                let response = ui
                    .add(toggle(&mut self.stream_enabled))
                    .on_hover_text(translator.text("statusbar-stream-on-hover"));
                if response.changed() {
                    stream_changed = true;
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    const FPS_STATUS_WIDTH: f32 = 56.0;
                    let fps = live_fps(ctx);
                    ui.add_sized(
                        [FPS_STATUS_WIDTH, ui.spacing().interact_size.y],
                        egui::Label::new(RichText::new(format!("{fps:.0} FPS")).small().weak())
                            .sense(egui::Sense::hover()),
                    )
                    .on_hover_text(translator.text("statusbar-fps-hover"));
                    if let Some(session) = self.active_session() {
                        ui.separator();
                        if let Some(activity) = session_activity_label(session, self.ui_language) {
                            ui.label(RichText::new(activity).small().weak())
                                .on_hover_text(translator.text("statusbar-activity-hover"));
                            ui.separator();
                        }

                        let message_count = session.buffers.messages.borrow().len();
                        ui.label(
                            RichText::new(translator.text_args(
                                "statusbar-messages",
                                HashMap::from([("count", message_count.to_string())]),
                            ))
                            .small()
                            .weak(),
                        )
                        .on_hover_text(translator.text("statusbar-messages-hover"));
                        ui.separator();

                        let route = session_route_label(session, self.ui_language);
                        ui.label(
                            RichText::new(compact_status_text(&route, 28))
                                .small()
                                .weak(),
                        )
                        .on_hover_text(route);
                        ui.separator();

                        ui.label(compact_status_text(&session.title, 24))
                            .on_hover_text(&session.title);
                    } else {
                        ui.separator();
                        ui.label(translator.text("statusbar-no-active-agent"));
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

    fn render_settings_dialog(&mut self, ctx: &Context) {
        if !self.show_settings_dialog {
            return;
        }

        let mut open = self.show_settings_dialog;
        let translator = Translator::new(LocaleDomain::WebUi, self.ui_language);
        let mut requested_theme_mode = None;
        let mut requested_light_theme = None;
        let mut requested_dark_theme = None;

        egui::Window::new(translator.text("settings-title"))
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.strong(translator.text("settings-general"));
                ui.add_space(8.0);
                ui.label(translator.text_args(
                    "settings-current-theme-mode",
                    HashMap::from([("mode", self.theme_mode.label().to_string())]),
                ));
                ui.add_space(8.0);

                egui::Grid::new("webui-general-theme-grid")
                    .num_columns(2)
                    .spacing([8.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(format!("{}:", translator.text("language")));
                        if let Some(language) = render_language_combo(
                            ui,
                            "webui-settings-language",
                            self.ui_language,
                            160.0,
                        ) {
                            self.set_ui_language(language);
                        }
                        ui.end_row();

                        ui.label(format!("{}:", translator.text("settings-theme-mode")));
                        ComboBox::from_id_salt("webui-settings-theme-mode")
                            .width(160.0)
                            .selected_text(self.theme_mode.label())
                            .show_ui(ui, |ui| {
                                for theme_mode in
                                    [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark]
                                {
                                    if ui
                                        .selectable_label(
                                            self.theme_mode == theme_mode,
                                            theme_mode.label(),
                                        )
                                        .clicked()
                                    {
                                        requested_theme_mode = Some(theme_mode);
                                        ui.close();
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label(format!("{}:", translator.text("settings-light-theme")));
                        ComboBox::from_id_salt("webui-settings-light-theme")
                            .width(160.0)
                            .selected_text(self.light_theme.label())
                            .show_ui(ui, |ui| {
                                for preset in [
                                    LightThemePreset::Default,
                                    LightThemePreset::Latte,
                                    LightThemePreset::Crab,
                                ] {
                                    if ui
                                        .selectable_label(
                                            self.light_theme == preset,
                                            preset.label(),
                                        )
                                        .clicked()
                                    {
                                        requested_light_theme = Some(preset);
                                        ui.close();
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label(format!("{}:", translator.text("settings-dark-theme")));
                        ComboBox::from_id_salt("webui-settings-dark-theme")
                            .width(160.0)
                            .selected_text(self.dark_theme.label())
                            .show_ui(ui, |ui| {
                                for preset in [
                                    DarkThemePreset::Default,
                                    DarkThemePreset::Frappe,
                                    DarkThemePreset::Macchiato,
                                    DarkThemePreset::Mocha,
                                    DarkThemePreset::Blackpink,
                                ] {
                                    if ui
                                        .selectable_label(self.dark_theme == preset, preset.label())
                                        .clicked()
                                    {
                                        requested_dark_theme = Some(preset);
                                        ui.close();
                                    }
                                }
                            });
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.small(translator.text("settings-theme-default-hint"));
            });

        self.show_settings_dialog = open;

        if let Some(theme_mode) = requested_theme_mode {
            self.set_theme_mode(theme_mode);
        }
        if let Some(light_theme) = requested_light_theme {
            self.set_light_theme(light_theme);
        }
        if let Some(dark_theme) = requested_dark_theme {
            self.set_dark_theme(dark_theme);
        }
    }

    fn render_about_dialog(&mut self, ctx: &Context) {
        if !self.show_about_dialog {
            return;
        }

        let translator = Translator::new(LocaleDomain::WebUi, self.ui_language);
        let mut open = self.show_about_dialog;
        let mut close_requested = false;
        egui::Window::new(translator.text("about-title"))
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(translator.text("assistant-label"))
                            .strong()
                            .size(22.0),
                    );
                    ui.add_space(18.0);

                    if let Some(origin) = &self.gateway_origin {
                        ui.add(
                            Image::from_uri(format!("{origin}/images/crab.png"))
                                .max_size(vec2(160.0, 160.0)),
                        );
                        ui.add_space(12.0);
                    }

                    ui.label(translator.text_args(
                        "about-version",
                        HashMap::from([("version", env!("CARGO_PKG_VERSION").to_string())]),
                    ));
                    ui.add_space(4.0);
                    ui.hyperlink_to(ABOUT_GITHUB_URL, ABOUT_GITHUB_URL);
                    ui.add_space(12.0);

                    if ui.button(translator.text("about-close")).clicked() {
                        close_requested = true;
                    }
                });
            });
        self.show_about_dialog = open && !close_requested;
    }

    fn render_archive_preview_dialog(&mut self, ctx: &Context) {
        let translator = Translator::new(LocaleDomain::WebUi, self.ui_language);
        let Some(dialog) = self.archive_preview.borrow().clone() else {
            return;
        };
        let mut open = true;
        let mut close_requested = false;
        let mut download_requested: Option<WebArchiveResource> = None;
        let default_size = archive_preview_default_size(&dialog);
        egui::Window::new(translator.text("archive-preview-title"))
            .id(egui::Id::new((
                "archive-resource-preview",
                dialog.resource.archive_id.as_str(),
            )))
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .default_size(default_size)
            .min_width(420.0)
            .min_height(320.0)
            .open(&mut open)
            .show(ctx, |ui| {
                render_archive_preview_header(ui, &dialog);
                ui.separator();
                StripBuilder::new(ui)
                    .size(Size::remainder().at_least(180.0))
                    .size(Size::exact(52.0))
                    .vertical(|mut strip| {
                        strip.cell(|ui| {
                            render_archive_preview_content(ui, &dialog, self.ui_language)
                        });
                        strip.cell(|ui| {
                            ui.separator();
                            ui.add_space(8.0);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .button(translator.text("archive-preview-close"))
                                    .clicked()
                                {
                                    close_requested = true;
                                }
                                if matches!(&dialog.status, ArchivePreviewStatus::Ready { .. })
                                    && ui
                                        .button(format!(
                                            "{} {}",
                                            regular::DOWNLOAD_SIMPLE,
                                            translator.text("archive-preview-download")
                                        ))
                                        .clicked()
                                {
                                    download_requested = Some(dialog.resource.clone());
                                }
                            });
                        });
                    });
            });
        if let Some(resource) = download_requested {
            self.download_archive_attachment(resource);
        }
        if close_requested || !open {
            *self.archive_preview.borrow_mut() = None;
        }
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
        let translator = Translator::new(LocaleDomain::WebUi, self.ui_language);
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
                    ui.heading(format!(
                        "{} {}",
                        regular::ROBOT,
                        translator.text("session-list-heading")
                    ));
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
                    ui.label(translator.text("session-list-empty"));
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
                                        translator.text("session-visible")
                                    } else {
                                        translator.text("session-hidden")
                                    }
                                ));
                        if response.clicked() {
                            focus_session_key = Some(session.session_key.clone());
                        }
                        response.context_menu(|ui| {
                            if ui
                                .button(format!(
                                    "{} {}",
                                    regular::PENCIL_SIMPLE,
                                    translator.text("session-rename")
                                ))
                                .clicked()
                            {
                                rename_session_key = Some(session.session_key.clone());
                                ui.close();
                            }
                            if ui
                                .button(format!(
                                    "{} {}",
                                    regular::COPY,
                                    translator.text("session-copy-id")
                                ))
                                .clicked()
                            {
                                copy_session_key = Some(session.session_key.clone());
                                ui.close();
                            }
                            if ui
                                .add(Button::new(
                                    RichText::new(format!(
                                        "{} {}",
                                        regular::TRASH,
                                        translator.text("session-delete")
                                    ))
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
            self.toasts
                .borrow_mut()
                .success(translator.text("session-id-copied"));
        }
        if let Some(session_key) = remove_session_key {
            self.delete_session_key = Some(session_key);
        }
        if create_session {
            self.create_session();
        }
    }

    fn render_rename_dialog(&mut self, ctx: &Context) {
        let translator = Translator::new(LocaleDomain::WebUi, self.ui_language);
        let Some(session_key) = self.rename_session_key.clone() else {
            return;
        };

        let mut open = true;
        let mut submit = false;
        let mut cancel = false;

        egui::Window::new(translator.text("rename-title"))
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                let response = ui.add(
                    TextEdit::singleline(&mut self.rename_session_input)
                        .desired_width(f32::INFINITY)
                        .hint_text(translator.text("rename-hint")),
                );
                let submit_with_enter =
                    response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(translator.text("rename-save")).clicked() || submit_with_enter {
                        submit = true;
                    }
                    if ui.button(translator.text("rename-cancel")).clicked() {
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
        let translator = Translator::new(LocaleDomain::WebUi, self.ui_language);
        if !self.show_gateway_dialog {
            return;
        }

        let mut open = self.show_gateway_dialog;
        let mut reconnect_all = false;

        egui::Window::new(translator.text("gateway-title"))
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.label(translator.text("gateway-hint"));
                ui.label(
                    RichText::new(translator.text("gateway-blank-hint"))
                        .small()
                        .weak(),
                );
                ui.add_space(8.0);

                let response = ui.add(
                    TextEdit::singleline(&mut self.gateway_token_input)
                        .password(true)
                        .desired_width(f32::INFINITY)
                        .hint_text(translator.text("gateway-token-hint")),
                );
                let submit_with_enter =
                    response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(translator.text("gateway-save-reconnect"))
                        .clicked()
                        || submit_with_enter
                    {
                        reconnect_all = true;
                    }
                    if ui.button(translator.text("gateway-clear")).clicked() {
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
        let translator = Translator::new(LocaleDomain::WebUi, self.ui_language);
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

        egui::Window::new(translator.text("delete-title"))
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(380.0);
                ui.label(delete_confirmation_body(&session_title, self.ui_language));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(Button::new(
                            RichText::new(translator.text("delete-confirm"))
                                .color(ui.visuals().error_fg_color),
                        ))
                        .clicked()
                    {
                        confirm = true;
                    }
                    if ui.button(translator.text("delete-cancel")).clicked() {
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
        let translator = Translator::new(LocaleDomain::WebUi, self.ui_language);
        let Some(index) = self.session_index(session_key) else {
            return;
        };

        let mut trigger_send = false;
        let mut trigger_file_picker = false;
        let mut trigger_card_action: Option<CardActionRequest> = None;
        let mut trigger_history_load: Option<PendingHistoryScrollRestore> = None;
        let mut trigger_open_file_dialog = false;
        let mut trigger_preview_attachment: Option<WebArchiveResource> = None;
        let mut trigger_download_attachment: Option<WebArchiveResource> = None;
        let mut remove_attachment_at: Option<usize> = None;
        let mut set_active = false;
        {
            let session = &mut self.sessions[index];
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
                    session.prune_finished_animations();
                    let messages = session.buffers.messages.borrow();
                    let scroll_output = ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .id_salt(("session-messages", &session.session_key))
                        .show(ui, |ui| {
                            if *session.buffers.history_loading.borrow() && messages.is_empty() {
                                render_history_loading_state(ui, self.ui_language);
                                return;
                            }
                            if messages.is_empty() {
                                render_empty_state(
                                    ui,
                                    &self.connection_state.borrow(),
                                    self.ui_language,
                                );
                                return;
                            }
                            if *session.buffers.history_loading.borrow() {
                                render_history_page_loading_state(ui, self.ui_language);
                                ui.add_space(8.0);
                            }
                            let mut card_action = None;
                            for (message_index, message) in messages.iter().enumerate() {
                                if is_hidden_internal_card_command(message) {
                                    continue;
                                }
                                if let Some(action) = render_message(
                                    ui,
                                    &mut session.markdown_cache,
                                    &mut session.fade_in_messages,
                                    &session.session_key,
                                    message,
                                    &messages,
                                    message_index,
                                    &session.card_state_overrides,
                                    &mut trigger_preview_attachment,
                                    &mut trigger_download_attachment,
                                    self.ui_language,
                                ) {
                                    card_action = Some(action);
                                }
                                ui.add_space(8.0);
                            }
                            let last_visible_role = messages
                                .iter()
                                .rev()
                                .find(|message| !is_hidden_internal_card_command(message))
                                .map(|message| message.role);
                            if should_show_thinking_placeholder(
                                last_visible_role,
                                session.buffers.active_stream_request_id.borrow().as_deref(),
                            ) {
                                render_thinking_placeholder(ui, self.ui_language);
                                ui.add_space(8.0);
                            }
                            trigger_card_action = card_action;
                        });
                    if let Some(restore) = session.pending_history_scroll_restore.as_ref()
                        && !*session.buffers.history_loading.borrow()
                    {
                        let mut state = scroll_output.state;
                        state.offset.y = (restore.offset_y
                            + (scroll_output.content_size.y - restore.content_height))
                            .max(0.0);
                        state.store(ui.ctx(), scroll_output.id);
                        session.pending_history_scroll_restore = None;
                        ui.ctx().request_repaint();
                    }
                    if scroll_output.state.offset.y <= 12.0
                        && session.history_has_more
                        && !*session.buffers.history_loading.borrow()
                        && session.pending_history_scroll_restore.is_none()
                        && !messages.is_empty()
                    {
                        trigger_history_load = Some(PendingHistoryScrollRestore {
                            offset_y: scroll_output.state.offset.y,
                            content_height: scroll_output.content_size.y,
                        });
                    }
                });

                ui.separator();
                ui.vertical(|ui| {
                    let selecting_file = *session.selecting_file.borrow();
                    let uploading_file = *session.uploading_file.borrow();
                    let attachment_busy =
                        attachment_action_in_progress(selecting_file, uploading_file);
                    let attachment_count = session.pending_attachments.borrow().len();
                    let can_send = self.connection_state.borrow().can_send();
                    ui.label(
                        RichText::new(translator.text("composer-slash-hint"))
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
                                    .hint_text(
                                        self.connection_state
                                            .borrow()
                                            .composer_hint_text(self.ui_language),
                                    )
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
                        if has_exact_slash_command_match(&trigger.query) {
                            return None;
                        }
                        let dismissed = session.slash_completer.dismissed_query.as_deref()
                            == Some(trigger.query.as_str())
                            && session.slash_completer.dismissed_start
                                == Some(trigger.replace_range.start);
                        (!dismissed).then_some(trigger.clone())
                    });
                    let slash_matches = slash_trigger
                        .as_ref()
                        .map(|trigger| slash_command_matches(&trigger.query));
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
                        && ui.input(|input| {
                            input.key_pressed(Key::Enter) && !input.modifiers.command
                        })
                        && slash_trigger.is_none()
                        && previous_slash_state.replace_range.is_some();
                    let insert_newline = response.has_focus()
                        && ui.input(|input| {
                            input.key_pressed(Key::Enter) && input.modifiers.command
                        });

                    if insert_newline {
                        session.draft.push('\n');
                    }

                    let popup_pos = input_output
                        .cursor_range
                        .map(|cursor_range| {
                            let cursor_rect =
                                input_output.galley.pos_from_cursor(cursor_range.primary);
                            response.rect.min + cursor_rect.left_bottom().to_vec2() + vec2(0.0, 6.0)
                        })
                        .unwrap_or_else(|| {
                            egui::pos2(response.rect.left(), response.rect.bottom() + 4.0)
                        });

                    if let (Some(trigger), Some(matches)) =
                        (slash_trigger.as_ref(), slash_matches.as_ref())
                    {
                        if response.has_focus() {
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
                    } else if response.has_focus()
                        && complete_on_enter
                        && let Some(replace_range) = previous_slash_state.replace_range.clone()
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
                                egui::Button::new(format!(
                                    "{} {}",
                                    regular::PAPERCLIP,
                                    translator.text("upload")
                                ))
                                .small(),
                            )
                            .on_hover_text(translator.text("upload-hover"))
                            .clicked()
                        {
                            trigger_file_picker = true;
                        }
                        if ui
                            .add_enabled(
                                attachment_count > 0,
                                egui::Button::new(format!(
                                    "{} {}",
                                    regular::FILE,
                                    translator.text_args(
                                        "file-count",
                                        HashMap::from([("count", attachment_count.to_string())])
                                    )
                                ))
                                .small(),
                            )
                            .on_hover_text(translator.text("file-count-hover"))
                            .clicked()
                        {
                            trigger_open_file_dialog = true;
                        }

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let send_button = ui.add_enabled(
                                can_send && !attachment_busy,
                                Button::new(format!(
                                    "{} {}",
                                    regular::PAPER_PLANE,
                                    translator.text("send")
                                )),
                            );
                            if send_button.clicked() || send_on_enter {
                                trigger_send = true;
                            }

                            ui.add_space(6.0);
                            ui.add_enabled_ui(can_send && !attachment_busy, |ui| {
                                ui.add_sized(
                                    [model_width, control_height],
                                    TextEdit::singleline(&mut session.selected_route.model)
                                        .hint_text(translator.text("model-hint")),
                                );
                                let provider_changed =
                                    ComboBox::from_id_salt(("session-model-provider", session_key))
                                        .width(provider_width)
                                        .selected_text(
                                            if session.selected_route.model_provider.is_empty() {
                                                translator.text("provider-hint")
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
                            if selecting_file {
                                ui.spinner();
                                ui.label(
                                    RichText::new(translator.text("selecting-file"))
                                        .small()
                                        .weak(),
                                );
                            } else if uploading_file {
                                ui.spinner();
                                ui.label(
                                    RichText::new(translator.text("uploading")).small().weak(),
                                );
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

        if let Some(index) = self.session_index(session_key) {
            if trigger_open_file_dialog {
                self.sessions[index].show_file_dialog = true;
            }
            if self.sessions[index].show_file_dialog {
                let mut show_file_dialog = self.sessions[index].show_file_dialog;
                let attachments = self.sessions[index].pending_attachments.borrow().clone();
                render_session_file_dialog(
                    ctx,
                    &mut show_file_dialog,
                    &attachments,
                    &mut trigger_preview_attachment,
                    &mut trigger_download_attachment,
                    &mut remove_attachment_at,
                    self.ui_language,
                );
                self.sessions[index].show_file_dialog = show_file_dialog;
            }
            if let Some(remove_index) = remove_attachment_at {
                let attachments = &mut *self.sessions[index].pending_attachments.borrow_mut();
                if remove_index < attachments.len() {
                    attachments.remove(remove_index);
                }
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
        if let Some(scroll_restore) = trigger_history_load {
            self.load_history_page(session_key, Some(scroll_restore));
        }
        if trigger_file_picker {
            self.trigger_file_picker(session_key);
        }
        if let Some(resource) = trigger_preview_attachment {
            self.preview_archive_attachment(resource);
        }
        if let Some(resource) = trigger_download_attachment {
            self.download_archive_attachment(resource);
        }
        if trigger_send {
            self.send_session_draft(session_key);
        }
        if let Some(action) = trigger_card_action {
            if let Some(index) = self.session_index(session_key) {
                self.sessions[index].card_state_overrides.insert(
                    action.card_key.clone(),
                    CardInteractionState::Pending {
                        label: action.pending_label.clone(),
                    },
                );
            }
            let sent = self.send_card_action(session_key, &action.command, action.metadata);
            if let Some(index) = self.session_index(session_key) {
                if sent {
                    if let Some(label) = action.completion_label {
                        self.sessions[index]
                            .card_state_overrides
                            .insert(action.card_key, CardInteractionState::Completed { label });
                    }
                } else {
                    self.sessions[index].card_state_overrides.insert(
                        action.card_key,
                        CardInteractionState::Failed {
                            message: translator.text("send-card-failed"),
                        },
                    );
                }
            }
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
        let translator = Translator::new(LocaleDomain::WebUi, self.ui_language);
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
                        ui.heading(translator.text("workbench-connect-heading"));
                        ui.add_space(4.0);
                        ui.label(translator.text("workbench-connect-body"));
                        ui.add_space(12.0);
                        if ui
                            .button(translator.text("workbench-connect-button"))
                            .clicked()
                        {
                            self.request_workspace_connection();
                        }
                    });
                });
                return;
            }
            PageMode::LoadingWorkspace => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label(translator.text("workbench-loading"));
                    });
                });
                return;
            }
            PageMode::Workspace => {}
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.sessions.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(translator.text("workbench-no-agents"));
                });
                return;
            }

            ui.label(RichText::new(translator.text("workbench-heading")).strong());
            ui.label(
                RichText::new(translator.text("workbench-subheading"))
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
        self.render_settings_dialog(ctx);
        self.render_about_dialog(ctx);
        self.render_rename_dialog(ctx);
        self.render_delete_dialog(ctx);
        self.render_archive_preview_dialog(ctx);
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
        let attachments = message_resources(message);
        let text_width = if t.trim().is_empty() {
            0.0
        } else {
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
        let attachment_width = attachments
            .iter()
            .map(|attachment| {
                WidgetText::from(RichText::new(format!(
                    "{} {}",
                    regular::FILE,
                    attachment.filename.as_deref().unwrap_or("unknown")
                )))
                .into_galley(ui, None, f32::INFINITY, TextStyle::Body)
                .size()
                .x
            })
            .fold(0.0, f32::max);
        if text_width >= BUBBLE_MAX_WIDTH || attachment_width >= BUBBLE_MAX_WIDTH {
            BUBBLE_MAX_WIDTH
        } else {
            text_width.max(attachment_width)
        }
    };

    (header_w.max(body_w) + SLACK).clamp(MIN_INNER, BUBBLE_MAX_WIDTH)
}

fn render_empty_state(ui: &mut egui::Ui, state: &ConnectionState, language: UiLanguage) {
    let copy = state.empty_state_copy(language);
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(copy.title).heading().strong());
        ui.add_space(4.0);
        ui.label(RichText::new(copy.body).weak());
    });
}

fn render_language_combo(
    ui: &mut egui::Ui,
    id: &'static str,
    selected_language: UiLanguage,
    width: f32,
) -> Option<UiLanguage> {
    let mut requested_language = None;
    ComboBox::from_id_salt(id)
        .width(width)
        .selected_text(selected_language.label())
        .show_ui(ui, |ui| {
            for language in UiLanguage::available() {
                if ui
                    .selectable_label(selected_language == *language, language.label())
                    .clicked()
                {
                    requested_language = Some(*language);
                    ui.close();
                }
            }
        });
    requested_language
}

fn render_history_loading_state(ui: &mut egui::Ui, language: UiLanguage) {
    let translator = Translator::new(LocaleDomain::WebUi, language);
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.spinner();
        ui.add_space(8.0);
        ui.label(
            RichText::new(translator.text("history-loading-title"))
                .heading()
                .strong(),
        );
        ui.add_space(4.0);
        ui.label(RichText::new(translator.text("history-loading-body")).weak());
    });
}

fn render_history_page_loading_state(ui: &mut egui::Ui, language: UiLanguage) {
    let translator = Translator::new(LocaleDomain::WebUi, language);
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(12.0));
        ui.label(
            RichText::new(translator.text("history-page-loading"))
                .small()
                .weak(),
        );
    });
}

fn render_thinking_placeholder(ui: &mut egui::Ui, language: UiLanguage) {
    let translator = Translator::new(LocaleDomain::WebUi, language);
    let palette = resolve_assistant_bubble_palette(ui.visuals().dark_mode);
    Frame::group(ui.style())
        .fill(rgb(palette.fill))
        .stroke(Stroke::new(1.0, rgb(palette.stroke)))
        .inner_margin(8.0)
        .outer_margin(2.0)
        .corner_radius(6.0)
        .show(ui, |ui| {
            ui.set_max_width(BUBBLE_MAX_WIDTH);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(translator.text("assistant-label"))
                        .strong()
                        .color(rgb(palette.heading)),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0));
                    ui.label(RichText::new(translator.text("thinking")).color(rgb(palette.body)));
                });
            });
        });
}

fn render_message(
    ui: &mut egui::Ui,
    markdown_cache: &mut MarkdownCache,
    fade_in_messages: &mut HashMap<String, TextAnimator>,
    session_key: &str,
    message: &ChatMessage,
    messages: &[ChatMessage],
    message_index: usize,
    card_state_overrides: &HashMap<String, CardInteractionState>,
    trigger_preview_attachment: &mut Option<WebArchiveResource>,
    trigger_download_attachment: &mut Option<WebArchiveResource>,
    language: UiLanguage,
) -> Option<CardActionRequest> {
    let translator = Translator::new(LocaleDomain::WebUi, language);
    let now_ms = current_timestamp_ms();
    let time_label = format_message_timestamp(message.timestamp_ms, now_ms);
    match message.role {
        MessageRole::System => {
            ui.vertical_centered(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(translator.text("role-system"))
                            .small()
                            .strong()
                            .weak(),
                    );
                    ui.label(RichText::new(time_label).small().weak());
                });
                render_plain_message(ui, &message.text, ui.visuals().weak_text_color());
            });
            None
        }
        MessageRole::Assistant | MessageRole::User => {
            let role_label = match message.role {
                MessageRole::Assistant => translator.text("assistant-label"),
                MessageRole::User => translator.text("role-you"),
                MessageRole::System => translator.text("role-system"),
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
                    _ => {
                        let palette = resolve_assistant_bubble_palette(dark_mode);
                        (
                            Color32::from_rgb(palette.fill[0], palette.fill[1], palette.fill[2]),
                            Stroke::new(
                                1.0,
                                Color32::from_rgb(
                                    palette.stroke[0],
                                    palette.stroke[1],
                                    palette.stroke[2],
                                ),
                            ),
                            Color32::from_rgb(
                                palette.heading[0],
                                palette.heading[1],
                                palette.heading[2],
                            ),
                            Color32::from_rgb(palette.body[0], palette.body[1], palette.body[2]),
                            Color32::from_rgb(palette.link[0], palette.link[1], palette.link[2]),
                        )
                    }
                };

            let inner_w_user = if is_user {
                Some(user_bubble_inner_max_width(
                    ui,
                    message,
                    &role_label,
                    &time_label,
                ))
            } else {
                None
            };
            let mut action_request = None;
            #[allow(unused_mut)]
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
                            if let Some(card) = message.card.as_ref() {
                                if let Some(action) = render_card_message(
                                    ui,
                                    markdown_cache,
                                    session_key,
                                    message,
                                    card,
                                    messages,
                                    message_index,
                                    card_state_overrides,
                                    language,
                                ) {
                                    action_request = Some(action);
                                }
                            } else {
                                render_message_body(
                                    ui,
                                    markdown_cache,
                                    fade_in_messages,
                                    message,
                                    body_color,
                                    link_color,
                                    trigger_preview_attachment,
                                    trigger_download_attachment,
                                    language,
                                );
                            }
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
            action_request
        }
    }
}

#[derive(Clone)]
pub(super) struct CardActionRequest {
    card_key: String,
    command: String,
    metadata: BTreeMap<String, serde_json::Value>,
    pending_label: String,
    completion_label: Option<String>,
}

fn render_card_message(
    ui: &mut egui::Ui,
    markdown_cache: &mut MarkdownCache,
    session_key: &str,
    message: &ChatMessage,
    card: &ImCard,
    messages: &[ChatMessage],
    message_index: usize,
    card_state_overrides: &HashMap<String, CardInteractionState>,
    language: UiLanguage,
) -> Option<CardActionRequest> {
    let translator = Translator::new(LocaleDomain::WebUi, language);
    let card_key = message
        .message_id
        .clone()
        .unwrap_or_else(|| message.id.clone());
    let derived_state = derived_card_state(messages, message_index, card, language);
    let effective_state = card_state_overrides
        .get(&card_key)
        .cloned()
        .or(derived_state);
    let interactive = effective_state.is_none() && !has_follow_up_messages(messages, message_index);

    let palette = resolve_im_card_palette(card.kind.clone(), ui.visuals().dark_mode);
    let badge_label = match card.kind {
        ImCardKind::Approval => translator.text("card-approval-badge"),
        ImCardKind::QuestionSingleSelect => translator.text("card-question-badge"),
    };
    let fallback_title = match card.kind {
        ImCardKind::Approval => translator.text("card-approval-title"),
        ImCardKind::QuestionSingleSelect => translator.text("card-question-title"),
    };

    let mut action_request = None;
    Frame::group(ui.style())
        .fill(rgb(palette.fill))
        .stroke(Stroke::new(1.0, rgb(palette.stroke)))
        .corner_radius(10.0)
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(card.title_or(&fallback_title))
                        .strong()
                        .color(rgb(palette.title)),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(badge_label).small().color(rgb(palette.badge)));
                });
            });
            if let Some(command_preview) = card.command_preview() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(translator.text("card-command-label"))
                        .small()
                        .strong()
                        .color(rgb(palette.title)),
                );
                Frame::group(ui.style())
                    .fill(rgb(palette.preview_fill))
                    .stroke(Stroke::new(1.0, rgb(palette.preview_stroke)))
                    .corner_radius(8.0)
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new(command_preview)
                                .monospace()
                                .color(rgb(palette.preview_text)),
                        );
                    });
            }
            let body = card.body_or(card.fallback_text_or(""));
            if !body.trim().is_empty() {
                ui.add_space(6.0);
                render_markdown(
                    ui,
                    markdown_cache,
                    body,
                    rgb(palette.body),
                    ui.visuals().hyperlink_color,
                );
            }
            if matches!(card.kind, ImCardKind::Approval)
                && let Some(approval_id) = card.approval_id()
            {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(translator.text_args(
                        "card-approval-id",
                        HashMap::from([("id", approval_id.to_string())]),
                    ))
                    .small()
                    .color(rgb(palette.body)),
                );
            }
            ui.add_space(8.0);
            if let Some(state) = effective_state.as_ref() {
                render_card_state_banner(ui, state);
            }
            ui.horizontal_wrapped(|ui| {
                for action in &card.actions {
                    match action.kind {
                        ImCardActionKind::OpenUrl => {
                            if let Some(url) = action
                                .url
                                .as_deref()
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                            {
                                ui.hyperlink_to(action.label_or_default(), url);
                            }
                        }
                        ImCardActionKind::Approve
                        | ImCardActionKind::Reject
                        | ImCardActionKind::SubmitCommand => {
                            let enabled = interactive
                                || matches!(
                                    effective_state.as_ref(),
                                    Some(CardInteractionState::Failed { .. })
                                );
                            let button =
                                ui.add_enabled(enabled, Button::new(action.label_or_default()));
                            if button.clicked() {
                                action_request = build_card_action_request(
                                    session_key,
                                    &card_key,
                                    card,
                                    action,
                                    language,
                                );
                            }
                        }
                    }
                }
            });
        });
    action_request
}

fn render_card_state_banner(ui: &mut egui::Ui, state: &CardInteractionState) {
    let (text, color) = match state {
        CardInteractionState::Pending { label } => {
            (format!("{label}…"), Color32::from_rgb(171, 111, 26))
        }
        CardInteractionState::Completed { label } => {
            (label.clone(), Color32::from_rgb(40, 130, 76))
        }
        CardInteractionState::Failed { message } => {
            (message.clone(), Color32::from_rgb(186, 64, 64))
        }
    };
    ui.label(RichText::new(text).small().strong().color(color));
}

fn build_card_action_request(
    session_key: &str,
    card_key: &str,
    card: &ImCard,
    action: &ImCardAction,
    language: UiLanguage,
) -> Option<CardActionRequest> {
    let translator = Translator::new(LocaleDomain::WebUi, language);
    let command = match action.kind {
        ImCardActionKind::Approve => {
            let approval_id = action.approval_id()?;
            format!("/approve {approval_id}")
        }
        ImCardActionKind::Reject => {
            let approval_id = action.approval_id()?;
            format!("/reject {approval_id}")
        }
        ImCardActionKind::SubmitCommand => action
            .command
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())?
            .to_string(),
        ImCardActionKind::OpenUrl => return None,
    };
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "webui.card.action".to_string(),
        serde_json::Value::Bool(true),
    );
    metadata.insert(
        "webui.card.kind".to_string(),
        serde_json::Value::String(match card.kind {
            ImCardKind::Approval => "approval".to_string(),
            ImCardKind::QuestionSingleSelect => "question_single_select".to_string(),
        }),
    );
    metadata.insert(
        "webui.card.action_kind".to_string(),
        serde_json::Value::String(match action.kind {
            ImCardActionKind::Approve => "approve".to_string(),
            ImCardActionKind::Reject => "reject".to_string(),
            ImCardActionKind::OpenUrl => "open_url".to_string(),
            ImCardActionKind::SubmitCommand => "submit_command".to_string(),
        }),
    );
    metadata.insert(
        "webui.card.source_message_id".to_string(),
        serde_json::Value::String(card_key.to_string()),
    );
    metadata.insert(
        "webui.card.session_key".to_string(),
        serde_json::Value::String(session_key.to_string()),
    );
    if let Some(question_id) = card
        .metadata
        .get("question_id")
        .and_then(serde_json::Value::as_str)
    {
        metadata.insert(
            "webui.card.question_id".to_string(),
            serde_json::Value::String(question_id.to_string()),
        );
    }
    if let Some(approval_id) = card.approval_id() {
        metadata.insert(
            "webui.card.approval_id".to_string(),
            serde_json::Value::String(approval_id.to_string()),
        );
    }
    let completion_label = match card.kind {
        ImCardKind::Approval => None,
        ImCardKind::QuestionSingleSelect => Some(translator.text_args(
            "card-selected-answer",
            HashMap::from([("answer", action.label_or_default().to_string())]),
        )),
    };
    Some(CardActionRequest {
        card_key: card_key.to_string(),
        command,
        metadata,
        pending_label: action.label_or_default().to_string(),
        completion_label,
    })
}

pub(super) fn sync_card_state_overrides(
    messages: &[ChatMessage],
    overrides: &mut HashMap<String, CardInteractionState>,
    language: UiLanguage,
) {
    let updates = messages
        .iter()
        .enumerate()
        .filter_map(|(message_index, message)| {
            let card = message.card.as_ref()?;
            let derived_state = derived_card_state(messages, message_index, card, language)?;
            let card_key = message
                .message_id
                .clone()
                .unwrap_or_else(|| message.id.clone());
            Some((card_key, derived_state))
        })
        .collect::<Vec<_>>();
    for (card_key, state) in updates {
        overrides.insert(card_key, state);
    }
}

fn derived_card_state(
    messages: &[ChatMessage],
    message_index: usize,
    card: &ImCard,
    language: UiLanguage,
) -> Option<CardInteractionState> {
    let translator = Translator::new(LocaleDomain::WebUi, language);
    match card.kind {
        ImCardKind::Approval => {
            let approval_id = card.approval_id()?;
            messages.iter().skip(message_index + 1).find_map(|message| {
                parse_internal_card_command(&message.text).and_then(|command| match command {
                    InternalCardCommand::Approve(id) if id == approval_id => {
                        Some(CardInteractionState::Completed {
                            label: translator.text("card-approved"),
                        })
                    }
                    InternalCardCommand::Reject(id) if id == approval_id => {
                        Some(CardInteractionState::Completed {
                            label: translator.text("card-rejected"),
                        })
                    }
                    _ => None,
                })
            })
        }
        ImCardKind::QuestionSingleSelect => {
            let question_id = card.metadata.get("question_id")?.as_str()?;
            messages.iter().skip(message_index + 1).find_map(|message| {
                parse_internal_card_command(&message.text).and_then(|command| match command {
                    InternalCardCommand::Answer {
                        question_id: answered_question_id,
                        option_id,
                    } if answered_question_id == question_id => {
                        Some(CardInteractionState::Completed {
                            label: translator.text_args(
                                "card-selected-answer",
                                HashMap::from([(
                                    "answer",
                                    find_card_option_label(card, &option_id).unwrap_or(option_id),
                                )]),
                            ),
                        })
                    }
                    _ => None,
                })
            })
        }
    }
}

fn find_card_option_label(card: &ImCard, option_id: &str) -> Option<String> {
    card.actions.iter().find_map(|action| {
        action
            .command
            .as_deref()
            .and_then(parse_card_answer_command)
            .filter(|(_, candidate_option_id)| candidate_option_id == option_id)
            .map(|_| action.label_or_default().to_string())
    })
}

fn has_follow_up_messages(messages: &[ChatMessage], message_index: usize) -> bool {
    messages
        .iter()
        .skip(message_index + 1)
        .any(|message| !is_hidden_internal_card_command(message))
}

enum InternalCardCommand {
    Approve(String),
    Reject(String),
    Answer {
        question_id: String,
        option_id: String,
    },
}

fn parse_internal_card_command(text: &str) -> Option<InternalCardCommand> {
    let trimmed = text.trim();
    if let Some(approval_id) = trimmed.strip_prefix("/approve ") {
        let approval_id = approval_id.trim();
        if !approval_id.is_empty() {
            return Some(InternalCardCommand::Approve(approval_id.to_string()));
        }
    }
    if let Some(approval_id) = trimmed.strip_prefix("/reject ") {
        let approval_id = approval_id.trim();
        if !approval_id.is_empty() {
            return Some(InternalCardCommand::Reject(approval_id.to_string()));
        }
    }
    parse_card_answer_command(trimmed).map(|(question_id, option_id)| InternalCardCommand::Answer {
        question_id,
        option_id,
    })
}

fn parse_card_answer_command(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim();
    let rest = trimmed.strip_prefix("/card_answer ")?;
    let mut parts = rest.split_whitespace();
    let question_id = parts.next()?.trim();
    let option_id = parts.next()?.trim();
    if question_id.is_empty() || option_id.is_empty() {
        return None;
    }
    Some((question_id.to_string(), option_id.to_string()))
}

pub(super) fn is_hidden_internal_card_command(message: &ChatMessage) -> bool {
    matches!(message.role, MessageRole::User)
        && parse_internal_card_command(&message.text).is_some()
}

fn render_message_body(
    ui: &mut egui::Ui,
    markdown_cache: &mut MarkdownCache,
    fade_in_messages: &mut HashMap<String, TextAnimator>,
    message: &ChatMessage,
    body_color: Color32,
    link_color: Color32,
    trigger_preview_attachment: &mut Option<WebArchiveResource>,
    trigger_download_attachment: &mut Option<WebArchiveResource>,
    language: UiLanguage,
) {
    let attachments = message_resources(message);
    let has_text = !message.text.trim().is_empty();
    let has_attachments = !attachments.is_empty();

    if has_text {
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
            if matches!(message.role, MessageRole::Assistant) {
                render_scrollable_markdown(
                    ui,
                    markdown_cache,
                    &message.text,
                    body_color,
                    link_color,
                    ("assistant-message-markdown", &message.id),
                );
            } else {
                render_markdown(ui, markdown_cache, &message.text, body_color, link_color);
            }
        }

        if should_remove_animation {
            fade_in_messages.remove(&message.id);
        }
    }

    if has_text && has_attachments {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);
    }

    if has_attachments {
        ui.vertical(|ui| {
            for attachment in attachments {
                render_resource_card(
                    ui,
                    &attachment,
                    body_color,
                    trigger_preview_attachment,
                    trigger_download_attachment,
                    language,
                );
            }
        });
    }
}

fn message_resources(message: &ChatMessage) -> Vec<WebArchiveResource> {
    if let Some(resources) = message
        .metadata
        .get("archive.resources")
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<WebArchiveResource>>(value).ok())
        .filter(|resources| !resources.is_empty())
    {
        return resources;
    }

    if let Some(attachments) = message
        .metadata
        .get("attachments")
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<WebArchiveAttachment>>(value).ok())
        .filter(|attachments| !attachments.is_empty())
    {
        return attachments
            .into_iter()
            .map(web_archive_resource_from_attachment)
            .collect();
    }

    message
        .metadata
        .get("channel.attachments")
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<serde_json::Value>>(value).ok())
        .unwrap_or_default()
        .into_iter()
        .filter_map(resource_from_channel_attachment)
        .collect()
}

fn resource_from_channel_attachment(value: serde_json::Value) -> Option<WebArchiveResource> {
    let archive_id = value
        .get("archive_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    Some(WebArchiveResource {
        archive_id,
        filename: value
            .get("filename")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        mime_type: value
            .get("mime_type")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        size_bytes: value
            .get("size_bytes")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or_default(),
        kind: value
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        caption: value
            .get("caption")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
    })
}

fn render_resource_card(
    ui: &mut egui::Ui,
    resource: &WebArchiveResource,
    body_color: Color32,
    trigger_preview_attachment: &mut Option<WebArchiveResource>,
    trigger_download_attachment: &mut Option<WebArchiveResource>,
    language: UiLanguage,
) {
    let translator = Translator::new(LocaleDomain::WebUi, language);
    let icon = if resource_is_image(resource) {
        regular::IMAGE
    } else {
        regular::FILE
    };
    Frame::group(ui.style())
        .fill(ui.visuals().extreme_bg_color)
        .stroke(Stroke::new(
            1.0,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .corner_radius(6.0)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).color(body_color));
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(resource.filename.as_deref().unwrap_or("attachment"))
                            .strong()
                            .color(body_color),
                    );
                    ui.label(RichText::new(resource_meta_label(resource)).small().weak());
                    if let Some(caption) = resource.caption.as_deref() {
                        ui.label(RichText::new(caption).small().color(body_color));
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if archive_resource_is_previewable(resource) {
                        if ui
                            .button(format!(
                                "{} {}",
                                regular::EYE,
                                archive_resource_card_action(resource)
                            ))
                            .on_hover_text(translator.text("archive-hover-preview"))
                            .clicked()
                        {
                            *trigger_preview_attachment = Some(resource.clone());
                        }
                    } else if ui
                        .button(format!(
                            "{} {}",
                            regular::DOWNLOAD_SIMPLE,
                            archive_resource_card_action(resource)
                        ))
                        .on_hover_text(translator.text("archive-hover-download"))
                        .clicked()
                    {
                        *trigger_download_attachment = Some(resource.clone());
                    }
                });
            });
        });
}

fn resource_meta_label(resource: &WebArchiveResource) -> String {
    let mut parts = Vec::new();
    if let Some(mime_type) = resource.mime_type.as_deref() {
        parts.push(mime_type.to_string());
    } else if let Some(kind) = resource.kind.as_deref() {
        parts.push(kind.to_string());
    }
    if resource.size_bytes > 0 {
        parts.push(format_file_size(resource.size_bytes));
    }
    parts.push(resource.archive_id.clone());
    parts.join(" · ")
}

fn resource_is_image(resource: &WebArchiveResource) -> bool {
    archive_resource_is_image(resource)
}

fn archive_preview_default_size(dialog: &ArchivePreviewDialog) -> egui::Vec2 {
    let Some([width, height]) = archive_preview_image_size(dialog) else {
        return vec2(720.0, 560.0);
    };
    if width == 0 || height == 0 {
        return vec2(720.0, 560.0);
    }

    const HORIZONTAL_CHROME: f32 = 48.0;
    const VERTICAL_CHROME: f32 = 136.0;
    const MAX_WINDOW_WIDTH: f32 = 960.0;
    const MAX_WINDOW_HEIGHT: f32 = 760.0;

    let image_width = width as f32;
    let image_height = height as f32;
    let scale = ((MAX_WINDOW_WIDTH - HORIZONTAL_CHROME) / image_width)
        .min((MAX_WINDOW_HEIGHT - VERTICAL_CHROME) / image_height)
        .min(1.0);

    vec2(
        (image_width * scale + HORIZONTAL_CHROME).clamp(520.0, MAX_WINDOW_WIDTH),
        (image_height * scale + VERTICAL_CHROME).clamp(420.0, MAX_WINDOW_HEIGHT),
    )
}

fn archive_preview_image_size(dialog: &ArchivePreviewDialog) -> Option<[usize; 2]> {
    match &dialog.status {
        ArchivePreviewStatus::Ready { image_size, .. } => *image_size,
        _ => None,
    }
}

fn preview_status_is_image(dialog: &ArchivePreviewDialog, content_type: Option<&str>) -> bool {
    content_type.is_some_and(content_type_is_image) || resource_is_image(&dialog.resource)
}

fn preview_status_is_text(dialog: &ArchivePreviewDialog, content_type: Option<&str>) -> bool {
    content_type.is_some_and(content_type_is_text)
        || dialog
            .resource
            .mime_type
            .as_deref()
            .is_some_and(content_type_is_text)
}

fn render_archive_preview_header(ui: &mut egui::Ui, dialog: &ArchivePreviewDialog) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(if resource_is_image(&dialog.resource) {
            regular::IMAGE
        } else {
            regular::FILE
        }));
        ui.vertical(|ui| {
            ui.label(
                RichText::new(
                    dialog
                        .resource
                        .filename
                        .as_deref()
                        .unwrap_or("archive resource"),
                )
                .strong(),
            );
            ui.label(
                RichText::new(resource_meta_label(&dialog.resource))
                    .small()
                    .weak(),
            );
        });
    });
}

fn render_archive_preview_content(
    ui: &mut egui::Ui,
    dialog: &ArchivePreviewDialog,
    language: UiLanguage,
) {
    let translator = Translator::new(LocaleDomain::WebUi, language);
    match &dialog.status {
        ArchivePreviewStatus::Loading => {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.spinner();
                ui.add_space(8.0);
                ui.label(translator.text("archive-preview-loading"));
            });
        }
        ArchivePreviewStatus::Ready {
            content_type,
            bytes,
            ..
        } => {
            if preview_status_is_image(dialog, content_type.as_deref()) {
                render_image_preview_body(ui, dialog, bytes);
            } else if preview_status_is_text(dialog, content_type.as_deref()) {
                render_text_preview_body(ui, bytes);
            } else {
                render_file_preview_body(ui, dialog, language);
            }
        }
        ArchivePreviewStatus::Failed(message) => {
            ui.label(
                RichText::new(format!("Preview failed: {message}"))
                    .color(ui.visuals().error_fg_color),
            );
        }
    }
}

fn render_image_preview_body(ui: &mut egui::Ui, dialog: &ArchivePreviewDialog, bytes: &[u8]) {
    ScrollArea::both().show(ui, |ui| {
        ui.add(
            Image::from_bytes(
                format!("bytes://archive/{}", dialog.resource.archive_id),
                bytes.to_vec(),
            )
            .max_size(ui.available_size())
            .maintain_aspect_ratio(true),
        );
    });
}

fn render_text_preview_body(ui: &mut egui::Ui, bytes: &[u8]) {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    ScrollArea::both().show(ui, |ui| {
        ui.add(
            TextEdit::multiline(&mut text)
                .font(TextStyle::Monospace)
                .desired_width(ui.available_width())
                .interactive(false),
        );
    });
}

fn render_file_preview_body(
    ui: &mut egui::Ui,
    dialog: &ArchivePreviewDialog,
    language: UiLanguage,
) {
    let translator = Translator::new(LocaleDomain::WebUi, language);
    ui.vertical_centered(|ui| {
        ui.add_space(24.0);
        ui.label(RichText::new(regular::FILE).size(42.0));
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                dialog
                    .resource
                    .filename
                    .as_deref()
                    .unwrap_or("archive resource"),
            )
            .strong(),
        );
        ui.label(
            RichText::new(resource_meta_label(&dialog.resource))
                .small()
                .weak(),
        );
        ui.add_space(12.0);
        ui.label(RichText::new(translator.text("archive-preview-unavailable")).weak());
    });
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
    let cursor_char_index =
        apply_slash_completion(draft, trigger.replace_range.clone(), completion);
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

fn session_route_label(session: &SessionWindow, language: UiLanguage) -> String {
    let translator = Translator::new(LocaleDomain::WebUi, language);
    let provider = session.selected_route.model_provider.trim();
    let model = session.selected_route.model.trim();
    match (provider.is_empty(), model.is_empty()) {
        (true, true) => translator.text("route-default"),
        (false, true) => translator.text_args(
            "route-provider",
            HashMap::from([("provider", provider.to_string())]),
        ),
        (true, false) => {
            translator.text_args("route-model", HashMap::from([("model", model.to_string())]))
        }
        (false, false) => translator.text_args(
            "route-provider-model",
            HashMap::from([
                ("provider", provider.to_string()),
                ("model", model.to_string()),
            ]),
        ),
    }
}

fn session_activity_label(session: &SessionWindow, language: UiLanguage) -> Option<String> {
    let translator = Translator::new(LocaleDomain::WebUi, language);
    if *session.buffers.history_loading.borrow() {
        Some(translator.text("activity-history"))
    } else if *session.uploading_file.borrow() {
        Some(translator.text("activity-uploading"))
    } else if *session.selecting_file.borrow() {
        Some(translator.text("activity-picking-file"))
    } else if session
        .buffers
        .active_stream_request_id
        .borrow()
        .as_deref()
        .is_some()
    {
        Some(translator.text("activity-streaming"))
    } else if !session.pending_attachments.borrow().is_empty() {
        Some(translator.text("activity-files-ready"))
    } else {
        None
    }
}

fn render_session_file_dialog(
    ctx: &Context,
    open: &mut bool,
    attachments: &[WebArchiveAttachment],
    trigger_preview_attachment: &mut Option<WebArchiveResource>,
    trigger_download_attachment: &mut Option<WebArchiveResource>,
    remove_attachment_at: &mut Option<usize>,
    language: UiLanguage,
) {
    let translator = Translator::new(LocaleDomain::WebUi, language);
    let mut keep_open = *open;
    egui::Window::new(translator.text("file-dialog-title"))
        .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(true)
        .default_size(vec2(560.0, 320.0))
        .min_width(460.0)
        .open(&mut keep_open)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(translator.text("file-dialog-hint"))
                    .small()
                    .weak(),
            );
            ui.add_space(8.0);

            if attachments.is_empty() {
                ui.label(translator.text("file-dialog-empty"));
                return;
            }

            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .column(Column::remainder())
                .column(Column::remainder().at_least(180.0))
                .column(Column::auto().at_least(72.0))
                .header(24.0, |mut header| {
                    header.col(|ui| {
                        ui.strong(translator.text("file-dialog-col-name"));
                    });
                    header.col(|ui| {
                        ui.strong(translator.text("file-dialog-col-archive-id"));
                    });
                    header.col(|ui| {
                        ui.strong(translator.text("file-dialog-col-size"));
                    });
                })
                .body(|body| {
                    body.rows(26.0, attachments.len(), |mut row| {
                        let index = row.index();
                        let attachment = &attachments[index];
                        row.col(|ui| {
                            let response = ui.selectable_label(
                                false,
                                attachment.filename.as_deref().unwrap_or("unknown"),
                            );
                            render_attachment_context_menu(
                                &response,
                                attachment,
                                index,
                                trigger_preview_attachment,
                                trigger_download_attachment,
                                remove_attachment_at,
                                &translator,
                            );
                        });
                        row.col(|ui| {
                            let response = ui.monospace(&attachment.archive_id);
                            render_attachment_context_menu(
                                &response,
                                attachment,
                                index,
                                trigger_preview_attachment,
                                trigger_download_attachment,
                                remove_attachment_at,
                                &translator,
                            );
                        });
                        row.col(|ui| {
                            let response = ui.label(format_file_size(attachment.size_bytes));
                            render_attachment_context_menu(
                                &response,
                                attachment,
                                index,
                                trigger_preview_attachment,
                                trigger_download_attachment,
                                remove_attachment_at,
                                &translator,
                            );
                        });
                    });
                });
        });
    *open = keep_open;
}

fn render_attachment_context_menu(
    response: &egui::Response,
    attachment: &WebArchiveAttachment,
    index: usize,
    trigger_preview_attachment: &mut Option<WebArchiveResource>,
    trigger_download_attachment: &mut Option<WebArchiveResource>,
    remove_attachment_at: &mut Option<usize>,
    translator: &Translator,
) {
    response.context_menu(|ui| {
        let resource = web_archive_resource_from_attachment(attachment.clone());
        if archive_resource_is_previewable(&resource) {
            if ui
                .button(format!(
                    "{} {}",
                    regular::EYE,
                    translator.text("attachment-preview")
                ))
                .clicked()
            {
                *trigger_preview_attachment = Some(resource);
                ui.close();
            }
        } else if ui
            .button(format!(
                "{} {}",
                regular::DOWNLOAD_SIMPLE,
                translator.text("attachment-download")
            ))
            .clicked()
        {
            *trigger_download_attachment = Some(resource);
            ui.close();
        }
        if ui
            .add(Button::new(
                RichText::new(format!(
                    "{} {}",
                    regular::TRASH,
                    translator.text("attachment-delete")
                ))
                .color(ui.visuals().error_fg_color),
            ))
            .clicked()
        {
            *remove_attachment_at = Some(index);
            ui.close();
        }
    });
}

fn format_file_size(size_bytes: i64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let size = size_bytes.max(0) as f64;
    if size >= GB {
        format!("{:.1} GB", size / GB)
    } else if size >= MB {
        format!("{:.1} MB", size / MB)
    } else if size >= KB {
        format!("{:.1} KB", size / KB)
    } else {
        format!("{} B", size_bytes.max(0))
    }
}
