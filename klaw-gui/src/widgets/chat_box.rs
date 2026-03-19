use egui_extras::{Size, StripBuilder};
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
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<ChatAction> {
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
        &self,
        ui: &mut egui::Ui,
        message: &ChatMessage,
        pending_action: &mut Option<ChatAction>,
    ) {
        let is_selected = self.selected_message_id.as_deref() == Some(&message.id);

        egui::Frame::group(ui.style())
            .fill(self.message_bg_color(message.role, is_selected))
            .inner_margin(8.0)
            .outer_margin(2.0)
            .corner_radius(4.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(message.role.display_name())
                            .strong()
                            .color(self.role_color(message.role)),
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

                let mut content = message.content.clone();
                ui.add(
                    egui::TextEdit::multiline(&mut content)
                        .desired_width(f32::INFINITY)
                        .interactive(false),
                );

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

    fn message_bg_color(&self, role: ChatRole, is_selected: bool) -> egui::Color32 {
        if is_selected {
            return egui::Color32::from_rgb(60, 60, 80);
        }

        match role {
            ChatRole::User => egui::Color32::from_rgb(45, 55, 72),
            ChatRole::Assistant => egui::Color32::from_rgb(35, 45, 55),
            ChatRole::System => egui::Color32::from_rgb(55, 45, 35),
            ChatRole::Tool => egui::Color32::from_rgb(40, 50, 45),
        }
    }

    fn role_color(&self, role: ChatRole) -> egui::Color32 {
        match role {
            ChatRole::User => egui::Color32::from_rgb(100, 180, 255),
            ChatRole::Assistant => egui::Color32::from_rgb(130, 200, 130),
            ChatRole::System => egui::Color32::from_rgb(255, 200, 100),
            ChatRole::Tool => egui::Color32::from_rgb(200, 150, 255),
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
