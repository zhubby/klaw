use super::{submit_webhook_event, RuntimeBundle};
use async_trait::async_trait;
use klaw_gateway::{
    GatewayOptions, GatewayWebhookHandler, GatewayWebhookRequest, GatewayWebhookResponse,
};
use klaw_session::{
    NewWebhookEventRecord, SessionManager, SqliteSessionManager, UpdateWebhookEventResult,
    WebhookEventStatus,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::warn;

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
    async fn handle(
        &self,
        request: GatewayWebhookRequest,
    ) -> Result<GatewayWebhookResponse, String> {
        let manager = SqliteSessionManager::from_store(self.runtime.session_store.clone());
        manager
            .touch_session(&request.session_key, &request.chat_id, "webhook")
            .await
            .map_err(|err| err.to_string())?;
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
                    .map_err(|err| err.to_string())?,
                metadata_json: Some(
                    serde_json::to_string(&request.metadata).map_err(|err| err.to_string())?,
                ),
                status: WebhookEventStatus::Accepted,
                error_message: None,
                response_summary: None,
                received_at_ms: request.received_at_ms,
                processed_at_ms: None,
                remote_addr: request.remote_addr.clone(),
            })
            .await
            .map_err(|err| err.to_string())?;

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
}

async fn process_webhook_event(runtime: Arc<RuntimeBundle>, request: GatewayWebhookRequest) {
    let manager = SqliteSessionManager::from_store(runtime.session_store.clone());
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
