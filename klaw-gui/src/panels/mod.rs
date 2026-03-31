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
    analyze_dashboard: analyze_dashboard::AnalyzeDashboardPanel,
    observability: observability::ObservabilityPanel,
}

impl PanelRegistry {
    pub fn tick(&mut self, ctx: &egui::Context) {
        self.logs.tick(ctx);
    }

    pub fn render_for(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        match ctx.menu {
            WorkbenchMenu::Profile => self.profile.render(ui, ctx, notifications),
            WorkbenchMenu::System => self.system.render(ui, ctx, notifications),
            WorkbenchMenu::Setting => self.setting.render(ui, ctx, notifications),
            WorkbenchMenu::Terminal => self.terminal.render(ui, ctx, notifications),
            WorkbenchMenu::Session => self.session.render(ui, ctx, notifications),
            WorkbenchMenu::Approval => self.approval.render(ui, ctx, notifications),
            WorkbenchMenu::Acp => self.acp.render(ui, ctx, notifications),
            WorkbenchMenu::Configuration => self.configuration.render(ui, ctx, notifications),
            WorkbenchMenu::Provider => self.provider.render(ui, ctx, notifications),
            WorkbenchMenu::Llm => self.llm.render(ui, ctx, notifications),
            WorkbenchMenu::Channel => self.channel.render(ui, ctx, notifications),
            WorkbenchMenu::Voice => self.voice.render(ui, ctx, notifications),
            WorkbenchMenu::Cron => self.cron.render(ui, ctx, notifications),
            WorkbenchMenu::Heartbeat => self.heartbeat.render(ui, ctx, notifications),
            WorkbenchMenu::Gateway => self.gateway.render(ui, ctx, notifications),
            WorkbenchMenu::Webhook => self.webhook.render(ui, ctx, notifications),
            WorkbenchMenu::Mcp => self.mcp.render(ui, ctx, notifications),
            WorkbenchMenu::Skill => self.skills_registry.render(ui, ctx, notifications),
            WorkbenchMenu::SkillsManager => self.skills_manager.render(ui, ctx, notifications),
            WorkbenchMenu::Memory => self.memory.render(ui, ctx, notifications),
            WorkbenchMenu::Archive => self.archive.render(ui, ctx, notifications),
            WorkbenchMenu::Tool => self.tool.render(ui, ctx, notifications),
            WorkbenchMenu::Monitor => self.monitor.render(ui, ctx, notifications),
            WorkbenchMenu::Logs => self.logs.render(ui, ctx, notifications),
            WorkbenchMenu::AnalyzeDashboard => {
                self.analyze_dashboard.render(ui, ctx, notifications)
            }
            WorkbenchMenu::Observability => self.observability.render(ui, ctx, notifications),
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
