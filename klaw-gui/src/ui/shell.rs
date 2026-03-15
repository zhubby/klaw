use crate::notifications::NotificationCenter;
use crate::panels::PanelRegistry;
use crate::state::{ThemeMode, UiAction, UiState};
use crate::ui::{sidebar, workbench};
use egui_phosphor::regular;

#[derive(Default)]
pub struct ShellUi {
    panels: PanelRegistry,
    notifications: NotificationCenter,
}

impl ShellUi {
    pub fn render(&mut self, ctx: &egui::Context, state: &UiState) -> Vec<UiAction> {
        let mut actions = Vec::new();

        egui::TopBottomPanel::top("klaw-menu-bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Close Windows").clicked() {
                        actions.push(UiAction::CloseWindow);
                        ui.close();
                    }
                });

                ui.menu_button("View", |ui| {
                    let label = if state.fullscreen {
                        "Exit Full Windows"
                    } else {
                        "Toggle Full Windows"
                    };
                    if ui.button(label).clicked() {
                        actions.push(UiAction::ToggleFullscreen);
                        ui.close();
                    }
                });

                ui.menu_button("Windows", |ui| {
                    if ui.button("Minimize").clicked() {
                        actions.push(UiAction::MinimizeWindow);
                        ui.close();
                    }
                    if ui.button("Zoom").clicked() {
                        actions.push(UiAction::ZoomWindow);
                        ui.close();
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("About").clicked() {
                        actions.push(UiAction::ShowAbout);
                        ui.close();
                    }
                });

                let row_height = ui.spacing().interact_size.y;
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), row_height),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if ui
                            .button(regular::X)
                            .on_hover_text("Close Window")
                            .clicked()
                        {
                            actions.push(UiAction::CloseWindow);
                        }

                        let zoom_icon = if state.fullscreen {
                            regular::ARROWS_IN
                        } else {
                            regular::ARROWS_OUT
                        };
                        if ui.button(zoom_icon).on_hover_text("Zoom Window").clicked() {
                            actions.push(UiAction::ZoomWindow);
                        }

                        if ui
                            .button(regular::MINUS)
                            .on_hover_text("Minimize Window")
                            .clicked()
                        {
                            actions.push(UiAction::MinimizeWindow);
                        }

                        let drag_size = egui::vec2(ui.available_width().max(0.0), row_height);
                        if drag_size.x > 0.0 {
                            let (_rect, drag_response) =
                                ui.allocate_exact_size(drag_size, egui::Sense::click_and_drag());
                            let pointer_pressed_on_region = drag_response.hovered()
                                && ui.input(|i| {
                                    i.pointer.button_pressed(egui::PointerButton::Primary)
                                });
                            if pointer_pressed_on_region {
                                actions.push(UiAction::StartWindowDrag);
                            }
                        }
                    },
                );
            });
        });

        egui::TopBottomPanel::bottom("klaw-status-bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let theme_icon = match state.theme_mode {
                    ThemeMode::System => regular::CIRCLE_HALF,
                    ThemeMode::Light => regular::SUN,
                    ThemeMode::Dark => regular::MOON,
                };

                let response = ui
                    .add(egui::Label::new(theme_icon).sense(egui::Sense::click()))
                    .on_hover_text("Theme: System -> Light -> Dark");
                if response.clicked() {
                    actions.push(UiAction::CycleTheme);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let version_label = format!("{} v{}", regular::INFO, env!("CARGO_PKG_VERSION"));
                    ui.label(version_label);
                });
            });
        });

        egui::SidePanel::left("klaw-sidebar")
            .resizable(true)
            .default_width(220.0)
            .show(ctx, |ui| {
                actions.extend(sidebar::show_sidebar(ui, state));
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            actions.extend(workbench::show_workbench(
                ui,
                state,
                &mut self.panels,
                &mut self.notifications,
            ));
        });

        if state.show_about {
            egui::Window::new("About Klaw Workbench")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("{} Klaw Workbench", regular::INFO));
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.label("Desktop UI shell built with egui.");
                    ui.add_space(8.0);
                    if ui.button("Close").clicked() {
                        actions.push(UiAction::HideAbout);
                    }
                });
        }

        self.notifications.show(ctx);

        actions
    }
}
