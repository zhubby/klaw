use egui::{vec2, Context};
use egui_notify::{Anchor, Toasts};

pub struct NotificationCenter {
    toasts: Toasts,
}

impl Default for NotificationCenter {
    fn default() -> Self {
        let toasts = Toasts::new()
            .with_anchor(Anchor::BottomRight)
            .with_margin(vec2(16.0, 16.0));
        Self { toasts }
    }
}

impl NotificationCenter {
    pub fn success(&mut self, message: impl Into<String>) {
        self.toasts.success(message.into());
    }

    pub fn info(&mut self, message: impl Into<String>) {
        self.toasts.info(message.into());
    }

    pub fn warning(&mut self, message: impl Into<String>) {
        self.toasts.warning(message.into());
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.toasts.error(message.into());
    }

    pub fn show(&mut self, ctx: &Context) {
        self.toasts.show(ctx);
    }
}
