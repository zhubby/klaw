use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::settings::{load_settings, save_settings, AppSettings, ProxyMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsSection {
    General,
    Privacy,
    Security,
    Network,
    Sync,
}

impl SettingsSection {
    fn title(&self) -> &'static str {
        match self {
            SettingsSection::General => "General",
            SettingsSection::Privacy => "Privacy",
            SettingsSection::Security => "Security",
            SettingsSection::Network => "Network",
            SettingsSection::Sync => "Sync",
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            SettingsSection::General => "\u{2699}",        // Gear
            SettingsSection::Privacy => "\u{1F512}",      // Lock
            SettingsSection::Security => "\u{1F6E1}",     // Shield
            SettingsSection::Network => "\u{1F310}",      // Globe
            SettingsSection::Sync => "\u{1F504}",         // Sync arrows
        }
    }
}

pub struct SettingPanel {
    settings: AppSettings,
    active_section: SettingsSection,
    save_error: Option<String>,
}

impl Default for SettingPanel {
    fn default() -> Self {
        let settings = load_settings();
        Self {
            settings,
            active_section: SettingsSection::General,
            save_error: None,
        }
    }
}

impl PanelRenderer for SettingPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        _notifications: &mut NotificationCenter,
    ) {
        ui.heading(ctx.tab_title);
        ui.label("Configure application preferences");
        ui.separator();

        if let Some(err) = &self.save_error {
            ui.colored_label(ui.style().visuals.error_fg_color, format!("Save error: {}", err));
        }

        // Two-column layout: sidebar on left, content on right
        ui.horizontal(|ui| {
            // Left sidebar with section buttons
            ui.vertical(|ui| {
                ui.set_min_width(140.0);
                ui.set_max_width(160.0);
                for section in [
                    SettingsSection::General,
                    SettingsSection::Privacy,
                    SettingsSection::Security,
                    SettingsSection::Network,
                    SettingsSection::Sync,
                ] {
                    let is_active = self.active_section == section;
                    let text = format!("{} {}", section.icon(), section.title());
                    if ui.selectable_label(is_active, text).clicked() {
                        self.active_section = section;
                    }
                }
            });

            ui.separator();

            // Right content area
            ui.vertical(|ui| {
                ui.set_min_width(400.0);

                match self.active_section {
                    SettingsSection::General => self.render_general_section(ui),
                    SettingsSection::Privacy => self.render_privacy_section(ui),
                    SettingsSection::Security => self.render_security_section(ui),
                    SettingsSection::Network => self.render_network_section(ui),
                    SettingsSection::Sync => self.render_sync_section(ui),
                }
            });
        });
    }
}

impl SettingPanel {
    fn try_save(&mut self) {
        match save_settings(&self.settings) {
            Ok(()) => {
                self.save_error = None;
            }
            Err(err) => {
                self.save_error = Some(err.to_string());
            }
        }
    }

    fn render_general_section(&mut self, ui: &mut egui::Ui) {
        ui.strong("General Settings");
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label("Launch at startup:");
            if ui
                .radio_value(&mut self.settings.general.launch_at_startup, true, "Yes")
                .changed()
                || ui
                    .radio_value(&mut self.settings.general.launch_at_startup, false, "No")
                    .changed()
            {
                self.try_save();
            }
        });

        ui.add_space(8.0);
        ui.label("Automatically start Klaw when you log in to your computer.");
    }

    fn render_privacy_section(&mut self, ui: &mut egui::Ui) {
        ui.strong("Privacy Settings");
        ui.add_space(8.0);
        ui.label("Privacy settings are not yet configured.");
        ui.add_space(8.0);
        ui.label("Future options may include:");
        ui.label("\u{2022} Data collection preferences");
        ui.label("\u{2022} Analytics opt-out");
        ui.label("\u{2022} Crash reporting");
    }

    fn render_security_section(&mut self, ui: &mut egui::Ui) {
        ui.strong("Security Settings");
        ui.add_space(8.0);
        ui.label("Security settings are not yet configured.");
        ui.add_space(8.0);
        ui.label("Future options may include:");
        ui.label("\u{2022} API key encryption");
        ui.label("\u{2022} Session timeout");
        ui.label("\u{2022} Two-factor authentication");
    }

    fn render_network_section(&mut self, ui: &mut egui::Ui) {
        ui.strong("Network Settings");
        ui.add_space(8.0);

        ui.label("Proxy Configuration:");
        ui.add_space(4.0);

        if ui
            .radio_value(
                &mut self.settings.network.proxy_mode,
                ProxyMode::NoProxy,
                "No proxy",
            )
            .changed()
            || ui
                .radio_value(
                    &mut self.settings.network.proxy_mode,
                    ProxyMode::SystemProxy,
                    "Use system proxy",
                )
                .changed()
            || ui
                .radio_value(
                    &mut self.settings.network.proxy_mode,
                    ProxyMode::ManualProxy,
                    "Manual proxy configuration",
                )
                .changed()
        {
            self.try_save();
        }

        // Show proxy config fields when ManualProxy is selected
        if self.settings.network.proxy_mode == ProxyMode::ManualProxy {
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // HTTP Proxy
            ui.group(|ui| {
                ui.strong("HTTP Proxy");
                if render_proxy_fields(ui, &mut self.settings.network.http_proxy) {
                    self.try_save();
                }
            });

            ui.add_space(8.0);

            // HTTPS Proxy
            ui.group(|ui| {
                ui.strong("HTTPS Proxy");
                if render_proxy_fields(ui, &mut self.settings.network.https_proxy) {
                    self.try_save();
                }
            });

            ui.add_space(8.0);

            // SOCKS5 Proxy
            ui.group(|ui| {
                ui.strong("SOCKS5 Proxy");
                if render_proxy_fields(ui, &mut self.settings.network.socks5_proxy) {
                    self.try_save();
                }
            });
        }
    }

    fn render_sync_section(&mut self, ui: &mut egui::Ui) {
        ui.strong("Sync Settings");
        ui.add_space(8.0);
        ui.label("Select items to backup:");
        ui.add_space(8.0);

        let mut changed = false;
        for item in crate::settings::SyncItem::all() {
            let index = self.settings.sync.backup_items.iter().position(|i| i == item);
            let mut is_checked = index.is_some();
            if ui.checkbox(&mut is_checked, item.label()).clicked() {
                if is_checked && index.is_none() {
                    self.settings.sync.backup_items.push(*item);
                    changed = true;
                } else if !is_checked {
                    if let Some(idx) = index {
                        self.settings.sync.backup_items.remove(idx);
                        changed = true;
                    }
                }
            }
        }

        if changed {
            self.try_save();
        }
    }
}

fn render_proxy_fields(ui: &mut egui::Ui, config: &mut crate::settings::ProxyConfig) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label("Host:");
        if ui.text_edit_singleline(&mut config.host).changed() {
            changed = true;
        }
    });

    ui.horizontal(|ui| {
        ui.label("Port:");
        let mut port_str = if config.port == 0 {
            String::new()
        } else {
            config.port.to_string()
        };
        if ui.text_edit_singleline(&mut port_str).changed() {
            if port_str.is_empty() {
                config.port = 0;
                changed = true;
            } else if let Ok(port) = port_str.parse::<u16>() {
                config.port = port;
                changed = true;
            }
        }
    });

    changed
}