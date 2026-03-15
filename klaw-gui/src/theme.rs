pub fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(10);

    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(egui::Color32::from_rgb(232, 240, 248));
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(64, 78, 103);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(34, 43, 58);
    visuals.panel_fill = egui::Color32::from_rgb(22, 28, 38);
    visuals.extreme_bg_color = egui::Color32::from_rgb(17, 22, 30);

    style.visuals = visuals;
    ctx.set_style(style);
}
