mod approval;
mod archive;
mod channel;
mod configuration;
mod cron;
mod heartbeat;
mod mcp;
mod memory;
mod profile;
mod provider;
mod session;
mod skill;
mod skill_manage;
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
    session: session::SessionPanel,
    approval: approval::ApprovalPanel,
    configuration: configuration::ConfigurationPanel,
    provider: provider::ProviderPanel,
    channel: channel::ChannelPanel,
    cron: cron::CronPanel,
    heartbeat: heartbeat::HeartbeatPanel,
    mcp: mcp::McpPanel,
    skill: skill::SkillPanel,
    skill_manage: skill_manage::SkillManagePanel,
    memory: memory::MemoryPanel,
    archive: archive::ArchivePanel,
    tool: tool::ToolPanel,
    system_monitor: system_monitor::SystemMonitorPanel,
}

impl Default for PanelRegistry {
    fn default() -> Self {
        Self {
            profile: profile::ProfilePanel,
            session: session::SessionPanel,
            approval: approval::ApprovalPanel,
            configuration: configuration::ConfigurationPanel::default(),
            provider: provider::ProviderPanel::default(),
            channel: channel::ChannelPanel::default(),
            cron: cron::CronPanel::default(),
            heartbeat: heartbeat::HeartbeatPanel::default(),
            mcp: mcp::McpPanel::default(),
            skill: skill::SkillPanel::default(),
            skill_manage: skill_manage::SkillManagePanel,
            memory: memory::MemoryPanel::default(),
            archive: archive::ArchivePanel::default(),
            tool: tool::ToolPanel::default(),
            system_monitor: system_monitor::SystemMonitorPanel::default(),
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
            WorkbenchMenu::Session => self.session.render(ui, ctx, notifications),
            WorkbenchMenu::Approval => self.approval.render(ui, ctx, notifications),
            WorkbenchMenu::Configuration => self.configuration.render(ui, ctx, notifications),
            WorkbenchMenu::Provider => self.provider.render(ui, ctx, notifications),
            WorkbenchMenu::Channel => self.channel.render(ui, ctx, notifications),
            WorkbenchMenu::Cron => self.cron.render(ui, ctx, notifications),
            WorkbenchMenu::Heartbeat => self.heartbeat.render(ui, ctx, notifications),
            WorkbenchMenu::Mcp => self.mcp.render(ui, ctx, notifications),
            WorkbenchMenu::Skill => self.skill.render(ui, ctx, notifications),
            WorkbenchMenu::SkillManage => self.skill_manage.render(ui, ctx, notifications),
            WorkbenchMenu::Memory => self.memory.render(ui, ctx, notifications),
            WorkbenchMenu::Archive => self.archive.render(ui, ctx, notifications),
            WorkbenchMenu::Tool => self.tool.render(ui, ctx, notifications),
            WorkbenchMenu::SystemMonitor => self.system_monitor.render(ui, ctx, notifications),
        }
    }
}
