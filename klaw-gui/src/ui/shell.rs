use crate::notifications::NotificationCenter;
use crate::panels::PanelRegistry;
use crate::state::{ThemeMode, UiAction, UiState};
use crate::ui::{sidebar, workbench};
use egui_phosphor::regular;
use klaw_config::{ConfigSnapshot, ConfigStore};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub struct ShellUi {
    panels: PanelRegistry,
    notifications: NotificationCenter,
    provider_store: Option<ConfigStore>,
    provider_ids: Vec<String>,
    config_default_provider: String,
    provider_default_models: BTreeMap<String, String>,
    last_provider_sync_at: Instant,
}

const PROVIDER_SYNC_INTERVAL: Duration = Duration::from_secs(2);

impl Default for ShellUi {
    fn default() -> Self {
        Self {
            panels: PanelRegistry::default(),
            notifications: NotificationCenter::default(),
            provider_store: None,
            provider_ids: Vec::new(),
            config_default_provider: String::new(),
            provider_default_models: BTreeMap::new(),
            last_provider_sync_at: Instant::now() - PROVIDER_SYNC_INTERVAL,
        }
    }
}

impl ShellUi {
    fn sync_provider_choices(&mut self) {
        if self.last_provider_sync_at.elapsed() < PROVIDER_SYNC_INTERVAL {
            return;
        }
        self.last_provider_sync_at = Instant::now();

        match self.provider_store.as_ref() {
            Some(store) => {
                if let Ok(snapshot) = store.reload() {
                    self.apply_provider_snapshot(snapshot);
                }
            }
            None => {
                if let Ok(store) = ConfigStore::open(None) {
                    let snapshot = store.snapshot();
                    self.provider_store = Some(store);
                    self.apply_provider_snapshot(snapshot);
                }
            }
        }
    }

    fn apply_provider_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config_default_provider = snapshot.config.model_provider;
        self.provider_ids = snapshot
            .config
            .model_providers
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        self.provider_ids.sort();
        self.provider_default_models = snapshot
            .config
            .model_providers
            .into_iter()
            .map(|(provider_id, provider)| (provider_id, provider.default_model))
            .collect();
    }

    pub fn render(&mut self, ctx: &egui::Context, state: &UiState) -> Vec<UiAction> {
        let mut actions = Vec::new();
        self.sync_provider_choices();

        egui::TopBottomPanel::top("klaw-menu-bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Force Persist Layout").clicked() {
                        actions.push(UiAction::ForcePersistLayout);
                        ui.close();
                    }
                    ui.separator();
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

                    ui.separator();
                    if self.provider_ids.is_empty() {
                        ui.label("Model Provider: N/A");
                    } else {
                        let default_provider = if self.config_default_provider.is_empty() {
                            "unknown"
                        } else {
                            self.config_default_provider.as_str()
                        };
                        let selected_provider_id = state
                            .runtime_provider_override
                            .as_deref()
                            .unwrap_or(default_provider);
                        let selected_text = selected_provider_id.to_string();

                        egui::ComboBox::from_id_salt("runtime-provider-override")
                            .width(180.0)
                            .selected_text(selected_text)
                            .show_ui(ui, |ui| {
                                for provider_id in &self.provider_ids {
                                    let selected = selected_provider_id == provider_id;
                                    if ui.selectable_label(selected, provider_id).clicked() {
                                        if provider_id == default_provider {
                                            actions
                                                .push(UiAction::SetRuntimeProviderOverride(None));
                                        } else {
                                            actions.push(UiAction::SetRuntimeProviderOverride(
                                                Some(provider_id.clone()),
                                            ));
                                        }
                                        ui.close();
                                    }
                                }
                            });

                        ui.label("Model Provider:");

                        ui.separator();

                        let default_model = self
                            .provider_default_models
                            .get(selected_provider_id)
                            .map(String::as_str)
                            .unwrap_or("N/A");
                        ui.label(format!("Default Model: {default_model}"));
                    }
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
            egui::Window::new("About Klaw")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("{} Klaw", regular::INFO));
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
