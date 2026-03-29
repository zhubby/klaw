use std::env;

use anyhow::Result;
use reqwest::Url;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::Value;

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
    let webhook_path = env_or_default("WEBHOOK_PATH", "/webhook/agents");
    let webhook_token = env_or_default("WEBHOOK_TOKEN", "replace-me");
    let hook_id = env_or_default("HOOK_ID", "order");
    let base_session_key = env_or_default("BASE_SESSION_KEY", "dingtalk:acc:chat-1");
    let provider = optional_env("PROVIDER");
    let model = optional_env("MODEL");
    let chat_id = optional_env("CHAT_ID");
    let sender_id = optional_env("SENDER_ID");
    let body = parse_json_env(
        "BODY_JSON",
        r#"{"order_id":"A123","status":"paid","amount":100}"#,
    )?;

    let mut url = Url::parse(&format!(
        "{}{}",
        base_url.trim_end_matches('/'),
        webhook_path
    ))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("hook_id", &hook_id);
        query.append_pair("base_session_key", &base_session_key);
        if let Some(provider) = &provider {
            query.append_pair("provider", provider);
        }
        if let Some(model) = &model {
            query.append_pair("model", model);
        }
        if let Some(chat_id) = &chat_id {
            query.append_pair("chat_id", chat_id);
        }
        if let Some(sender_id) = &sender_id {
            query.append_pair("sender_id", sender_id);
        }
    }

    let client = reqwest::Client::new();
    let response = client
        .post(url.clone())
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
