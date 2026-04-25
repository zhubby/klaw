use egui_phosphor::regular;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkbenchMenuGroup {
    Workspace,
    AiAndCapability,
    RuntimeAndAccess,
    AutomationAndOperations,
    DataAndHistory,
    Observability,
}

impl WorkbenchMenuGroup {
    pub const ALL: [WorkbenchMenuGroup; 6] = [
        WorkbenchMenuGroup::Workspace,
        WorkbenchMenuGroup::AiAndCapability,
        WorkbenchMenuGroup::RuntimeAndAccess,
        WorkbenchMenuGroup::AutomationAndOperations,
        WorkbenchMenuGroup::DataAndHistory,
        WorkbenchMenuGroup::Observability,
    ];

    pub const fn title(self) -> &'static str {
        match self {
            WorkbenchMenuGroup::Workspace => "WORKSPACE",
            WorkbenchMenuGroup::AiAndCapability => "AI & CAPABILITY",
            WorkbenchMenuGroup::RuntimeAndAccess => "RUNTIME & ACCESS",
            WorkbenchMenuGroup::AutomationAndOperations => "AUTOMATION & OPERATIONS",
            WorkbenchMenuGroup::DataAndHistory => "DATA & HISTORY",
            WorkbenchMenuGroup::Observability => "OBSERVABILITY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkbenchMenu {
    Profile,
    System,
    Setting,
    Terminal,
    Session,
    Approval,
    Configuration,
    Provider,
    LocalModels,
    Llm,
    Channel,
    Voice,
    Cron,
    Heartbeat,
    Gateway,
    Webhook,
    Mcp,
    Acp,
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
    pub const ALL: [WorkbenchMenu; 27] = [
        WorkbenchMenu::Profile,
        WorkbenchMenu::System,
        WorkbenchMenu::Setting,
        WorkbenchMenu::Terminal,
        WorkbenchMenu::Session,
        WorkbenchMenu::Approval,
        WorkbenchMenu::Configuration,
        WorkbenchMenu::Provider,
        WorkbenchMenu::LocalModels,
        WorkbenchMenu::Llm,
        WorkbenchMenu::Channel,
        WorkbenchMenu::Voice,
        WorkbenchMenu::Cron,
        WorkbenchMenu::Heartbeat,
        WorkbenchMenu::Gateway,
        WorkbenchMenu::Webhook,
        WorkbenchMenu::Mcp,
        WorkbenchMenu::Acp,
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
            WorkbenchMenu::Terminal => "terminal",
            WorkbenchMenu::Session => "session",
            WorkbenchMenu::Approval => "approval",
            WorkbenchMenu::Configuration => "configuration",
            WorkbenchMenu::Provider => "provider",
            WorkbenchMenu::LocalModels => "local-models",
            WorkbenchMenu::Llm => "llm",
            WorkbenchMenu::Channel => "channel",
            WorkbenchMenu::Voice => "voice",
            WorkbenchMenu::Cron => "cron",
            WorkbenchMenu::Heartbeat => "heartbeat",
            WorkbenchMenu::Gateway => "gateway",
            WorkbenchMenu::Webhook => "webhook",
            WorkbenchMenu::Mcp => "mcp",
            WorkbenchMenu::Acp => "acp",
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
            WorkbenchMenu::Setting => "Settings",
            WorkbenchMenu::Terminal => "Terminal",
            WorkbenchMenu::Session => "Session",
            WorkbenchMenu::Approval => "Approval",
            WorkbenchMenu::Configuration => "Configuration",
            WorkbenchMenu::Provider => "Model Provider",
            WorkbenchMenu::LocalModels => "Local Models",
            WorkbenchMenu::Llm => "LLM",
            WorkbenchMenu::Channel => "Channel",
            WorkbenchMenu::Voice => "Voice",
            WorkbenchMenu::Cron => "Cron",
            WorkbenchMenu::Heartbeat => "Heartbeat",
            WorkbenchMenu::Gateway => "Gateway",
            WorkbenchMenu::Webhook => "Webhook",
            WorkbenchMenu::Mcp => "MCP",
            WorkbenchMenu::Acp => "ACP",
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
            WorkbenchMenu::Terminal => regular::TERMINAL,
            WorkbenchMenu::Session => regular::USERS,
            WorkbenchMenu::Approval => regular::SEAL_CHECK,
            WorkbenchMenu::Configuration => regular::TOOLBOX,
            WorkbenchMenu::Provider => regular::BRAIN,
            WorkbenchMenu::LocalModels => regular::PACKAGE,
            WorkbenchMenu::Llm => regular::CHATS_CIRCLE,
            WorkbenchMenu::Channel => regular::USERS,
            WorkbenchMenu::Voice => regular::MICROPHONE,
            WorkbenchMenu::Cron => regular::CLOCK,
            WorkbenchMenu::Heartbeat => regular::HEARTBEAT,
            WorkbenchMenu::Gateway => regular::PLUG,
            WorkbenchMenu::Webhook => regular::PLUG,
            WorkbenchMenu::Mcp => regular::PLUG,
            WorkbenchMenu::Acp => regular::PLUG,
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

    pub const fn group(self) -> WorkbenchMenuGroup {
        match self {
            WorkbenchMenu::Profile
            | WorkbenchMenu::System
            | WorkbenchMenu::Setting
            | WorkbenchMenu::Terminal
            | WorkbenchMenu::Configuration => WorkbenchMenuGroup::Workspace,
            WorkbenchMenu::Provider
            | WorkbenchMenu::LocalModels
            | WorkbenchMenu::Llm
            | WorkbenchMenu::Mcp
            | WorkbenchMenu::Acp
            | WorkbenchMenu::Skill
            | WorkbenchMenu::SkillsManager
            | WorkbenchMenu::Tool
            | WorkbenchMenu::Voice => WorkbenchMenuGroup::AiAndCapability,
            WorkbenchMenu::Channel | WorkbenchMenu::Gateway | WorkbenchMenu::Webhook => {
                WorkbenchMenuGroup::RuntimeAndAccess
            }
            WorkbenchMenu::Approval
            | WorkbenchMenu::Cron
            | WorkbenchMenu::Heartbeat
            | WorkbenchMenu::Session => WorkbenchMenuGroup::AutomationAndOperations,
            WorkbenchMenu::Memory | WorkbenchMenu::Archive => WorkbenchMenuGroup::DataAndHistory,
            WorkbenchMenu::Monitor
            | WorkbenchMenu::Logs
            | WorkbenchMenu::AnalyzeDashboard
            | WorkbenchMenu::Observability => WorkbenchMenuGroup::Observability,
        }
    }

    pub fn sorted_for_group(group: WorkbenchMenuGroup) -> Vec<WorkbenchMenu> {
        let mut menus = Self::ALL
            .into_iter()
            .filter(|menu| menu.group() == group)
            .collect::<Vec<_>>();
        menus.sort_by_key(|menu| menu.title());
        menus
    }
}

#[cfg(test)]
mod tests {
    use super::{WorkbenchMenu, WorkbenchMenuGroup};
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

    #[test]
    fn terminal_menu_is_registered() {
        assert!(WorkbenchMenu::ALL.contains(&WorkbenchMenu::Terminal));
        assert_eq!(WorkbenchMenu::Terminal.id_key(), "terminal");
        assert_eq!(WorkbenchMenu::Terminal.title(), "Terminal");
        assert_eq!(
            WorkbenchMenu::Terminal.group(),
            WorkbenchMenuGroup::Workspace
        );
    }

    #[test]
    fn local_models_menu_is_registered() {
        assert!(WorkbenchMenu::ALL.contains(&WorkbenchMenu::LocalModels));
        assert_eq!(WorkbenchMenu::LocalModels.id_key(), "local-models");
        assert_eq!(WorkbenchMenu::LocalModels.title(), "Local Models");
        assert_eq!(
            WorkbenchMenu::LocalModels.group(),
            WorkbenchMenuGroup::AiAndCapability
        );
    }

    #[test]
    fn every_menu_has_a_group_and_groups_cover_all_menus_once() {
        let mut seen = HashSet::new();

        for group in WorkbenchMenuGroup::ALL {
            for menu in WorkbenchMenu::sorted_for_group(group) {
                assert_eq!(menu.group(), group);
                assert!(
                    seen.insert(menu),
                    "menu assigned more than once: {:?}",
                    menu
                );
            }
        }

        assert_eq!(seen.len(), WorkbenchMenu::ALL.len());
    }

    #[test]
    fn settings_title_is_plural_while_id_key_stays_stable() {
        assert_eq!(WorkbenchMenu::Setting.title(), "Settings");
        assert_eq!(WorkbenchMenu::Setting.id_key(), "setting");
    }

    #[test]
    fn menus_within_group_are_sorted_by_title() {
        for group in WorkbenchMenuGroup::ALL {
            let menus = WorkbenchMenu::sorted_for_group(group);
            let mut titles = menus.iter().map(|menu| menu.title()).collect::<Vec<_>>();
            let mut sorted_titles = titles.clone();
            sorted_titles.sort_unstable();
            assert_eq!(titles, sorted_titles);
            titles.dedup();
            assert_eq!(titles.len(), menus.len());
        }
    }
}
