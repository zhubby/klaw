use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEvent {
    Submit,
    Newline,
    Backspace,
    ClearMessages,
    NewSession,
    ToggleReasoning,
    Quit,
    Input(char),
}

pub fn map_key_event(key: KeyEvent) -> Option<AppEvent> {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(AppEvent::Quit),
        KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(AppEvent::ClearMessages)
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(AppEvent::NewSession)
        }
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(AppEvent::ToggleReasoning)
        }
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => Some(AppEvent::Newline),
        KeyCode::Enter => Some(AppEvent::Submit),
        KeyCode::Backspace => Some(AppEvent::Backspace),
        KeyCode::Esc => Some(AppEvent::Quit),
        KeyCode::Char(ch) => Some(AppEvent::Input(ch)),
        _ => None,
    }
}
