use crate::notifications::NotificationCenter;
use crate::panels::terminal_palette::palette_for_theme;
use crate::panels::{PanelRenderer, RenderCtx};
use egui::scroll_area::ScrollBarVisibility;
use egui_term::{BackendSettings, PtyEvent, TerminalBackend, TerminalTheme, TerminalView};
use klaw_util::default_workspace_dir;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

const MIN_TERMINAL_HEIGHT: f32 = 220.0;
const MAX_PTY_EVENTS_PER_FRAME: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TerminalTabId(u64);

struct TerminalSession {
    backend: TerminalBackend,
    event_rx: Receiver<(u64, PtyEvent)>,
    pty_id: u32,
    shell: String,
    working_directory: PathBuf,
}

struct TerminalTab {
    id: TerminalTabId,
    backend_id: u64,
    tty_number: u64,
    alias: Option<String>,
    session: Option<TerminalSession>,
    start_error: Option<String>,
    exit_notice: Option<String>,
}

impl TerminalTab {
    fn label(&self) -> String {
        self.alias
            .clone()
            .unwrap_or_else(|| format!("tty {}", self.tty_number))
    }
}

pub struct TerminalPanel {
    tabs: Vec<TerminalTab>,
    active_tab: Option<TerminalTabId>,
    next_tab_id: u64,
    next_tty_number: u64,
    next_backend_id: u64,
    rename_tab_id: Option<TerminalTabId>,
    rename_input: String,
}

impl Default for TerminalPanel {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: None,
            next_tab_id: 1,
            next_tty_number: 1,
            next_backend_id: 1,
            rename_tab_id: None,
            rename_input: String::new(),
        }
    }
}

impl TerminalPanel {
    fn ensure_initial_tab(&mut self, ctx: &egui::Context, notifications: &mut NotificationCenter) {
        if self.tabs.is_empty() {
            let tab_id = self.add_tab_state();
            self.start_tab_session(tab_id, ctx, notifications);
        }
    }

    fn add_tab_state(&mut self) -> TerminalTabId {
        let id = TerminalTabId(self.next_tab_id);
        self.next_tab_id += 1;
        let tty_number = self.next_tty_number;
        self.next_tty_number += 1;
        let backend_id = self.next_backend_id;
        self.next_backend_id += 1;

        self.tabs.push(TerminalTab {
            id,
            backend_id,
            tty_number,
            alias: None,
            session: None,
            start_error: None,
            exit_notice: None,
        });
        self.active_tab = Some(id);
        id
    }

    fn add_session_tab(&mut self, ctx: &egui::Context, notifications: &mut NotificationCenter) {
        let tab_id = self.add_tab_state();
        self.start_tab_session(tab_id, ctx, notifications);
    }

    fn active_tab_mut(&mut self) -> Option<&mut TerminalTab> {
        let active_tab = self.active_tab?;
        self.tabs.iter_mut().find(|tab| tab.id == active_tab)
    }

    fn active_tab_ref(&self) -> Option<&TerminalTab> {
        let active_tab = self.active_tab?;
        self.tabs.iter().find(|tab| tab.id == active_tab)
    }

    fn tab_mut(&mut self, tab_id: TerminalTabId) -> Option<&mut TerminalTab> {
        self.tabs.iter_mut().find(|tab| tab.id == tab_id)
    }

    fn tab_label(&self, tab_id: TerminalTabId) -> Option<String> {
        self.tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .map(TerminalTab::label)
    }

    #[cfg(test)]
    fn backend_id(&self, tab_id: TerminalTabId) -> Option<u64> {
        self.tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .map(|tab| tab.backend_id)
    }

    fn activate_tab(&mut self, tab_id: TerminalTabId) {
        if self.tabs.iter().any(|tab| tab.id == tab_id) {
            self.active_tab = Some(tab_id);
        }
    }

    fn rename_tab(&mut self, tab_id: TerminalTabId, alias: &str) {
        let trimmed = alias.trim();
        if let Some(tab) = self.tab_mut(tab_id) {
            tab.alias = (!trimmed.is_empty()).then(|| trimmed.to_string());
        }
    }

    fn close_tab(&mut self, tab_id: TerminalTabId) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };

        self.tabs.remove(index);

        if self.rename_tab_id == Some(tab_id) {
            self.rename_tab_id = None;
            self.rename_input.clear();
        }

        if self.active_tab != Some(tab_id) {
            return;
        }

        self.active_tab = self
            .tabs
            .get(index.min(self.tabs.len().saturating_sub(1)))
            .map(|tab| tab.id);
    }

    fn ensure_active_session(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        let Some(active_tab) = self.active_tab_ref() else {
            return;
        };
        if active_tab.session.is_some()
            || active_tab.start_error.is_some()
            || active_tab.exit_notice.is_some()
        {
            return;
        }
        self.start_tab_session(active_tab.id, ctx, notifications);
    }

    fn start_session_for(
        tab: &TerminalTab,
        ctx: &egui::Context,
    ) -> Result<TerminalSession, String> {
        let working_directory = resolve_working_directory()?;
        let shell = default_shell();
        let (event_tx, event_rx) = mpsc::channel();
        let backend = TerminalBackend::new(
            tab.backend_id,
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

        Ok(TerminalSession {
            backend,
            event_rx,
            pty_id,
            shell,
            working_directory,
        })
    }

    fn start_tab_session(
        &mut self,
        tab_id: TerminalTabId,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id) else {
            return;
        };
        let label = tab.label();

        match Self::start_session_for(tab, ctx) {
            Ok(session) => {
                if let Some(tab) = self.tab_mut(tab_id) {
                    tab.session = Some(session);
                    tab.start_error = None;
                    tab.exit_notice = None;
                }
            }
            Err(err) => {
                if let Some(tab) = self.tab_mut(tab_id) {
                    tab.start_error = Some(err.clone());
                    tab.session = None;
                }
                notifications.error(format!("Failed to start {label}: {err}"));
            }
        }
    }

    fn poll_events(&mut self, notifications: &mut NotificationCenter) {
        let mut exited_tabs = Vec::new();
        let mut remaining_events = MAX_PTY_EVENTS_PER_FRAME;
        for tab in &mut self.tabs {
            let mut exited = false;
            while remaining_events > 0 {
                let Some(session) = tab.session.as_mut() else {
                    break;
                };
                match session.event_rx.try_recv() {
                    Ok((_, PtyEvent::Exit)) => {
                        remaining_events -= 1;
                        exited = true;
                        break;
                    }
                    Ok(_) => {
                        remaining_events -= 1;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        exited = true;
                        break;
                    }
                }
            }

            if exited {
                tab.session = None;
                tab.exit_notice = Some("Shell exited. Open a new terminal tab to continue.".into());
                exited_tabs.push(tab.label());
            }

            if remaining_events == 0 {
                break;
            }
        }

        for label in exited_tabs {
            notifications.info(format!("{label} shell exited"));
        }
    }

    fn render_tab_strip(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        let mut activate_tab = None;
        let mut close_tab = None;
        let mut rename_tab = None;
        ui.horizontal(|ui| {
            let button_width = ui.spacing().interact_size.x;
            let strip_width =
                (ui.available_width() - button_width - ui.spacing().item_spacing.x).max(0.0);
            ui.allocate_ui_with_layout(
                egui::vec2(strip_width, ui.spacing().interact_size.y + 14.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt("terminal-tab-strip")
                        .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                for tab in &self.tabs {
                                    let is_active = self.active_tab == Some(tab.id);
                                    let label = tab.label();
                                    let visuals = ui.visuals();
                                    let fill = if is_active {
                                        visuals.selection.bg_fill
                                    } else {
                                        visuals.widgets.inactive.bg_fill
                                    };
                                    let stroke = if is_active {
                                        visuals.selection.stroke
                                    } else {
                                        visuals.widgets.inactive.bg_stroke
                                    };
                                    let tab_response = egui::Frame::new()
                                        .fill(fill)
                                        .stroke(stroke)
                                        .corner_radius(4.0)
                                        .inner_margin(egui::Margin::symmetric(8, 4))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 6.0;
                                                ui.label(label);
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            egui::RichText::new("x").small(),
                                                        )
                                                        .frame(false),
                                                    )
                                                    .on_hover_text("Close terminal session")
                                                    .clicked()
                                                {
                                                    close_tab = Some(tab.id);
                                                }
                                            });
                                        })
                                        .response
                                        .interact(egui::Sense::click());

                                    if tab_response.clicked() {
                                        activate_tab = Some(tab.id);
                                    }
                                    tab_response.context_menu(|ui| {
                                        if ui.button("Rename").clicked() {
                                            rename_tab = Some(tab.id);
                                            ui.close();
                                        }
                                    });
                                }
                            });
                        });
                },
            );
            let visuals = ui.visuals();
            let add_response = egui::Frame::new()
                .fill(visuals.widgets.inactive.bg_fill)
                .stroke(visuals.widgets.inactive.bg_stroke)
                .corner_radius(4.0)
                .inner_margin(egui::Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("+").strong());
                })
                .response
                .interact(egui::Sense::click())
                .on_hover_text("New terminal session");
            if add_response.clicked() {
                self.add_session_tab(ctx, notifications);
            }
        });

        if let Some(tab_id) = activate_tab {
            self.activate_tab(tab_id);
        }
        if let Some(tab_id) = close_tab {
            self.close_tab(tab_id);
            if self.tabs.is_empty() {
                self.add_session_tab(ctx, notifications);
            }
        }
        if let Some(tab_id) = rename_tab
            && let Some(label) = self.tab_label(tab_id)
        {
            self.rename_tab_id = Some(tab_id);
            self.rename_input = label;
        }
    }

    fn render_rename_dialog(&mut self, ctx: &egui::Context) {
        let Some(tab_id) = self.rename_tab_id else {
            return;
        };

        let mut open = true;
        let mut submit = false;
        let mut cancel = false;

        egui::Window::new("Rename Terminal")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.rename_input)
                        .desired_width(f32::INFINITY)
                        .hint_text("Terminal alias"),
                );
                let submit_with_enter =
                    response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
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
            let alias = self.rename_input.clone();
            self.rename_tab(tab_id, &alias);
            self.rename_tab_id = None;
            self.rename_input.clear();
            return;
        }

        if cancel || !open {
            self.rename_tab_id = None;
            self.rename_input.clear();
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
        self.ensure_active_session(&egui_ctx, notifications);
        self.poll_events(notifications);

        ui.heading(ctx.tab_title);
        self.render_tab_strip(ui, &egui_ctx, notifications);
        self.render_rename_dialog(&egui_ctx);
        ui.separator();

        if let Some(session) = self.active_tab_ref().and_then(|tab| tab.session.as_ref()) {
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
        } else if let Some(message) = self
            .active_tab_ref()
            .and_then(|tab| tab.exit_notice.as_ref())
        {
            ui.label(message);
            ui.add_space(8.0);
        }

        if let Some(message) = self
            .active_tab_ref()
            .and_then(|tab| tab.start_error.as_ref())
        {
            ui.colored_label(ui.visuals().error_fg_color, message);
            return;
        }

        let terminal_should_focus = self.rename_tab_id.is_none();
        let Some(tab) = self.active_tab_mut() else {
            ui.label("No terminal sessions.");
            return;
        };
        let Some(session) = tab.session.as_mut() else {
            ui.label("Terminal is not running.");
            return;
        };

        let terminal_size = egui::vec2(
            ui.available_width(),
            ui.available_height().max(MIN_TERMINAL_HEIGHT),
        );
        let palette = palette_for_theme(ctx.is_dark_mode, ctx.light_theme, ctx.dark_theme);
        let terminal = TerminalView::new(ui, &mut session.backend)
            .set_focus(terminal_should_focus)
            .set_size(terminal_size)
            .set_theme(TerminalTheme::new(Box::new(palette)));
        ui.add(terminal);
    }

    fn on_tab_closed(&mut self) {
        self.tabs.clear();
        self.active_tab = None;
        self.rename_tab_id = None;
        self.rename_input.clear();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_tabs_allocate_stable_labels_and_unique_backend_ids() {
        let mut panel = TerminalPanel::default();

        let first = panel.add_tab_state();
        let second = panel.add_tab_state();
        let third = panel.add_tab_state();

        assert_eq!(panel.tab_label(first), Some("tty 1".to_string()));
        assert_eq!(panel.tab_label(second), Some("tty 2".to_string()));
        assert_eq!(panel.tab_label(third), Some("tty 3".to_string()));
        assert_eq!(panel.backend_id(first), Some(1));
        assert_eq!(panel.backend_id(second), Some(2));
        assert_eq!(panel.backend_id(third), Some(3));
        assert_eq!(panel.active_tab, Some(third));
    }

    #[test]
    fn terminal_tab_alias_overrides_and_empty_alias_resets_to_default() {
        let mut panel = TerminalPanel::default();
        let tab_id = panel.add_tab_state();

        panel.rename_tab(tab_id, "build logs");
        assert_eq!(panel.tab_label(tab_id), Some("build logs".to_string()));

        panel.rename_tab(tab_id, "   ");
        assert_eq!(panel.tab_label(tab_id), Some("tty 1".to_string()));
    }

    #[test]
    fn closing_active_terminal_tab_activates_adjacent_tab() {
        let mut panel = TerminalPanel::default();
        let first = panel.add_tab_state();
        let second = panel.add_tab_state();
        let third = panel.add_tab_state();

        panel.activate_tab(second);
        panel.close_tab(second);

        assert_eq!(panel.active_tab, Some(third));
        assert_eq!(panel.tab_label(first), Some("tty 1".to_string()));
        assert_eq!(panel.tab_label(third), Some("tty 3".to_string()));
        assert_eq!(panel.tabs.len(), 2);
    }

    #[test]
    fn closing_inactive_terminal_tab_keeps_active_tab() {
        let mut panel = TerminalPanel::default();
        let first = panel.add_tab_state();
        let second = panel.add_tab_state();

        panel.activate_tab(first);
        panel.close_tab(second);

        assert_eq!(panel.active_tab, Some(first));
        assert_eq!(panel.tab_label(first), Some("tty 1".to_string()));
        assert_eq!(panel.tabs.len(), 1);
    }

    #[test]
    fn closing_last_terminal_tab_clears_active_and_pending_rename() {
        let mut panel = TerminalPanel::default();
        let tab_id = panel.add_tab_state();
        panel.rename_tab_id = Some(tab_id);
        panel.rename_input = "scratch".to_string();

        panel.close_tab(tab_id);

        assert!(panel.tabs.is_empty());
        assert!(panel.active_tab.is_none());
        assert!(panel.rename_tab_id.is_none());
        assert!(panel.rename_input.is_empty());
    }
}
