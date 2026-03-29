use super::{RuntimeBundle, submit_webhook_agent, submit_webhook_event};
use async_trait::async_trait;
use klaw_config::ConfigStore;
use klaw_gateway::{
    GatewayOptions, GatewayWebhookAgentRequest, GatewayWebhookAgentResponse, GatewayWebhookHandler,
    GatewayWebhookHandlerError, GatewayWebhookRequest, GatewayWebhookResponse,
};
use klaw_session::{
    NewWebhookAgentRecord, NewWebhookEventRecord, SessionManager, SqliteSessionManager,
    UpdateWebhookAgentResult, UpdateWebhookEventResult, WebhookEventStatus,
};
use klaw_util::default_data_dir;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::fs;
use tracing::{debug, warn};

pub fn gateway_options(runtime: Arc<RuntimeBundle>) -> GatewayOptions {
    GatewayOptions {
        webhook_handler: Some(Arc::new(RuntimeWebhookHandler { runtime })),
        ..GatewayOptions::default()
    }
}

struct RuntimeWebhookHandler {
    runtime: Arc<RuntimeBundle>,
}

#[async_trait]
impl GatewayWebhookHandler for RuntimeWebhookHandler {
    async fn handle_event(
        &self,
        request: GatewayWebhookRequest,
    ) -> Result<GatewayWebhookResponse, GatewayWebhookHandlerError> {
        debug!(
            webhook_kind = "events",
            event_id = request.event_id.as_str(),
            source = request.source.as_str(),
            event_type = request.event_type.as_str(),
            session_key = request.session_key.as_str(),
            remote_addr = request.remote_addr.as_deref().unwrap_or("unknown"),
            "accepting webhook event request"
        );
        let manager = SqliteSessionManager::from_store(self.runtime.session_store.clone());
        manager
            .touch_session(&request.session_key, &request.chat_id, "webhook")
            .await
            .map_err(|err| GatewayWebhookHandlerError::internal(err.to_string()))?;
        manager
            .append_webhook_event(&NewWebhookEventRecord {
                id: request.event_id.clone(),
                source: request.source.clone(),
                event_type: request.event_type.clone(),
                session_key: request.session_key.clone(),
                chat_id: request.chat_id.clone(),
                sender_id: request.sender_id.clone(),
                content: request.content.clone(),
                payload_json: request
                    .payload
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|err| GatewayWebhookHandlerError::internal(err.to_string()))?,
                metadata_json: Some(
                    serde_json::to_string(&request.metadata)
                        .map_err(|err| GatewayWebhookHandlerError::internal(err.to_string()))?,
                ),
                status: WebhookEventStatus::Accepted,
                error_message: None,
                response_summary: None,
                received_at_ms: request.received_at_ms,
                processed_at_ms: None,
                remote_addr: request.remote_addr.clone(),
            })
            .await
            .map_err(|err| GatewayWebhookHandlerError::internal(err.to_string()))?;
        debug!(
            webhook_kind = "events",
            event_id = request.event_id.as_str(),
            session_key = request.session_key.as_str(),
            "persisted webhook event request"
        );

        let event_id = request.event_id.clone();
        let session_key = request.session_key.clone();
        let runtime = Arc::clone(&self.runtime);
        tokio::spawn(async move {
            process_webhook_event(runtime, request).await;
        });

        Ok(GatewayWebhookResponse {
            event_id,
            status: WebhookEventStatus::Accepted.as_str().to_string(),
            session_key,
        })
    }

    async fn handle_agent(
        &self,
        request: GatewayWebhookAgentRequest,
    ) -> Result<GatewayWebhookAgentResponse, GatewayWebhookHandlerError> {
        debug!(
            webhook_kind = "agents",
            request_id = request.request_id.as_str(),
            hook_id = request.hook_id.as_str(),
            session_key = request.session_key.as_str(),
            remote_addr = request.remote_addr.as_deref().unwrap_or("unknown"),
            "accepting webhook agent request"
        );
        let content = load_webhook_agent_prompt(&request)
            .await
            .map_err(|err| GatewayWebhookHandlerError::not_found(err))?;
        let manager = SqliteSessionManager::from_store(self.runtime.session_store.clone());
        manager
            .touch_session(&request.session_key, &request.chat_id, "webhook")
            .await
            .map_err(|err| GatewayWebhookHandlerError::internal(err.to_string()))?;
        manager
            .append_webhook_agent(&NewWebhookAgentRecord {
                id: request.request_id.clone(),
                hook_id: request.hook_id.clone(),
                session_key: request.session_key.clone(),
                chat_id: request.chat_id.clone(),
                sender_id: request.sender_id.clone(),
                content: content.clone(),
                payload_json: Some(
                    serde_json::to_string(&request.body)
                        .map_err(|err| GatewayWebhookHandlerError::internal(err.to_string()))?,
                ),
                metadata_json: Some(
                    serde_json::to_string(&request.metadata)
                        .map_err(|err| GatewayWebhookHandlerError::internal(err.to_string()))?,
                ),
                status: WebhookEventStatus::Accepted,
                error_message: None,
                response_summary: None,
                received_at_ms: request.received_at_ms,
                processed_at_ms: None,
                remote_addr: request.remote_addr.clone(),
            })
            .await
            .map_err(|err| GatewayWebhookHandlerError::internal(err.to_string()))?;
        debug!(
            webhook_kind = "agents",
            request_id = request.request_id.as_str(),
            hook_id = request.hook_id.as_str(),
            session_key = request.session_key.as_str(),
            "persisted webhook agent request"
        );

        let request_id = request.request_id.clone();
        let hook_id = request.hook_id.clone();
        let session_key = request.session_key.clone();
        let runtime = Arc::clone(&self.runtime);
        tokio::spawn(async move {
            process_webhook_agent(runtime, request, content).await;
        });

        Ok(GatewayWebhookAgentResponse {
            request_id,
            status: WebhookEventStatus::Accepted.as_str().to_string(),
            hook_id,
            session_key,
        })
    }
}

async fn process_webhook_event(runtime: Arc<RuntimeBundle>, request: GatewayWebhookRequest) {
    let manager = SqliteSessionManager::from_store(runtime.session_store.clone());
    debug!(
        webhook_kind = "events",
        event_id = request.event_id.as_str(),
        session_key = request.session_key.as_str(),
        "starting webhook event processing"
    );
    let result = submit_webhook_event(runtime.as_ref(), &request).await;
    let update = match result {
        Ok(output) => UpdateWebhookEventResult {
            status: WebhookEventStatus::Processed,
            error_message: None,
            response_summary: output
                .as_ref()
                .map(|output| summarize_response(&output.content)),
            processed_at_ms: Some(now_ms()),
        },
        Err(err) => UpdateWebhookEventResult {
            status: WebhookEventStatus::Failed,
            error_message: Some(err.to_string()),
            response_summary: None,
            processed_at_ms: Some(now_ms()),
        },
    };
    match &update.status {
        WebhookEventStatus::Processed => debug!(
            webhook_kind = "events",
            event_id = request.event_id.as_str(),
            session_key = request.session_key.as_str(),
            "webhook event processed"
        ),
        WebhookEventStatus::Failed => debug!(
            webhook_kind = "events",
            event_id = request.event_id.as_str(),
            session_key = request.session_key.as_str(),
            error = update.error_message.as_deref().unwrap_or("unknown"),
            "webhook event processing failed"
        ),
        WebhookEventStatus::Accepted => {}
    }

    if let Err(err) = manager
        .update_webhook_event_status(&request.event_id, &update)
        .await
    {
        warn!(
            error = %err,
            webhook_event_id = request.event_id.as_str(),
            "failed to persist webhook event status"
        );
    }
}

async fn process_webhook_agent(
    runtime: Arc<RuntimeBundle>,
    request: GatewayWebhookAgentRequest,
    content: String,
) {
    let manager = SqliteSessionManager::from_store(runtime.session_store.clone());
    debug!(
        webhook_kind = "agents",
        request_id = request.request_id.as_str(),
        hook_id = request.hook_id.as_str(),
        session_key = request.session_key.as_str(),
        "starting webhook agent processing"
    );
    let result = submit_webhook_agent(runtime.as_ref(), &request, content).await;
    let update = match result {
        Ok(output) => UpdateWebhookAgentResult {
            status: WebhookEventStatus::Processed,
            error_message: None,
            response_summary: output
                .as_ref()
                .map(|output| summarize_response(&output.content)),
            processed_at_ms: Some(now_ms()),
        },
        Err(err) => UpdateWebhookAgentResult {
            status: WebhookEventStatus::Failed,
            error_message: Some(err.to_string()),
            response_summary: None,
            processed_at_ms: Some(now_ms()),
        },
    };
    match &update.status {
        WebhookEventStatus::Processed => debug!(
            webhook_kind = "agents",
            request_id = request.request_id.as_str(),
            hook_id = request.hook_id.as_str(),
            session_key = request.session_key.as_str(),
            "webhook agent processed"
        ),
        WebhookEventStatus::Failed => debug!(
            webhook_kind = "agents",
            request_id = request.request_id.as_str(),
            hook_id = request.hook_id.as_str(),
            session_key = request.session_key.as_str(),
            error = update.error_message.as_deref().unwrap_or("unknown"),
            "webhook agent processing failed"
        ),
        WebhookEventStatus::Accepted => {}
    }

    if let Err(err) = manager
        .update_webhook_agent_status(&request.request_id, &update)
        .await
    {
        warn!(
            error = %err,
            webhook_event_id = request.request_id.as_str(),
            "failed to persist webhook agent status"
        );
    }
}

async fn load_webhook_agent_prompt(request: &GatewayWebhookAgentRequest) -> Result<String, String> {
    let prompt_path = webhook_agent_prompt_path(&request.hook_id)?;
    let template = fs::read_to_string(&prompt_path).await.map_err(|_| {
        format!(
            "hook prompt `{}` not found at {}",
            request.hook_id,
            prompt_path.display()
        )
    })?;
    build_webhook_agent_content(
        &template,
        &request.hook_id,
        &request.session_key,
        &request.body,
    )
}

fn webhook_agent_prompt_path(hook_id: &str) -> Result<PathBuf, String> {
    let root = ConfigStore::open(None)
        .ok()
        .and_then(|store| store.reload().ok())
        .and_then(|snapshot| snapshot.config.storage.root_dir.map(PathBuf::from))
        .or_else(default_data_dir)
        .ok_or_else(|| "HOME is unavailable".to_string())?;
    Ok(root
        .join("hooks")
        .join("prompts")
        .join(format!("{hook_id}.md")))
}

fn build_webhook_agent_content(
    template: &str,
    hook_id: &str,
    session_key: &str,
    body: &Value,
) -> Result<String, String> {
    let body_json = serde_json::to_string_pretty(body)
        .map_err(|err| format!("failed to serialize request body: {err}"))?;
    let template = template.trim_end();
    let mut content = String::new();
    if !template.is_empty() {
        content.push_str(template);
        content.push_str("\n\n");
    }
    content.push_str("## Hook Context\n\n");
    content.push_str(&format!(
        "- Hook ID: `{}`\n- Original Session Key: `{}`\n",
        hook_id, session_key
    ));
    content.push_str("\n## Request JSON\n\n```json\n");
    content.push_str(&body_json);
    content.push_str("\n```");
    Ok(content)
}

fn summarize_response(content: &str) -> String {
    const MAX_LEN: usize = 160;
    let trimmed = content.trim();
    if trimmed.chars().count() <= MAX_LEN {
        return trimmed.to_string();
    }
    let summary: String = trimmed.chars().take(MAX_LEN).collect();
    format!("{summary}...")
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[allow(dead_code)]
fn _metadata_json(metadata: &BTreeMap<String, Value>) -> Result<String, serde_json::Error> {
    serde_json::to_string(metadata)
}

#[cfg(test)]
mod tests {
    use super::build_webhook_agent_content;
    use serde_json::json;

    #[test]
    fn build_webhook_agent_content_appends_pretty_json_block() {
        let content = build_webhook_agent_content(
            "# Order Hook\n\nFollow the request.",
            "order",
            "dingtalk:acc:chat-1",
            &json!({"order_id":"A123","status":"paid"}),
        )
        .expect("content should build");

        assert!(content.contains("# Order Hook"));
        assert!(content.contains("## Hook Context"));
        assert!(content.contains("`order`"));
        assert!(content.contains("`dingtalk:acc:chat-1`"));
        assert!(content.contains("## Request JSON"));
        assert!(content.contains("```json"));
        assert!(content.contains("\"order_id\": \"A123\""));
    }
}
