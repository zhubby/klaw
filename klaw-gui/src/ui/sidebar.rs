use crate::domain::menu::{WorkbenchMenu, WorkbenchMenuGroup};
use crate::settings::current_ui_language;
use crate::state::{UiAction, UiState};
use egui_phosphor::regular;
use klaw_ui_kit::{LocaleDomain, Translator};

fn grouped_menus() -> Vec<(WorkbenchMenuGroup, Vec<WorkbenchMenu>)> {
    WorkbenchMenuGroup::ALL
        .into_iter()
        .map(|group| (group, WorkbenchMenu::sorted_for_group(group)))
        .collect()
}

pub fn show_sidebar(ui: &mut egui::Ui, state: &UiState) -> Vec<UiAction> {
    puffin::profile_function!();
    let mut actions = Vec::new();

    let translator = Translator::new(LocaleDomain::Gui, current_ui_language());

    ui.label(
        egui::RichText::new(format!("{} Klaw", regular::ROBOT))
            .strong()
            .size(20.0),
    );
    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            puffin::profile_scope!("sidebar_grouped_menus");
            let groups = grouped_menus();
            for (index, (group, menus)) in groups.iter().enumerate() {
                if index > 0 {
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);
                }

                ui.label(
                    egui::RichText::new(translator.text(group.i18n_key()))
                        .small()
                        .strong()
                        .color(ui.visuals().weak_text_color()),
                );
                ui.add_space(4.0);

                for menu in menus {
                    let is_active = state
                        .workbench
                        .active_tab
                        .is_some_and(|id| id.menu == *menu);
                    let label = format!("{} {}", menu.icon(), translator.text(menu.i18n_key()));
                    if ui.selectable_label(is_active, label).clicked() {
                        actions.push(UiAction::OpenMenu(*menu));
                    }
                }
            }
        });

    actions
}

#[cfg(test)]
mod tests {
    use super::grouped_menus;
    use crate::domain::menu::{WorkbenchMenu, WorkbenchMenuGroup};
    use klaw_ui_kit::{LocaleDomain, Translator, UiLanguage};

    #[test]
    fn grouped_menus_follow_expected_group_order() {
        let groups = grouped_menus();
        let order = groups
            .into_iter()
            .map(|(group, _)| group)
            .collect::<Vec<_>>();
        assert_eq!(order, WorkbenchMenuGroup::ALL);
    }

    #[test]
    fn grouped_menus_are_sorted_and_keep_skills_adjacent() {
        let groups = grouped_menus();
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);

        let (_, workspace_group) = groups
            .iter()
            .find(|(group, _)| *group == WorkbenchMenuGroup::Workspace)
            .expect("workspace group should exist");
        let workspace_titles = workspace_group
            .iter()
            .map(|menu| translator.text(menu.i18n_key()))
            .collect::<Vec<_>>();
        assert_eq!(
            workspace_titles,
            vec![
                "Configuration",
                "Profile Prompt",
                "Settings",
                "System",
                "Terminal",
            ]
        );

        let (_, ai_group) = groups
            .into_iter()
            .find(|(group, _)| *group == WorkbenchMenuGroup::AiAndCapability)
            .expect("AI & Capability group should exist");

        let titles = ai_group
            .iter()
            .map(|menu| translator.text(menu.i18n_key()))
            .collect::<Vec<_>>();
        assert_eq!(
            titles,
            vec![
                "ACP",
                "LLM",
                "MCP",
                "Model",
                "Model Provider",
                "Skills Manager",
                "Skills Registry",
                "Tool",
                "Voice",
            ]
        );

        let manager_index = ai_group
            .iter()
            .position(|menu| *menu == WorkbenchMenu::SkillsManager)
            .expect("skills manager should exist");
        let registry_index = ai_group
            .iter()
            .position(|menu| *menu == WorkbenchMenu::Skill)
            .expect("skills registry should exist");
        assert_eq!(registry_index, manager_index + 1);
    }

    #[test]
    fn sidebar_menu_i18n_keys_match_ftl_keys() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        for menu in WorkbenchMenu::ALL {
            let key = menu.i18n_key();
            let translated = translator.text(key);
            // English fallback must resolve to a display string (not the raw key)
            assert_ne!(translated, key, "i18n_key {key} has no English FTL entry");
            assert!(!translated.is_empty());
        }
    }

    #[test]
    fn sidebar_group_i18n_keys_match_ftl_keys() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        for group in WorkbenchMenuGroup::ALL {
            let key = group.i18n_key();
            let translated = translator.text(key);
            assert_ne!(translated, key, "i18n_key {key} has no English FTL entry");
            assert!(!translated.is_empty());
        }
    }

    #[test]
    fn sidebar_menu_i18n_keys_cover_chinese_translations() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        for menu in WorkbenchMenu::ALL {
            let key = menu.i18n_key();
            let translated = translator.text(key);
            assert!(!translated.is_empty());
        }
    }
}
