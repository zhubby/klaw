use egui_phosphor::regular;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkbenchMenu {
    Profile,
    Session,
    Approval,
    Configuration,
    Provider,
    Channel,
    Cron,
    Heartbeat,
    Mcp,
    Skill,
    SkillManage,
    Memory,
    Archive,
    Tool,
    SystemMonitor,
}

impl WorkbenchMenu {
    pub const ALL: [WorkbenchMenu; 15] = [
        WorkbenchMenu::Profile,
        WorkbenchMenu::Session,
        WorkbenchMenu::Approval,
        WorkbenchMenu::Configuration,
        WorkbenchMenu::Provider,
        WorkbenchMenu::Channel,
        WorkbenchMenu::Cron,
        WorkbenchMenu::Heartbeat,
        WorkbenchMenu::Mcp,
        WorkbenchMenu::Skill,
        WorkbenchMenu::SkillManage,
        WorkbenchMenu::Memory,
        WorkbenchMenu::Archive,
        WorkbenchMenu::Tool,
        WorkbenchMenu::SystemMonitor,
    ];

    pub const fn id_key(self) -> &'static str {
        match self {
            WorkbenchMenu::Profile => "profile",
            WorkbenchMenu::Session => "session",
            WorkbenchMenu::Approval => "approval",
            WorkbenchMenu::Configuration => "configuration",
            WorkbenchMenu::Provider => "provider",
            WorkbenchMenu::Channel => "channel",
            WorkbenchMenu::Cron => "cron",
            WorkbenchMenu::Heartbeat => "heartbeat",
            WorkbenchMenu::Mcp => "mcp",
            WorkbenchMenu::Skill => "skill-registry",
            WorkbenchMenu::SkillManage => "skill",
            WorkbenchMenu::Memory => "memory",
            WorkbenchMenu::Archive => "archive",
            WorkbenchMenu::Tool => "tool",
            WorkbenchMenu::SystemMonitor => "system-monitor",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            WorkbenchMenu::Profile => "Profile",
            WorkbenchMenu::Session => "Session",
            WorkbenchMenu::Approval => "Approval",
            WorkbenchMenu::Configuration => "Configuration",
            WorkbenchMenu::Provider => "Model Provider",
            WorkbenchMenu::Channel => "Channel",
            WorkbenchMenu::Cron => "Cron",
            WorkbenchMenu::Heartbeat => "Heartbeat",
            WorkbenchMenu::Mcp => "MCP",
            WorkbenchMenu::Skill => "Skill Registry",
            WorkbenchMenu::SkillManage => "Skill",
            WorkbenchMenu::Memory => "Memory",
            WorkbenchMenu::Archive => "Archive",
            WorkbenchMenu::Tool => "Tool",
            WorkbenchMenu::SystemMonitor => "System Monitor",
        }
    }

    pub const fn icon(self) -> &'static str {
        match self {
            WorkbenchMenu::Profile => regular::USER_CIRCLE,
            WorkbenchMenu::Session => regular::USERS,
            WorkbenchMenu::Approval => regular::SEAL_CHECK,
            WorkbenchMenu::Configuration => regular::TOOLBOX,
            WorkbenchMenu::Provider => regular::BRAIN,
            WorkbenchMenu::Channel => regular::USERS,
            WorkbenchMenu::Cron => regular::CLOCK,
            WorkbenchMenu::Heartbeat => regular::HEARTBEAT,
            WorkbenchMenu::Mcp => regular::PLUG,
            WorkbenchMenu::Skill => regular::PUZZLE_PIECE,
            WorkbenchMenu::SkillManage => regular::PUZZLE_PIECE,
            WorkbenchMenu::Memory => regular::MEMORY,
            WorkbenchMenu::Archive => regular::ARCHIVE,
            WorkbenchMenu::Tool => regular::TOOLBOX,
            WorkbenchMenu::SystemMonitor => regular::CHART_LINE,
        }
    }

    pub const fn default_tab_title(self) -> &'static str {
        self.title()
    }
}

#[cfg(test)]
mod tests {
    use super::WorkbenchMenu;
    use std::collections::HashSet;

    #[test]
    fn menu_id_keys_are_unique_and_non_empty() {
        let mut keys = HashSet::new();
        for menu in WorkbenchMenu::ALL {
            let key = menu.id_key();
            assert!(!key.is_empty());
            assert!(keys.insert(key), "duplicate menu key: {key}");
        }
    }

    #[test]
    fn menu_titles_and_icons_are_present() {
        for menu in WorkbenchMenu::ALL {
            assert!(!menu.title().is_empty());
            assert!(!menu.icon().is_empty());
            assert!(!menu.default_tab_title().is_empty());
        }
    }

    #[test]
    fn configuration_menu_is_registered() {
        assert!(WorkbenchMenu::ALL.contains(&WorkbenchMenu::Configuration));
        assert_eq!(WorkbenchMenu::Configuration.id_key(), "configuration");
    }
}
