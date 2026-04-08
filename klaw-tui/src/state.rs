use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiMeta {
    pub version: String,
    pub session_key: String,
    pub channel: String,
    pub provider: String,
    pub model: String,
    pub skills: String,
    pub tools: String,
    pub mcp: String,
    pub show_reasoning: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Agent,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppMessage {
    pub role: MessageRole,
    pub content: String,
}

impl AppMessage {
    pub fn new(role: MessageRole, content: String) -> Self {
        Self { role, content }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppStatus {
    Idle,
    Submitting,
}

#[derive(Debug, Clone)]
pub struct AppState {
    meta: TuiMeta,
    messages: Vec<AppMessage>,
    input: String,
    status: AppStatus,
    pending_agent_index: Option<usize>,
}

impl AppState {
    pub fn new(meta: TuiMeta) -> Self {
        Self {
            meta,
            messages: Vec::new(),
            input: String::new(),
            status: AppStatus::Idle,
            pending_agent_index: None,
        }
    }

    pub fn meta(&self) -> &TuiMeta {
        &self.meta
    }

    pub fn messages(&self) -> &[AppMessage] {
        &self.messages
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn status(&self) -> AppStatus {
        self.status
    }

    pub fn set_input(&mut self, input: String) {
        self.input = input;
    }

    pub fn insert_char(&mut self, ch: char) {
        self.input.push(ch);
    }

    pub fn backspace(&mut self) {
        self.input.pop();
    }

    pub fn insert_newline(&mut self) {
        self.input.push('\n');
    }

    pub fn take_submit_input(&mut self) -> Option<String> {
        let trimmed = self.input.trim();
        if trimmed.is_empty() {
            return None;
        }
        let submitted = trimmed.to_string();
        self.messages
            .push(AppMessage::new(MessageRole::User, submitted.clone()));
        self.input.clear();
        Some(submitted)
    }

    pub fn begin_agent_response(&mut self) {
        if self.pending_agent_index.is_none() {
            self.messages
                .push(AppMessage::new(MessageRole::Agent, String::new()));
            self.pending_agent_index = Some(self.messages.len() - 1);
        }
        self.status = AppStatus::Submitting;
    }

    pub fn apply_agent_snapshot(&mut self, content: String) {
        if let Some(index) = self.pending_agent_index {
            self.messages[index] = AppMessage::new(MessageRole::Agent, content);
        } else {
            self.messages
                .push(AppMessage::new(MessageRole::Agent, content.clone()));
            self.pending_agent_index = Some(self.messages.len() - 1);
        }
    }

    pub fn complete_agent_response(&mut self) {
        self.pending_agent_index = None;
        self.status = AppStatus::Idle;
    }

    pub fn clear_pending_agent_response(&mut self) {
        if let Some(index) = self.pending_agent_index.take()
            && self
                .messages
                .get(index)
                .is_some_and(|message| message.content.is_empty())
        {
            self.messages.remove(index);
        }
        self.status = AppStatus::Idle;
    }

    pub fn push_error(&mut self, content: impl Into<String>) {
        self.messages
            .push(AppMessage::new(MessageRole::Error, content.into()));
        self.pending_agent_index = None;
        self.status = AppStatus::Idle;
    }

    pub fn clear_messages(&mut self) {
        self.messages.clear();
        self.pending_agent_index = None;
    }

    pub fn toggle_reasoning(&mut self) {
        self.meta.show_reasoning = !self.meta.show_reasoning;
    }

    pub fn reset_session(&mut self) {
        self.meta.session_key = format!("terminal:{}", Uuid::new_v4());
        self.clear_messages();
        self.status = AppStatus::Idle;
    }

    pub fn chat_id(&self) -> String {
        self.meta
            .session_key
            .split(':')
            .nth(1)
            .unwrap_or("chat")
            .to_string()
    }
}
