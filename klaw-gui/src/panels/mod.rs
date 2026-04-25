mod acp;
mod analyze_dashboard;
mod approval;
mod archive;
mod channel;
mod configuration;
mod cron;
mod gateway;
mod heartbeat;
mod llm;
mod local_models;
mod logs;
mod mcp;
mod memory;
mod monitor;
mod observability;
mod profile;
mod provider;
mod session;
mod setting;
mod skills_manager;
mod skills_registry;
mod system;
mod terminal;
mod tool;
mod voice;
mod webhook;

use crate::domain::menu::WorkbenchMenu;
use crate::notifications::NotificationCenter;

pub struct RenderCtx<'a> {
    pub menu: WorkbenchMenu,
    pub tab_title: &'a str,
}

pub trait PanelRenderer {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    );

    fn on_tab_closed(&mut self) {}
}

#[derive(Default)]
pub struct PanelRegistry {
    profile: profile::ProfilePanel,
    system: system::SystemPanel,
    setting: setting::SettingPanel,
    terminal: terminal::TerminalPanel,
    session: session::SessionPanel,
    approval: approval::ApprovalPanel,
    acp: acp::AcpPanel,
    configuration: configuration::ConfigurationPanel,
    provider: provider::ProviderPanel,
    channel: channel::ChannelPanel,
    voice: voice::VoicePanel,
    cron: cron::CronPanel,
    heartbeat: heartbeat::HeartbeatPanel,
    gateway: gateway::GatewayPanel,
    mcp: mcp::McpPanel,
    skills_registry: skills_registry::SkillsRegistryPanel,
    skills_manager: skills_manager::SkillsManagerPanel,
    memory: memory::MemoryPanel,
    archive: archive::ArchivePanel,
    tool: tool::ToolPanel,
    webhook: webhook::WebhookPanel,
    monitor: monitor::MonitorPanel,
    logs: logs::LogsPanel,
    llm: llm::LlmPanel,
    local_models: local_models::LocalModelsPanel,
    analyze_dashboard: analyze_dashboard::AnalyzeDashboardPanel,
    observability: observability::ObservabilityPanel,
}

impl PanelRegistry {
    pub fn tick(&mut self, ctx: &egui::Context) {
        puffin::profile_scope!("panels_tick");
        self.logs.tick(ctx);
    }

    pub fn render_for(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        match ctx.menu {
            WorkbenchMenu::Profile => {
                puffin::profile_scope!("panel_profile");
                self.profile.render(ui, ctx, notifications)
            }
            WorkbenchMenu::System => {
                puffin::profile_scope!("panel_system");
                self.system.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Setting => {
                puffin::profile_scope!("panel_setting");
                self.setting.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Terminal => {
                puffin::profile_scope!("panel_terminal");
                self.terminal.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Session => {
                puffin::profile_scope!("panel_session");
                self.session.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Approval => {
                puffin::profile_scope!("panel_approval");
                self.approval.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Acp => {
                puffin::profile_scope!("panel_acp");
                self.acp.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Configuration => {
                puffin::profile_scope!("panel_configuration");
                self.configuration.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Provider => {
                puffin::profile_scope!("panel_provider");
                self.provider.render(ui, ctx, notifications)
            }
            WorkbenchMenu::LocalModels => {
                puffin::profile_scope!("panel_local_models");
                self.local_models.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Llm => {
                puffin::profile_scope!("panel_llm");
                self.llm.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Channel => {
                puffin::profile_scope!("panel_channel");
                self.channel.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Voice => {
                puffin::profile_scope!("panel_voice");
                self.voice.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Cron => {
                puffin::profile_scope!("panel_cron");
                self.cron.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Heartbeat => {
                puffin::profile_scope!("panel_heartbeat");
                self.heartbeat.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Gateway => {
                puffin::profile_scope!("panel_gateway");
                self.gateway.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Webhook => {
                puffin::profile_scope!("panel_webhook");
                self.webhook.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Mcp => {
                puffin::profile_scope!("panel_mcp");
                self.mcp.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Skill => {
                puffin::profile_scope!("panel_skill");
                self.skills_registry.render(ui, ctx, notifications)
            }
            WorkbenchMenu::SkillsManager => {
                puffin::profile_scope!("panel_skills_manager");
                self.skills_manager.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Memory => {
                puffin::profile_scope!("panel_memory");
                self.memory.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Archive => {
                puffin::profile_scope!("panel_archive");
                self.archive.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Tool => {
                puffin::profile_scope!("panel_tool");
                self.tool.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Monitor => {
                puffin::profile_scope!("panel_monitor");
                self.monitor.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Logs => {
                puffin::profile_scope!("panel_logs");
                self.logs.render(ui, ctx, notifications)
            }
            WorkbenchMenu::AnalyzeDashboard => {
                puffin::profile_scope!("panel_analyze_dashboard");
                self.analyze_dashboard.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Observability => {
                puffin::profile_scope!("panel_observability");
                self.observability.render(ui, ctx, notifications)
            }
        }
    }

    pub fn handle_tab_closed(&mut self, menu: WorkbenchMenu) {
        match menu {
            WorkbenchMenu::Profile => self.profile.on_tab_closed(),
            WorkbenchMenu::System => self.system.on_tab_closed(),
            WorkbenchMenu::Setting => self.setting.on_tab_closed(),
            WorkbenchMenu::Terminal => self.terminal.on_tab_closed(),
            WorkbenchMenu::Session => self.session.on_tab_closed(),
            WorkbenchMenu::Approval => self.approval.on_tab_closed(),
            WorkbenchMenu::Configuration => self.configuration.on_tab_closed(),
            WorkbenchMenu::Provider => self.provider.on_tab_closed(),
            WorkbenchMenu::LocalModels => self.local_models.on_tab_closed(),
            WorkbenchMenu::Llm => self.llm.on_tab_closed(),
            WorkbenchMenu::Channel => self.channel.on_tab_closed(),
            WorkbenchMenu::Voice => self.voice.on_tab_closed(),
            WorkbenchMenu::Cron => self.cron.on_tab_closed(),
            WorkbenchMenu::Heartbeat => self.heartbeat.on_tab_closed(),
            WorkbenchMenu::Gateway => self.gateway.on_tab_closed(),
            WorkbenchMenu::Webhook => self.webhook.on_tab_closed(),
            WorkbenchMenu::Mcp => self.mcp.on_tab_closed(),
            WorkbenchMenu::Acp => self.acp.on_tab_closed(),
            WorkbenchMenu::Skill => self.skills_registry.on_tab_closed(),
            WorkbenchMenu::SkillsManager => self.skills_manager.on_tab_closed(),
            WorkbenchMenu::Memory => self.memory.on_tab_closed(),
            WorkbenchMenu::Archive => self.archive.on_tab_closed(),
            WorkbenchMenu::Tool => self.tool.on_tab_closed(),
            WorkbenchMenu::Monitor => self.monitor.on_tab_closed(),
            WorkbenchMenu::Logs => self.logs.on_tab_closed(),
            WorkbenchMenu::AnalyzeDashboard => self.analyze_dashboard.on_tab_closed(),
            WorkbenchMenu::Observability => self.observability.on_tab_closed(),
        }
    }
}
