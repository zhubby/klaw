use std::hash::Hash;

use egui::{Color32, RichText, ScrollArea, Ui};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

pub(super) type MarkdownCache = CommonMarkCache;

pub(super) fn render_markdown(
    ui: &mut Ui,
    cache: &mut MarkdownCache,
    markdown: &str,
    text_color: Color32,
    link_color: Color32,
) {
    ui.style_mut().url_in_tooltip = true;
    ui.scope(|ui| {
        ui.visuals_mut().override_text_color = Some(text_color);
        ui.visuals_mut().hyperlink_color = link_color;
        CommonMarkViewer::new().show(ui, cache, markdown);
    });
}

pub(super) fn render_scrollable_markdown(
    ui: &mut Ui,
    cache: &mut MarkdownCache,
    markdown: &str,
    text_color: Color32,
    link_color: Color32,
    id_salt: impl Hash,
) {
    ui.style_mut().url_in_tooltip = true;
    ui.scope(|ui| {
        ui.visuals_mut().override_text_color = Some(text_color);
        ui.visuals_mut().hyperlink_color = link_color;

        let max_width = ui.available_width();
        ScrollArea::horizontal()
            .id_salt(id_salt)
            .auto_shrink([false, true])
            .max_width(max_width)
            .show(ui, |ui| {
                ui.set_min_width(max_width);
                CommonMarkViewer::new().show(ui, cache, markdown);
            });
    });
}

pub(super) fn render_plain_message(ui: &mut Ui, text: &str, text_color: Color32) {
    ui.label(RichText::new(text).color(text_color));
}
