use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use egui_term::{BackendSettings, PtyEvent, TerminalBackend, TerminalView};
use klaw_util::default_workspace_dir;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

const TERMINAL_ID: u64 = 0;
const MIN_TERMINAL_HEIGHT: f32 = 220.0;

struct TerminalSession {
    backend: TerminalBackend,
    event_rx: Receiver<(u64, PtyEvent)>,
    pty_id: u32,
    shell: String,
    working_directory: PathBuf,
}

#[derive(Default)]
pub struct TerminalPanel {
    session: Option<TerminalSession>,
    start_error: Option<String>,
    exit_notice: Option<String>,
}

impl TerminalPanel {
    fn ensure_session(&mut self, ctx: &egui::Context, notifications: &mut NotificationCenter) {
        if self.session.is_some() || self.start_error.is_some() {
            return;
        }

        if let Err(err) = self.start_session(ctx) {
            self.start_error = Some(err.clone());
            notifications.error(format!("Failed to start terminal: {err}"));
        }
    }

    fn start_session(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let working_directory = resolve_working_directory()?;
        let shell = default_shell();
        let (event_tx, event_rx) = mpsc::channel();
        let backend = TerminalBackend::new(
            TERMINAL_ID,
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

        self.exit_notice = None;
        self.start_error = None;
        self.session = Some(TerminalSession {
            backend,
            event_rx,
            pty_id,
            shell,
            working_directory,
        });
        Ok(())
    }

    fn restart_session(&mut self, ctx: &egui::Context, notifications: &mut NotificationCenter) {
        self.stop_session();
        match self.start_session(ctx) {
            Ok(()) => notifications.success("Terminal restarted"),
            Err(err) => {
                self.start_error = Some(err.clone());
                notifications.error(format!("Failed to restart terminal: {err}"));
            }
        }
    }

    fn stop_session(&mut self) {
        self.session = None;
    }

    fn poll_events(&mut self, notifications: &mut NotificationCenter) {
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
            self.stop_session();
            self.exit_notice =
                Some("Shell exited. Start or restart the terminal to continue.".into());
            notifications.info("Terminal shell exited");
        }
    }

    fn render_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
        ctx: &egui::Context,
    ) {
        let running = self.session.is_some();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!running, egui::Button::new("Start"))
                .clicked()
            {
                self.start_error = None;
                self.ensure_session(ctx, notifications);
            }
            if ui.button("Restart").clicked() {
                self.restart_session(ctx, notifications);
            }
            if ui.add_enabled(running, egui::Button::new("Stop")).clicked() {
                self.stop_session();
                self.exit_notice = Some("Terminal stopped.".to_string());
                notifications.info("Terminal stopped");
            }
        });
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
        self.ensure_session(&egui_ctx, notifications);
        self.poll_events(notifications);

        ui.heading(ctx.tab_title);
        self.render_toolbar(ui, notifications, &egui_ctx);
        ui.separator();

        if let Some(session) = &self.session {
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
        } else if let Some(message) = &self.exit_notice {
            ui.label(message);
            ui.add_space(8.0);
        }

        if let Some(message) = &self.start_error {
            ui.colored_label(ui.visuals().error_fg_color, message);
            return;
        }

        let Some(session) = self.session.as_mut() else {
            ui.label("Terminal is not running.");
            return;
        };

        let terminal_size = egui::vec2(
            ui.available_width(),
            ui.available_height().max(MIN_TERMINAL_HEIGHT),
        );
        let terminal = TerminalView::new(ui, &mut session.backend)
            .set_focus(true)
            .set_size(terminal_size);
        ui.add(terminal);
    }

    fn on_tab_closed(&mut self) {
        self.stop_session();
        self.exit_notice = Some("Terminal closed with the tab.".to_string());
        self.start_error = None;
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
