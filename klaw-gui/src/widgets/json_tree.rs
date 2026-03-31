use egui_json_tree::JsonTree;

pub fn show_json_tree(ui: &mut egui::Ui, value: &serde_json::Value) {
    show_json_tree_with_id(ui, value, "$");
}

pub fn show_json_tree_with_id(ui: &mut egui::Ui, value: &serde_json::Value, root_id: &str) {
    JsonTree::new(root_id, value).show(ui);
}
