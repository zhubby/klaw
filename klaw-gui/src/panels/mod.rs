mod approval;
mod archive;
mod channel;
mod configuration;
mod cron;
mod heartbeat;
mod logs;
mod mcp;
mod memory;
mod profile;
mod provider;
mod session;
mod setting;
mod skills_manager;
mod skills_registry;
mod system;
mod system_monitor;
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
    system_monitor: system_monitor::SystemMonitorPanel,
    logs: logs::LogsPanel,
}

impl Default for PanelRegistry {
    fn default() -> Self {
        Self {
            profile: profile::ProfilePanel::default(),
            system: system::SystemPanel::default(),
            setting: setting::SettingPanel::default(),
            session: session::SessionPanel::default(),
            approval: approval::ApprovalPanel::default(),
            configuration: configuration::ConfigurationPanel::default(),
            provider: provider::ProviderPanel::default(),
            channel: channel::ChannelPanel::default(),
            cron: cron::CronPanel::default(),
            heartbeat: heartbeat::HeartbeatPanel::default(),
            mcp: mcp::McpPanel::default(),
            skills_registry: skills_registry::SkillsRegistryPanel::default(),
            skills_manager: skills_manager::SkillsManagerPanel::default(),
            memory: memory::MemoryPanel::default(),
            archive: archive::ArchivePanel::default(),
            tool: tool::ToolPanel::default(),
            system_monitor: system_monitor::SystemMonitorPanel::default(),
            logs: logs::LogsPanel::default(),
        }
    }
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
            WorkbenchMenu::SystemMonitor => self.system_monitor.render(ui, ctx, notifications),
            WorkbenchMenu::Logs => self.logs.render(ui, ctx, notifications),
        }
    }
}
