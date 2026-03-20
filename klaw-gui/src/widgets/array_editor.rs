#[derive(Debug, Clone, Default)]
pub struct ArrayEditor {
    entries: Vec<String>,
    new_value: String,
    label: String,
}

impl ArrayEditor {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            entries: Vec::new(),
            new_value: String::new(),
            label: label.into(),
        }
    }

    pub fn from_vec(label: impl Into<String>, items: &[String]) -> Self {
        Self {
            entries: items.to_vec(),
            new_value: String::new(),
            label: label.into(),
        }
    }

    pub fn to_vec(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.label(&self.label);

        let mut to_remove: Option<usize> = None;
        for (idx, item) in self.entries.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.add_enabled(
                    false,
                    egui::TextEdit::singleline(&mut item.clone()).desired_width(320.0),
                );
                if ui.small_button("×").clicked() {
                    to_remove = Some(idx);
                }
            });
        }

        if let Some(idx) = to_remove {
            self.entries.remove(idx);
        }

        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.new_value)
                    .hint_text("value")
                    .desired_width(320.0),
            );
            let can_add = !self.new_value.trim().is_empty();
            if ui.add_enabled(can_add, egui::Button::new("+")).clicked() {
                self.entries.push(self.new_value.trim().to_string());
                self.new_value.clear();
            }
            let enter_pressed =
                response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && can_add;
            if enter_pressed {
                self.entries.push(self.new_value.trim().to_string());
                self.new_value.clear();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_vec_creates_entries() {
        let items = vec!["arg1".to_string(), "arg2".to_string()];
        let editor = ArrayEditor::from_vec("Test", &items);

        assert_eq!(editor.entries.len(), 2);
        assert_eq!(editor.to_vec(), items);
    }

    #[test]
    fn to_vec_filters_empty_items() {
        let mut editor = ArrayEditor::new("Test");
        editor.entries.push("value1".to_string());
        editor.entries.push("".to_string());
        editor.entries.push("  ".to_string());

        let vec = editor.to_vec();

        assert_eq!(vec.len(), 1);
        assert_eq!(vec[0], "value1");
    }

    #[test]
    fn to_vec_trims_whitespace() {
        let mut editor = ArrayEditor::new("Test");
        editor.entries.push("  value  ".to_string());

        let vec = editor.to_vec();

        assert_eq!(vec[0], "value");
    }
}
