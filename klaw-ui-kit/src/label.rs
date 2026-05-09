use egui::{Color32, RichText};

/// Renders a label with an info icon that shows a tooltip hint on hover.
///
/// Useful for form fields where a short label is needed alongside
/// additional context that would clutter the layout if shown inline.
pub fn label_with_hint(ui: &mut egui::Ui, label: &str, hint: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add_space(2.0);
        let response = ui.label(
            RichText::new(egui_phosphor::regular::INFO)
                .size(14.0)
                .color(Color32::GRAY),
        );
        response.on_hover_text(hint);
    });
}
