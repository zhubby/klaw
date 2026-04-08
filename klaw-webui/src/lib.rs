//! Browser chat UI for Klaw gateway (egui + WebSocket).
//!
//! Build for web: `wasm-pack build klaw-webui --target web --out-dir ../klaw-gateway/static/chat/pkg`

#[allow(dead_code)]
mod presentation;

#[cfg(target_arch = "wasm32")]
mod web_chat;

#[cfg(target_arch = "wasm32")]
pub use web_chat::start_chat_ui;

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    #[test]
    fn session_key_has_web_prefix() {
        let key = format!("web:{}", Uuid::new_v4());
        assert!(key.starts_with("web:"));
    }
}
