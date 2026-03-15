mod archive;
mod channel;
mod cron;
mod heartbeat;
mod mcp;
mod memory;
mod profile;
mod provider;
mod skill;
mod system_monitor;
mod tool;

use crate::domain::menu::WorkbenchMenu;

pub struct RenderCtx<'a> {
    pub menu: WorkbenchMenu,
    pub tab_title: &'a str,
}

pub trait PanelRenderer {
    fn render(&mut self, ui: &mut egui::Ui, ctx: &RenderCtx<'_>);
}

pub struct PanelRegistry {
    profile: profile::ProfilePanel,
    provider: provider::ProviderPanel,
    channel: channel::ChannelPanel,
    cron: cron::CronPanel,
    heartbeat: heartbeat::HeartbeatPanel,
    mcp: mcp::McpPanel,
    skill: skill::SkillPanel,
    memory: memory::MemoryPanel,
    archive: archive::ArchivePanel,
    tool: tool::ToolPanel,
    system_monitor: system_monitor::SystemMonitorPanel,
}

impl Default for PanelRegistry {
    fn default() -> Self {
        Self {
            profile: profile::ProfilePanel,
            provider: provider::ProviderPanel,
            channel: channel::ChannelPanel,
            cron: cron::CronPanel,
            heartbeat: heartbeat::HeartbeatPanel,
            mcp: mcp::McpPanel,
            skill: skill::SkillPanel,
            memory: memory::MemoryPanel,
            archive: archive::ArchivePanel,
            tool: tool::ToolPanel,
            system_monitor: system_monitor::SystemMonitorPanel,
        }
    }
}

impl PanelRegistry {
    pub fn render_for(&mut self, ui: &mut egui::Ui, ctx: &RenderCtx<'_>) {
        match ctx.menu {
            WorkbenchMenu::Profile => self.profile.render(ui, ctx),
            WorkbenchMenu::Provider => self.provider.render(ui, ctx),
            WorkbenchMenu::Channel => self.channel.render(ui, ctx),
            WorkbenchMenu::Cron => self.cron.render(ui, ctx),
            WorkbenchMenu::Heartbeat => self.heartbeat.render(ui, ctx),
            WorkbenchMenu::Mcp => self.mcp.render(ui, ctx),
            WorkbenchMenu::Skill => self.skill.render(ui, ctx),
            WorkbenchMenu::Memory => self.memory.render(ui, ctx),
            WorkbenchMenu::Archive => self.archive.render(ui, ctx),
            WorkbenchMenu::Tool => self.tool.render(ui, ctx),
            WorkbenchMenu::SystemMonitor => self.system_monitor.render(ui, ctx),
        }
    }
}
