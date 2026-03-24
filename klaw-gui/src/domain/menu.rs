use egui_phosphor::regular;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkbenchMenu {
    Profile,
    System,
    Setting,
    Session,
    Approval,
    Configuration,
    Provider,
    Llm,
    Channel,
    Voice,
    Cron,
    Heartbeat,
    Gateway,
    Webhook,
    Mcp,
    Skill,
    #[serde(alias = "SkillManage")]
    SkillsManager,
    Memory,
    Archive,
    Tool,
    Monitor,
    Logs,
    AnalyzeDashboard,
    Observability,
}

impl WorkbenchMenu {
    pub const ALL: [WorkbenchMenu; 24] = [
        WorkbenchMenu::Profile,
        WorkbenchMenu::System,
        WorkbenchMenu::Setting,
        WorkbenchMenu::Session,
        WorkbenchMenu::Approval,
        WorkbenchMenu::Configuration,
        WorkbenchMenu::Provider,
        WorkbenchMenu::Llm,
        WorkbenchMenu::Channel,
        WorkbenchMenu::Voice,
        WorkbenchMenu::Cron,
        WorkbenchMenu::Heartbeat,
        WorkbenchMenu::Gateway,
        WorkbenchMenu::Webhook,
        WorkbenchMenu::Mcp,
        WorkbenchMenu::Skill,
        WorkbenchMenu::SkillsManager,
        WorkbenchMenu::Memory,
        WorkbenchMenu::Archive,
        WorkbenchMenu::Tool,
        WorkbenchMenu::Monitor,
        WorkbenchMenu::Logs,
        WorkbenchMenu::AnalyzeDashboard,
        WorkbenchMenu::Observability,
    ];

    pub const fn id_key(self) -> &'static str {
        match self {
            WorkbenchMenu::Profile => "profile",
            WorkbenchMenu::System => "system",
            WorkbenchMenu::Setting => "setting",
            WorkbenchMenu::Session => "session",
            WorkbenchMenu::Approval => "approval",
            WorkbenchMenu::Configuration => "configuration",
            WorkbenchMenu::Provider => "provider",
            WorkbenchMenu::Llm => "llm",
            WorkbenchMenu::Channel => "channel",
            WorkbenchMenu::Voice => "voice",
            WorkbenchMenu::Cron => "cron",
            WorkbenchMenu::Heartbeat => "heartbeat",
            WorkbenchMenu::Gateway => "gateway",
            WorkbenchMenu::Webhook => "webhook",
            WorkbenchMenu::Mcp => "mcp",
            WorkbenchMenu::Skill => "skill-registry",
            WorkbenchMenu::SkillsManager => "skills-manager",
            WorkbenchMenu::Memory => "memory",
            WorkbenchMenu::Archive => "archive",
            WorkbenchMenu::Tool => "tool",
            WorkbenchMenu::Monitor => "monitor",
            WorkbenchMenu::Logs => "logs",
            WorkbenchMenu::AnalyzeDashboard => "analyze-dashboard",
            WorkbenchMenu::Observability => "observability",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            WorkbenchMenu::Profile => "Profile Prompt",
            WorkbenchMenu::System => "System",
            WorkbenchMenu::Setting => "Setting",
            WorkbenchMenu::Session => "Session",
            WorkbenchMenu::Approval => "Approval",
            WorkbenchMenu::Configuration => "Configuration",
            WorkbenchMenu::Provider => "Model Provider",
            WorkbenchMenu::Llm => "LLM",
            WorkbenchMenu::Channel => "Channel",
            WorkbenchMenu::Voice => "Voice",
            WorkbenchMenu::Cron => "Cron",
            WorkbenchMenu::Heartbeat => "Heartbeat",
            WorkbenchMenu::Gateway => "Gateway",
            WorkbenchMenu::Webhook => "Webhook",
            WorkbenchMenu::Mcp => "MCP",
            WorkbenchMenu::Skill => "Skills Registry",
            WorkbenchMenu::SkillsManager => "Skills Manager",
            WorkbenchMenu::Memory => "Memory",
            WorkbenchMenu::Archive => "Archive",
            WorkbenchMenu::Tool => "Tool",
            WorkbenchMenu::Monitor => "Monitor",
            WorkbenchMenu::Logs => "Logs",
            WorkbenchMenu::AnalyzeDashboard => "Analyze Dashboard",
            WorkbenchMenu::Observability => "Observability",
        }
    }

    pub const fn icon(self) -> &'static str {
        match self {
            WorkbenchMenu::Profile => regular::USER_CIRCLE,
            WorkbenchMenu::System => regular::DATABASE,
            WorkbenchMenu::Setting => regular::GEAR,
            WorkbenchMenu::Session => regular::USERS,
            WorkbenchMenu::Approval => regular::SEAL_CHECK,
            WorkbenchMenu::Configuration => regular::TOOLBOX,
            WorkbenchMenu::Provider => regular::BRAIN,
            WorkbenchMenu::Llm => regular::CHATS_CIRCLE,
            WorkbenchMenu::Channel => regular::USERS,
            WorkbenchMenu::Voice => regular::MICROPHONE,
            WorkbenchMenu::Cron => regular::CLOCK,
            WorkbenchMenu::Heartbeat => regular::HEARTBEAT,
            WorkbenchMenu::Gateway => regular::PLUG,
            WorkbenchMenu::Webhook => regular::PLUG,
            WorkbenchMenu::Mcp => regular::PLUG,
            WorkbenchMenu::Skill => regular::PUZZLE_PIECE,
            WorkbenchMenu::SkillsManager => regular::PUZZLE_PIECE,
            WorkbenchMenu::Memory => regular::MEMORY,
            WorkbenchMenu::Archive => regular::ARCHIVE,
            WorkbenchMenu::Tool => regular::TOOLBOX,
            WorkbenchMenu::Monitor => regular::CHART_LINE,
            WorkbenchMenu::Logs => regular::INFO,
            WorkbenchMenu::AnalyzeDashboard => regular::CHART_LINE,
            WorkbenchMenu::Observability => regular::ACTIVITY,
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

    #[test]
    fn gateway_menu_is_registered() {
        assert!(WorkbenchMenu::ALL.contains(&WorkbenchMenu::Gateway));
        assert_eq!(WorkbenchMenu::Gateway.id_key(), "gateway");
    }

    #[test]
    fn webhook_menu_is_registered() {
        assert!(WorkbenchMenu::ALL.contains(&WorkbenchMenu::Webhook));
        assert_eq!(WorkbenchMenu::Webhook.id_key(), "webhook");
    }

    #[test]
    fn voice_menu_is_registered() {
        assert!(WorkbenchMenu::ALL.contains(&WorkbenchMenu::Voice));
        assert_eq!(WorkbenchMenu::Voice.id_key(), "voice");
    }
}
