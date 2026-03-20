use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct KeyValueInput {
    entries: Vec<(String, String)>,
    new_key: String,
    new_value: String,
    label: String,
}

impl KeyValueInput {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            entries: Vec::new(),
            new_key: String::new(),
            new_value: String::new(),
            label: label.into(),
        }
    }

    pub fn from_map(label: impl Into<String>, map: &BTreeMap<String, String>) -> Self {
        Self {
            entries: map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            new_key: String::new(),
            new_value: String::new(),
            label: label.into(),
        }
    }

    pub fn to_map(&self) -> BTreeMap<String, String> {
        self.entries
            .iter()
            .filter(|(k, _)| !k.trim().is_empty())
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect()
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.label(&self.label);

        let mut to_remove: Option<usize> = None;
        for (idx, (key, value)) in self.entries.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("{idx}:"));
                ui.add_enabled(false, egui::TextEdit::singleline(&mut key.clone()));
                ui.label("=");
                ui.add_enabled(false, egui::TextEdit::singleline(&mut value.clone()));
                if ui.small_button("×").clicked() {
                    to_remove = Some(idx);
                }
            });
        }

        if let Some(idx) = to_remove {
            self.entries.remove(idx);
        }

        ui.horizontal(|ui| {
            let key_response = ui.add(
                egui::TextEdit::singleline(&mut self.new_key)
                    .hint_text("key")
                    .desired_width(120.0),
            );
            ui.label("=");
            ui.add(
                egui::TextEdit::singleline(&mut self.new_value)
                    .hint_text("value")
                    .desired_width(200.0),
            );
            let can_add = !self.new_key.trim().is_empty();
            if ui.add_enabled(can_add, egui::Button::new("+")).clicked() {
                self.entries.push((
                    self.new_key.trim().to_string(),
                    self.new_value.trim().to_string(),
                ));
                self.new_key.clear();
                self.new_value.clear();
            }
            let enter_pressed = key_response.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && can_add;
            if enter_pressed {
                self.entries.push((
                    self.new_key.trim().to_string(),
                    self.new_value.trim().to_string(),
                ));
                self.new_key.clear();
                self.new_value.clear();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_map_creates_entries() {
        let mut map = BTreeMap::new();
        map.insert("KEY1".to_string(), "value1".to_string());
        map.insert("KEY2".to_string(), "value2".to_string());

        let input = KeyValueInput::from_map("Test", &map);

        assert_eq!(input.entries.len(), 2);
        assert_eq!(input.to_map(), map);
    }

    #[test]
    fn to_map_filters_empty_keys() {
        let mut input = KeyValueInput::new("Test");
        input
            .entries
            .push(("key1".to_string(), "value1".to_string()));
        input.entries.push(("".to_string(), "value2".to_string()));
        input.entries.push(("  ".to_string(), "value3".to_string()));

        let map = input.to_map();

        assert_eq!(map.len(), 1);
        assert_eq!(map.get("key1"), Some(&"value1".to_string()));
    }

    #[test]
    fn to_map_trims_whitespace() {
        let mut input = KeyValueInput::new("Test");
        input
            .entries
            .push(("  key  ".to_string(), "  value  ".to_string()));

        let map = input.to_map();

        assert_eq!(map.get("key"), Some(&"value".to_string()));
    }
}
