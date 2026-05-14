use crate::notifications::NotificationCenter;
use crate::panels::terminal_palette::palette_for_theme;
use crate::panels::{PanelRenderer, RenderCtx};
use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::{DockArea, DockState, NodeIndex, Style, SurfaceIndex};
use egui_term::{BackendSettings, PtyEvent, TerminalBackend, TerminalTheme, TerminalView};
use klaw_util::default_workspace_dir;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

const MIN_TERMINAL_HEIGHT: f32 = 220.0;

struct TerminalSession {
    backend: TerminalBackend,
    event_rx: Receiver<(u64, PtyEvent)>,
    pty_id: u32,
    shell: String,
    working_directory: PathBuf,
}

impl TerminalSession {
    fn start(id: u64, ctx: &egui::Context) -> Result<Self, String> {
        let working_directory = resolve_working_directory()?;
        let shell = default_shell();
        let (event_tx, event_rx) = mpsc::channel();
        let backend = TerminalBackend::new(
            id,
            ctx.clone(),
            event_tx,
            BackendSettings {
                shell: shell.clone(),
                args: Vec::new(),
                working_directory: Some(working_directory.clone()),
            },
        )
        .map_err(|err| err.to_string())?;
        let pty_id = backend.pty_id();

        Ok(Self {
            backend,
            event_rx,
            pty_id,
            shell,
            working_directory,
        })
    }
}

struct TerminalTab {
    id: u64,
    title: String,
    session: Option<TerminalSession>,
    start_error: Option<String>,
    exit_notice: Option<String>,
}

impl TerminalTab {
    fn start(id: u64, ctx: &egui::Context) -> Self {
        match TerminalSession::start(id, ctx) {
            Ok(session) => Self {
                id,
                title: format!("Terminal {id}"),
                session: Some(session),
                start_error: None,
                exit_notice: None,
            },
            Err(err) => Self {
                id,
                title: format!("Terminal {id}"),
                session: None,
                start_error: Some(err),
                exit_notice: None,
            },
        }
    }

    fn poll_events(&mut self) -> bool {
        let mut exited = false;
        while let Some(session) = self.session.as_mut() {
            match session.event_rx.try_recv() {
                Ok((_, PtyEvent::Exit)) => {
                    exited = true;
                    break;
                }
                Ok(_) => {}
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    exited = true;
                    break;
                }
            }
        }

        if exited {
            self.session = None;
            self.exit_notice = Some("Shell exited. Close this tab or open a new terminal.".into());
            self.title = format!("Terminal {} (exited)", self.id);
        }

        exited
    }
}

pub struct TerminalPanel {
    dock_state: DockState<TerminalTab>,
    next_session_id: u64,
}

impl Default for TerminalPanel {
    fn default() -> Self {
        Self {
            dock_state: DockState::new(Vec::new()),
            next_session_id: 1,
        }
    }
}

impl TerminalPanel {
    fn ensure_initial_tab(&mut self, ctx: &egui::Context, notifications: &mut NotificationCenter) {
        if self.dock_state.iter_all_tabs().next().is_none() {
            self.add_session_tab(ctx, notifications);
        }
    }

    fn add_session_tab(&mut self, ctx: &egui::Context, notifications: &mut NotificationCenter) {
        let id = self.next_session_id;
        self.next_session_id += 1;
        let tab = TerminalTab::start(id, ctx);
        if let Some(err) = &tab.start_error {
            notifications.error(format!("Failed to start terminal: {err}"));
        }
        self.dock_state.push_to_focused_leaf(tab);
    }

    fn poll_events(&mut self, notifications: &mut NotificationCenter) {
        let mut exited_tabs = Vec::new();
        for (_, tab) in self.dock_state.iter_all_tabs_mut() {
            if tab.poll_events() {
                exited_tabs.push(tab.id);
            }
        }

        for id in exited_tabs {
            notifications.info(format!("Terminal {id} shell exited"));
        }
    }
}

impl PanelRenderer for TerminalPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        let egui_ctx = ui.ctx().clone();
        self.ensure_initial_tab(&egui_ctx, notifications);
        self.poll_events(notifications);

        ui.heading(ctx.tab_title);
        ui.separator();

        let mut add_requested = false;
        let mut style = Style::from_egui(ui.style().as_ref());
        style.tab_bar.show_scroll_bar_on_overflow = false;

        DockArea::new(&mut self.dock_state)
            .id(egui::Id::new("terminal_panel_dock"))
            .style(style)
            .show_add_buttons(true)
            .show_close_buttons(true)
            .show_leaf_close_all_buttons(false)
            .show_leaf_collapse_buttons(false)
            .show_inside(
                ui,
                &mut TerminalTabViewer {
                    is_dark_mode: ctx.is_dark_mode,
                    light_theme: ctx.light_theme,
                    dark_theme: ctx.dark_theme,
                    add_requested: &mut add_requested,
                },
            );

        if add_requested {
            self.add_session_tab(&egui_ctx, notifications);
        }
    }

    fn on_tab_closed(&mut self) {
        self.dock_state = DockState::new(Vec::new());
    }
}

struct TerminalTabViewer<'a> {
    is_dark_mode: bool,
    light_theme: crate::state::LightThemePreset,
    dark_theme: crate::state::DarkThemePreset,
    add_requested: &'a mut bool,
}

impl egui_dock::TabViewer for TerminalTabViewer<'_> {
    type Tab = TerminalTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.title.clone().into()
    }

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(("terminal-tab", tab.id))
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        if let Some(session) = &tab.session {
            ui.horizontal_wrapped(|ui| {
                ui.label("Shell:");
                ui.monospace(&session.shell);
                ui.separator();
                ui.label("PTY:");
                ui.monospace(session.pty_id.to_string());
                ui.separator();
                ui.label("Working Directory:");
                ui.monospace(session.working_directory.display().to_string());
            });
            ui.add_space(8.0);
        } else if let Some(message) = &tab.exit_notice {
            ui.label(message);
            ui.add_space(8.0);
        }

        if let Some(message) = &tab.start_error {
            ui.colored_label(ui.visuals().error_fg_color, message);
            return;
        }

        let Some(session) = tab.session.as_mut() else {
            ui.label("Terminal is not running.");
            return;
        };

        let terminal_size = egui::vec2(
            ui.available_width(),
            ui.available_height().max(MIN_TERMINAL_HEIGHT),
        );
        let palette = palette_for_theme(self.is_dark_mode, self.light_theme, self.dark_theme);
        let terminal = TerminalView::new(ui, &mut session.backend)
            .set_focus(true)
            .set_size(terminal_size)
            .set_theme(TerminalTheme::new(Box::new(palette)));
        ui.add(terminal);
    }

    fn on_close(&mut self, _tab: &mut Self::Tab) -> OnCloseResponse {
        OnCloseResponse::Close
    }

    fn on_add(&mut self, _surface: SurfaceIndex, _node: NodeIndex) {
        *self.add_requested = true;
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false]
    }
}

fn resolve_working_directory() -> Result<PathBuf, String> {
    if let Some(workspace_dir) = default_workspace_dir() {
        fs::create_dir_all(&workspace_dir).map_err(|err| {
            format!(
                "failed to create workspace dir {}: {err}",
                workspace_dir.display()
            )
        })?;
        return Ok(workspace_dir);
    }

    std::env::current_dir().map_err(|err| format!("failed to resolve current directory: {err}"))
}

fn default_shell() -> String {
    #[cfg(unix)]
    {
        if let Some(shell) = std::env::var_os("SHELL").filter(|shell| !shell.is_empty()) {
            return shell.to_string_lossy().into_owned();
        }
        for fallback in ["/bin/zsh", "/bin/bash", "/bin/sh"] {
            if std::path::Path::new(fallback).exists() {
                return fallback.to_string();
            }
        }
        "/bin/sh".to_string()
    }

    #[cfg(windows)]
    {
        "cmd.exe".to_string()
    }
}
