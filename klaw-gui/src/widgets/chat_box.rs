use egui_extras::{Size, StripBuilder};
use klaw_ui_kit::text_animator::{AnimationType, TextAnimator};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
    System,
    Tool,
}

impl ChatRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
            Self::Tool => "tool",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::User => "You",
            Self::Assistant => "Assistant",
            Self::System => "System",
            Self::Tool => "Tool",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "user" => Self::User,
            "assistant" => Self::Assistant,
            "system" => Self::System,
            "tool" => Self::Tool,
            _ => Self::User,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub id: String,
    pub role: ChatRole,
    pub content: String,
    pub timestamp_ms: i64,
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
    pub is_streaming: bool,
}

impl ChatMessage {
    pub fn new(role: ChatRole, content: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role,
            content: content.into(),
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
            tool_name: None,
            tool_call_id: None,
            is_streaming: false,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(ChatRole::User, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(ChatRole::Assistant, content)
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self::new(ChatRole::System, content)
    }

    pub fn tool(
        content: impl Into<String>,
        tool_name: impl Into<String>,
        tool_call_id: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: ChatRole::Tool,
            content: content.into(),
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
            tool_name: Some(tool_name.into()),
            tool_call_id: Some(tool_call_id.into()),
            is_streaming: false,
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    pub fn with_timestamp(mut self, ts_ms: i64) -> Self {
        self.timestamp_ms = ts_ms;
        self
    }

    pub fn with_tool_name(mut self, name: impl Into<String>) -> Self {
        self.tool_name = Some(name.into());
        self
    }

    pub fn with_tool_call_id(mut self, id: impl Into<String>) -> Self {
        self.tool_call_id = Some(id.into());
        self
    }

    pub fn set_streaming(mut self, streaming: bool) -> Self {
        self.is_streaming = streaming;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatAction {
    Send,
    Retry(String),
}

pub struct ChatBox {
    pub messages: Vec<ChatMessage>,
    pub input_text: String,
    pub title: String,
    pub open: bool,
    scroll_to_bottom: bool,
    selected_message_id: Option<String>,
    fade_in_messages: HashMap<String, TextAnimator>,
}

impl Default for ChatBox {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            input_text: String::new(),
            title: "Chat".to_string(),
            open: false,
            scroll_to_bottom: false,
            selected_message_id: None,
            fade_in_messages: HashMap::new(),
        }
    }
}

impl ChatBox {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Default::default()
        }
    }

    pub fn with_messages(mut self, messages: Vec<ChatMessage>) -> Self {
        self.messages = messages;
        self.scroll_to_bottom = true;
        self
    }

    pub fn add_message(&mut self, message: ChatMessage) {
        self.track_fade_in_message(&message);
        self.messages.push(message);
        self.scroll_to_bottom = true;
    }

    pub fn add_user_message(&mut self, content: impl Into<String>) {
        self.add_message(ChatMessage::user(content));
    }

    pub fn add_assistant_message(&mut self, content: impl Into<String>) {
        self.add_message(ChatMessage::assistant(content));
    }

    pub fn clear_messages(&mut self) {
        self.messages.clear();
        self.fade_in_messages.clear();
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<ChatAction> {
        puffin::profile_function!();
        if !self.open {
            return None;
        }

        let mut pending_action: Option<ChatAction> = None;
        let mut open = self.open;

        egui::Window::new(&self.title)
            .id(egui::Id::new(&self.title))
            .default_size([500.0, 600.0])
            .min_size([300.0, 400.0])
            .max_size([800.0, 700.0])
            .collapsible(true)
            .resizable(true)
            .open(&mut open)
            .show(ctx, |ui| {
                let action = self.render_content(ui);
                if action.is_some() {
                    pending_action = action;
                }
            });

        self.open = open;
        pending_action
    }

    fn render_content(&mut self, ui: &mut egui::Ui) -> Option<ChatAction> {
        let mut pending_action: Option<ChatAction> = None;

        StripBuilder::new(ui)
            .size(Size::remainder().at_least(100.0))
            .size(Size::exact(INPUT_AREA_HEIGHT))
            .vertical(|mut strip| {
                strip.cell(|ui| {
                    self.render_messages(ui, &mut pending_action);
                });

                strip.cell(|ui| {
                    self.render_input(ui, &mut pending_action);
                });
            });

        pending_action
    }

    fn render_messages(&mut self, ui: &mut egui::Ui, pending_action: &mut Option<ChatAction>) {
        puffin::profile_scope!("chat_box_messages");
        self.prune_finished_animations();
        let scroll_id = egui::Id::new((&self.title, "chat_messages"));

        egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.messages.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.label(egui::RichText::new("No messages yet").weak().italics());
                    });
                    return;
                }

                for message in self.messages.clone() {
                    self.render_message(ui, &message, pending_action);
                    ui.add_space(4.0);
                }
            });
    }

    fn render_message(
        &mut self,
        ui: &mut egui::Ui,
        message: &ChatMessage,
        pending_action: &mut Option<ChatAction>,
    ) {
        let is_selected = self.selected_message_id.as_deref() == Some(&message.id);
        let dark_mode = ui.visuals().dark_mode;

        egui::Frame::group(ui.style())
            .fill(self.message_bg_color(message.role, is_selected, dark_mode))
            .inner_margin(8.0)
            .outer_margin(2.0)
            .corner_radius(4.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(message.role.display_name())
                            .strong()
                            .color(self.role_color(message.role, dark_mode)),
                    );

                    if let Some(tool_name) = &message.tool_name {
                        ui.label(
                            egui::RichText::new(format!("[{}]", tool_name))
                                .small()
                                .weak(),
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("📋").on_hover_text("Copy").clicked() {
                            ui.ctx().output_mut(|o| {
                                o.commands
                                    .push(egui::OutputCommand::CopyText(message.content.clone()));
                            });
                        }

                        if matches!(message.role, ChatRole::User) {
                            if ui.small_button("🔄").on_hover_text("Retry").clicked() {
                                *pending_action = Some(ChatAction::Retry(message.id.clone()));
                            }
                        }
                    });
                });

                ui.add_space(4.0);

                self.render_message_content(ui, message);

                if message.is_streaming {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(egui::RichText::new("thinking...").weak().italics());
                    });
                }
            });
    }

    fn render_input(&mut self, ui: &mut egui::Ui, pending_action: &mut Option<ChatAction>) {
        ui.separator();

        let input_height = INPUT_AREA_HEIGHT - 10.0;

        ui.with_layout(
            egui::Layout::left_to_right(egui::Align::Center).with_cross_align(egui::Align::Center),
            |ui| {
                let input_id = egui::Id::new((&self.title, "chat_input"));

                let response = ui.add_sized(
                    [ui.available_width() - 60.0, input_height],
                    egui::TextEdit::multiline(&mut self.input_text)
                        .id(input_id)
                        .hint_text("Type a message..."),
                );

                let send_enabled = !self.input_text.trim().is_empty();

                ui.add_enabled_ui(send_enabled, |ui| {
                    let button = ui.add_sized([50.0, input_height], egui::Button::new("Send"));
                    if button.clicked()
                        || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        if !self.input_text.trim().is_empty() {
                            *pending_action = Some(ChatAction::Send);
                        }
                    }
                });
            },
        );
    }

    fn message_bg_color(
        &self,
        role: ChatRole,
        is_selected: bool,
        dark_mode: bool,
    ) -> egui::Color32 {
        if is_selected {
            return if dark_mode {
                egui::Color32::from_rgb(60, 60, 80)
            } else {
                egui::Color32::from_rgb(225, 232, 245)
            };
        }

        if dark_mode {
            match role {
                ChatRole::User => egui::Color32::from_rgb(45, 55, 72),
                ChatRole::Assistant => egui::Color32::from_rgb(35, 45, 55),
                ChatRole::System => egui::Color32::from_rgb(55, 45, 35),
                ChatRole::Tool => egui::Color32::from_rgb(40, 50, 45),
            }
        } else {
            match role {
                ChatRole::User => egui::Color32::from_rgb(255, 239, 246),
                ChatRole::Assistant => egui::Color32::from_rgb(236, 245, 255),
                ChatRole::System => egui::Color32::from_rgb(255, 247, 232),
                ChatRole::Tool => egui::Color32::from_rgb(239, 248, 241),
            }
        }
    }

    fn role_color(&self, role: ChatRole, dark_mode: bool) -> egui::Color32 {
        if dark_mode {
            match role {
                ChatRole::User => egui::Color32::from_rgb(255, 176, 209),
                ChatRole::Assistant => egui::Color32::from_rgb(145, 198, 255),
                ChatRole::System => egui::Color32::from_rgb(255, 205, 120),
                ChatRole::Tool => egui::Color32::from_rgb(154, 212, 167),
            }
        } else {
            match role {
                ChatRole::User => egui::Color32::from_rgb(210, 104, 146),
                ChatRole::Assistant => egui::Color32::from_rgb(82, 142, 222),
                ChatRole::System => egui::Color32::from_rgb(186, 130, 42),
                ChatRole::Tool => egui::Color32::from_rgb(86, 153, 103),
            }
        }
    }

    pub fn get_input_text(&self) -> &str {
        &self.input_text
    }

    pub fn take_input_text(&mut self) -> String {
        std::mem::take(&mut self.input_text)
    }

    pub fn set_input_text(&mut self, text: impl Into<String>) {
        self.input_text = text.into();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    fn track_fade_in_message(&mut self, message: &ChatMessage) {
        if !Self::should_fade_in_message(message) {
            return;
        }

        self.fade_in_messages
            .entry(message.id.clone())
            .or_insert_with(|| {
                TextAnimator::new(
                    &message.content,
                    egui::TextStyle::Body.resolve(&egui::Style::default()),
                    egui::Color32::WHITE,
                    2.5,
                    AnimationType::FadeIn,
                )
            });
    }

    fn should_fade_in_message(message: &ChatMessage) -> bool {
        matches!(message.role, ChatRole::Assistant)
            && !message.is_streaming
            && !message.content.is_empty()
    }

    fn render_message_content(&mut self, ui: &mut egui::Ui, message: &ChatMessage) {
        let mut should_remove_animation = false;
        if let Some(animator) = self.fade_in_messages.get_mut(&message.id) {
            animator.font = egui::TextStyle::Body.resolve(ui.style());
            animator.color = ui.visuals().text_color();
            animator.process_animation(ui.ctx());
            animator.render(ui);
            if !animator.is_animation_finished() {
                ui.ctx().request_repaint();
            } else {
                should_remove_animation = true;
            }
        } else {
            ui.add(
                egui::Label::new(message.content.as_str())
                    .wrap()
                    .selectable(true),
            );
        }

        if should_remove_animation {
            self.fade_in_messages.remove(&message.id);
        }
    }

    fn prune_finished_animations(&mut self) {
        self.fade_in_messages
            .retain(|_, animator| !animator.is_animation_finished());
    }

    pub fn from_chat_records(records: &[klaw_storage::ChatRecord]) -> Self {
        let messages: Vec<ChatMessage> = records
            .iter()
            .map(|r| {
                ChatMessage::new(ChatRole::from_str(&r.role), &r.content).with_timestamp(r.ts_ms)
            })
            .collect();

        Self::new("Chat").with_messages(messages)
    }
}

const INPUT_AREA_HEIGHT: f32 = 60.0;

#[cfg(test)]
impl ChatBox {
    fn animated_message_ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.fade_in_messages.keys().map(String::as_str).collect();
        ids.sort_unstable();
        ids
    }

    fn prune_finished_animations_for_test(&mut self) {
        self.prune_finished_animations();
    }
}

#[cfg(test)]
mod tests {
    use super::{ChatBox, ChatMessage};

    #[test]
    fn tracks_only_new_non_stream_assistant_messages_for_animation() {
        let existing = ChatMessage::assistant("history");
        let mut chat_box = ChatBox::new("Chat").with_messages(vec![existing]);

        assert!(chat_box.animated_message_ids().is_empty());

        chat_box.add_message(ChatMessage::user("hello"));
        assert!(chat_box.animated_message_ids().is_empty());

        chat_box.add_message(ChatMessage::assistant("partial").set_streaming(true));
        assert!(chat_box.animated_message_ids().is_empty());

        let final_assistant = ChatMessage::assistant("final");
        let final_id = final_assistant.id.clone();
        chat_box.add_message(final_assistant);

        assert_eq!(chat_box.animated_message_ids(), vec![final_id.as_str()]);
    }

    #[test]
    fn historical_messages_do_not_start_fade_in_animation() {
        let history = vec![
            ChatMessage::assistant("first"),
            ChatMessage::assistant("second"),
        ];

        let chat_box = ChatBox::new("Chat").with_messages(history);

        assert!(chat_box.animated_message_ids().is_empty());
    }

    #[test]
    fn removes_finished_fade_in_animation_state() {
        let mut chat_box = ChatBox::new("Chat");
        let message = ChatMessage::assistant("done");
        let id = message.id.clone();
        chat_box.add_message(message);

        let animator = chat_box.fade_in_messages.get_mut(&id).unwrap();
        animator.timer = 1.0;
        animator.animation_finished = true;

        chat_box.prune_finished_animations_for_test();

        assert!(!chat_box.fade_in_messages.contains_key(&id));
    }
}
