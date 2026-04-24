use egui::{Color32, FontId, Galley, TextBuffer, TextFormat, Ui, text::LayoutJob};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::sync::Arc;

pub type MarkdownCache = CommonMarkCache;

pub fn text_layouter(ui: &Ui, text: &dyn TextBuffer, wrap_width: f32) -> Arc<Galley> {
    puffin::profile_function!();
    let mut job = highlight_job(text.as_str());
    job.wrap.max_width = wrap_width;
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

pub fn render(ui: &mut Ui, cache: &mut MarkdownCache, markdown: &str) {
    puffin::profile_function!();
    ui.style_mut().url_in_tooltip = true;
    CommonMarkViewer::new().show(ui, cache, markdown);
}

fn highlight_job(markdown: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    for line in markdown.split_inclusive('\n') {
        highlight_line(&mut job, line);
    }
    if markdown.is_empty() {
        append_text(&mut job, "", fmt_default());
    }
    job
}

fn highlight_line(job: &mut LayoutJob, line: &str) {
    let (body, has_newline) = match line.strip_suffix('\n') {
        Some(stripped) => (stripped, true),
        None => (line, false),
    };
    let trimmed = body.trim_start();

    if trimmed.starts_with("```") {
        append_text(job, body, fmt_code());
    } else if trimmed.starts_with('#') {
        append_text(job, body, fmt_heading());
    } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        append_text(job, body, fmt_list());
    } else if trimmed.starts_with('>') {
        append_text(job, body, fmt_quote());
    } else {
        highlight_inline(job, body);
    }

    if has_newline {
        append_text(job, "\n", fmt_default());
    }
}

fn highlight_inline(job: &mut LayoutJob, line: &str) {
    let mut rest = line;
    while let Some(start) = rest.find('`') {
        let (prefix, after_prefix) = rest.split_at(start);
        if !prefix.is_empty() {
            append_text(job, prefix, fmt_default());
        }

        let after_tick = &after_prefix[1..];
        if let Some(end) = after_tick.find('`') {
            let code = &after_prefix[..end + 2];
            append_text(job, code, fmt_code());
            rest = &after_tick[end + 1..];
        } else {
            append_text(job, after_prefix, fmt_default());
            return;
        }
    }

    if !rest.is_empty() {
        append_text(job, rest, fmt_default());
    }
}

fn append_text(job: &mut LayoutJob, text: &str, format: TextFormat) {
    job.append(text, 0.0, format);
}

fn fmt_default() -> TextFormat {
    TextFormat::simple(FontId::monospace(13.0), Color32::LIGHT_GRAY)
}

fn fmt_heading() -> TextFormat {
    TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(132, 197, 255))
}

fn fmt_code() -> TextFormat {
    TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(255, 196, 126))
}

fn fmt_list() -> TextFormat {
    TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(159, 216, 159))
}

fn fmt_quote() -> TextFormat {
    TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(180, 180, 255))
}
