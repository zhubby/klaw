pub fn show_json_tree(ui: &mut egui::Ui, value: &serde_json::Value) {
    show_json_value(ui, "root", value, "$");
}

fn show_json_value(ui: &mut egui::Ui, label: &str, value: &serde_json::Value, path: &str) {
    match value {
        serde_json::Value::Object(map) => {
            let header = format!("{label} {{}} ({})", map.len());
            egui::CollapsingHeader::new(header)
                .id_salt(path)
                .default_open(false)
                .show(ui, |ui| {
                    if map.is_empty() {
                        ui.monospace("{}");
                    }
                    for (key, child) in map {
                        show_json_value(ui, key, child, &format!("{path}.{key}"));
                    }
                });
        }
        serde_json::Value::Array(items) => {
            let header = format!("{label} [] ({})", items.len());
            egui::CollapsingHeader::new(header)
                .id_salt(path)
                .default_open(false)
                .show(ui, |ui| {
                    if items.is_empty() {
                        ui.monospace("[]");
                    }
                    for (index, child) in items.iter().enumerate() {
                        show_json_value(
                            ui,
                            &format!("[{index}]"),
                            child,
                            &format!("{path}[{index}]"),
                        );
                    }
                });
        }
        serde_json::Value::String(text) => {
            ui.horizontal_wrapped(|ui| {
                ui.strong(label);
                ui.monospace(format!("\"{text}\""));
            });
        }
        serde_json::Value::Number(number) => {
            ui.horizontal(|ui| {
                ui.strong(label);
                ui.monospace(number.to_string());
            });
        }
        serde_json::Value::Bool(boolean) => {
            ui.horizontal(|ui| {
                ui.strong(label);
                ui.monospace(boolean.to_string());
            });
        }
        serde_json::Value::Null => {
            ui.horizontal(|ui| {
                ui.strong(label);
                ui.monospace("null");
            });
        }
    }
}
