mod approval;
mod archive;
mod channel;
mod configuration;
mod cron;
mod heartbeat;
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
mod tool;

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
}

#[derive(Default)]
pub struct PanelRegistry {
    profile: profile::ProfilePanel,
    system: system::SystemPanel,
    setting: setting::SettingPanel,
    session: session::SessionPanel,
    approval: approval::ApprovalPanel,
    configuration: configuration::ConfigurationPanel,
    provider: provider::ProviderPanel,
    channel: channel::ChannelPanel,
    cron: cron::CronPanel,
    heartbeat: heartbeat::HeartbeatPanel,
    mcp: mcp::McpPanel,
    skills_registry: skills_registry::SkillsRegistryPanel,
    skills_manager: skills_manager::SkillsManagerPanel,
    memory: memory::MemoryPanel,
    archive: archive::ArchivePanel,
    tool: tool::ToolPanel,
    monitor: monitor::MonitorPanel,
    logs: logs::LogsPanel,
    observability: observability::ObservabilityPanel,
}

impl PanelRegistry {
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
            WorkbenchMenu::Session => self.session.render(ui, ctx, notifications),
            WorkbenchMenu::Approval => self.approval.render(ui, ctx, notifications),
            WorkbenchMenu::Configuration => self.configuration.render(ui, ctx, notifications),
            WorkbenchMenu::Provider => self.provider.render(ui, ctx, notifications),
            WorkbenchMenu::Channel => self.channel.render(ui, ctx, notifications),
            WorkbenchMenu::Cron => self.cron.render(ui, ctx, notifications),
            WorkbenchMenu::Heartbeat => self.heartbeat.render(ui, ctx, notifications),
            WorkbenchMenu::Mcp => self.mcp.render(ui, ctx, notifications),
            WorkbenchMenu::Skill => self.skills_registry.render(ui, ctx, notifications),
            WorkbenchMenu::SkillsManager => self.skills_manager.render(ui, ctx, notifications),
            WorkbenchMenu::Memory => self.memory.render(ui, ctx, notifications),
            WorkbenchMenu::Archive => self.archive.render(ui, ctx, notifications),
            WorkbenchMenu::Tool => self.tool.render(ui, ctx, notifications),
            WorkbenchMenu::Monitor => self.monitor.render(ui, ctx, notifications),
            WorkbenchMenu::Logs => self.logs.render(ui, ctx, notifications),
            WorkbenchMenu::Observability => self.observability.render(ui, ctx, notifications),
        }
    }
}
