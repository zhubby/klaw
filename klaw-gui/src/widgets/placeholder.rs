pub fn section_card(ui: &mut egui::Ui, title: &str, body: &str) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.vertical(|ui| {
            ui.strong(title);
            ui.add_space(4.0);
            ui.label(body);
        });
    });
}

pub fn key_value_grid(ui: &mut egui::Ui, id_source: &str, rows: &[(&str, &str)]) {
    egui::Grid::new(id_source).num_columns(2).show(ui, |ui| {
        for (key, value) in rows {
            ui.label(*key);
            ui.monospace(*value);
            ui.end_row();
        }
    });
}
