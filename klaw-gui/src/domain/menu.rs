#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkbenchMenu {
    Profile,
    Provider,
    Channel,
    Cron,
    Heartbeat,
    Mcp,
    Skill,
    Memory,
    Archive,
    Tool,
    SystemMonitor,
}

impl WorkbenchMenu {
    pub const ALL: [WorkbenchMenu; 11] = [
        WorkbenchMenu::Profile,
        WorkbenchMenu::Provider,
        WorkbenchMenu::Channel,
        WorkbenchMenu::Cron,
        WorkbenchMenu::Heartbeat,
        WorkbenchMenu::Mcp,
        WorkbenchMenu::Skill,
        WorkbenchMenu::Memory,
        WorkbenchMenu::Archive,
        WorkbenchMenu::Tool,
        WorkbenchMenu::SystemMonitor,
    ];

    pub const fn id_key(self) -> &'static str {
        match self {
            WorkbenchMenu::Profile => "profile",
            WorkbenchMenu::Provider => "provider",
            WorkbenchMenu::Channel => "channel",
            WorkbenchMenu::Cron => "cron",
            WorkbenchMenu::Heartbeat => "heartbeat",
            WorkbenchMenu::Mcp => "mcp",
            WorkbenchMenu::Skill => "skill",
            WorkbenchMenu::Memory => "memory",
            WorkbenchMenu::Archive => "archive",
            WorkbenchMenu::Tool => "tool",
            WorkbenchMenu::SystemMonitor => "system-monitor",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            WorkbenchMenu::Profile => "Profile",
            WorkbenchMenu::Provider => "Provider",
            WorkbenchMenu::Channel => "Channel",
            WorkbenchMenu::Cron => "Cron",
            WorkbenchMenu::Heartbeat => "Heartbeat",
            WorkbenchMenu::Mcp => "MCP",
            WorkbenchMenu::Skill => "Skill",
            WorkbenchMenu::Memory => "Memory",
            WorkbenchMenu::Archive => "Archive",
            WorkbenchMenu::Tool => "Tool",
            WorkbenchMenu::SystemMonitor => "System Monitor",
        }
    }

    pub const fn icon(self) -> &'static str {
        match self {
            WorkbenchMenu::Profile => "[PRF]",
            WorkbenchMenu::Provider => "[PRV]",
            WorkbenchMenu::Channel => "[CHN]",
            WorkbenchMenu::Cron => "[CRN]",
            WorkbenchMenu::Heartbeat => "[HBT]",
            WorkbenchMenu::Mcp => "[MCP]",
            WorkbenchMenu::Skill => "[SKL]",
            WorkbenchMenu::Memory => "[MEM]",
            WorkbenchMenu::Archive => "[ARC]",
            WorkbenchMenu::Tool => "[TOL]",
            WorkbenchMenu::SystemMonitor => "[MON]",
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
}
