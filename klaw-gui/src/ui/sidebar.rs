use crate::domain::menu::{WorkbenchMenu, WorkbenchMenuGroup};
use crate::state::{UiAction, UiState};
use egui_phosphor::regular;

fn grouped_menus() -> Vec<(WorkbenchMenuGroup, Vec<WorkbenchMenu>)> {
    WorkbenchMenuGroup::ALL
        .into_iter()
        .map(|group| (group, WorkbenchMenu::sorted_for_group(group)))
        .collect()
}

pub fn show_sidebar(ui: &mut egui::Ui, state: &UiState) -> Vec<UiAction> {
    puffin::profile_function!();
    let mut actions = Vec::new();

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
                    egui::RichText::new(group.title())
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
                    let label = format!("{} {}", menu.icon(), menu.title());
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
        let (_, workspace_group) = groups
            .iter()
            .find(|(group, _)| *group == WorkbenchMenuGroup::Workspace)
            .expect("workspace group should exist");
        let workspace_titles = workspace_group
            .iter()
            .map(|menu| menu.title())
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

        let titles = ai_group.iter().map(|menu| menu.title()).collect::<Vec<_>>();
        assert_eq!(
            titles,
            vec![
                "ACP",
                "LLM",
                "Local Models",
                "MCP",
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
}
