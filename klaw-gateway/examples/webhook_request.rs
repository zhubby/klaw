use std::env;

use anyhow::Result;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Value};

fn env_or_default(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

fn optional_env(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.trim().is_empty())
}

fn parse_json_env(key: &str, default: &str) -> Result<Value> {
    match env::var(key) {
        Ok(value) => Ok(serde_json::from_str(&value)?),
        Err(_) => Ok(serde_json::from_str(default)?),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let base_url = env_or_default("BASE_URL", "http://127.0.0.1:18080");
    let webhook_path = env_or_default("WEBHOOK_PATH", "/webhook/events");
    let webhook_token = env_or_default("WEBHOOK_TOKEN", "replace-me");
    let source = env_or_default("SOURCE", "github");
    let event_type = env_or_default("EVENT_TYPE", "issue_comment.created");
    let content = env_or_default("CONTENT", "PR #42 received a new review comment");
    let session_key = optional_env("SESSION_KEY");
    let chat_id = optional_env("CHAT_ID");
    let sender_id = optional_env("SENDER_ID");
    let payload = parse_json_env(
        "PAYLOAD_JSON",
        r#"{"number":42,"action":"created","repository":"openclaw/klaw"}"#,
    )?;
    let metadata = parse_json_env(
        "METADATA_JSON",
        r#"{"repo":"openclaw/klaw","trigger":"manual-example"}"#,
    )?;

    let body = json!({
        "source": source,
        "event_type": event_type,
        "content": content,
        "session_key": session_key,
        "chat_id": chat_id,
        "sender_id": sender_id,
        "payload": payload,
        "metadata": metadata,
    });

    let url = format!("{}{}", base_url.trim_end_matches('/'), webhook_path);
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header(AUTHORIZATION, format!("Bearer {webhook_token}"))
        .header(CONTENT_TYPE, "application/json")
        .json(&body)
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await?;

    println!("POST {url}");
    println!("status: {status}");
    println!("{text}");

    Ok(())
}
