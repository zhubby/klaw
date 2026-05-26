use super::{
    RuntimeBundle, gateway_websocket, service_loop::ChannelAvailability, submit_webhook_agent,
    submit_webhook_event,
};
use async_trait::async_trait;
use klaw_config::{AppConfig, ConfigError, ConfigStore, McpServerConfig};
use klaw_gateway::{
    GatewayMcpHandler, GatewayMcpHandlerError, GatewayMcpRuntimeSnapshot,
    GatewayMcpServerConfigView, GatewayMcpServerDetailView, GatewayMcpServerStatusView,
    GatewayMcpServerUpsertRequest, GatewayOptions, GatewayWebhookAgentRequest,
    GatewayWebhookAgentResponse, GatewayWebhookHandler, GatewayWebhookHandlerError,
    GatewayWebhookRequest, GatewayWebhookResponse,
};
use klaw_mcp::{McpConfigSnapshot, McpRuntimeSnapshot, McpServerKey};
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

pub fn gateway_options(runtime: Arc<RuntimeBundle>, config: &AppConfig) -> GatewayOptions {
    GatewayOptions {
        websocket_broadcaster: Some(Arc::clone(&runtime.websocket_broadcaster)),
        webhook_handler: Some(Arc::new(RuntimeWebhookHandler {
            runtime: Arc::clone(&runtime),
            channel_availability: ChannelAvailability::from_app_config(config),
        })),
        websocket_handler: Some(gateway_websocket::build_gateway_websocket_handler(
            Arc::clone(&runtime),
            config,
        )),
        archive_service: runtime.archive_service.clone(),
        app_config: Some(Arc::new(config.clone())),
        mcp_handler: Some(Arc::new(RuntimeMcpHandler {
            runtime: Arc::clone(&runtime),
        })),
        ..GatewayOptions::default()
    }
}

struct RuntimeMcpHandler {
    runtime: Arc<RuntimeBundle>,
}

#[async_trait]
impl GatewayMcpHandler for RuntimeMcpHandler {
    async fn status(&self) -> Result<GatewayMcpRuntimeSnapshot, GatewayMcpHandlerError> {
        let store = ConfigStore::open(None).map_err(gateway_mcp_config_error)?;
        let snapshot = McpConfigSnapshot::from_mcp_config(&store.snapshot().config.mcp);
        let manager = self.mcp_manager().await;
        let guard = manager
            .try_lock()
            .map_err(|_| GatewayMcpHandlerError::unavailable("mcp manager is busy"))?;
        Ok(convert_mcp_runtime_snapshot(
            guard.runtime_snapshot(&snapshot),
        ))
    }

    async fn list_servers(
        &self,
    ) -> Result<(Vec<GatewayMcpServerConfigView>, GatewayMcpRuntimeSnapshot), GatewayMcpHandlerError>
    {
        let store = ConfigStore::open(None).map_err(gateway_mcp_config_error)?;
        let config = store.snapshot().config;
        let servers = config.mcp.servers.iter().map(redacted_mcp_server).collect();
        let runtime = self.status().await?;
        Ok((servers, runtime))
    }

    async fn get_server(
        &self,
        id: String,
    ) -> Result<
        (
            GatewayMcpServerConfigView,
            Option<GatewayMcpServerStatusView>,
            Option<GatewayMcpServerDetailView>,
        ),
        GatewayMcpHandlerError,
    > {
        let id = normalize_mcp_server_id(&id)?;
        let store = ConfigStore::open(None).map_err(gateway_mcp_config_error)?;
        let config = store.snapshot().config;
        let Some(server) = config.mcp.servers.iter().find(|server| server.id == id) else {
            return Err(GatewayMcpHandlerError::not_found(format!(
                "mcp server '{id}' not found"
            )));
        };
        let runtime = self.status().await?;
        let status = runtime.statuses.into_iter().find(|status| status.id == id);
        let detail = runtime.details.into_iter().find(|detail| detail.id == id);
        Ok((redacted_mcp_server(server), status, detail))
    }

    async fn create_server(
        &self,
        request: GatewayMcpServerUpsertRequest,
    ) -> Result<(GatewayMcpServerConfigView, GatewayMcpRuntimeSnapshot), GatewayMcpHandlerError>
    {
        let server = build_mcp_server_config(request, None)?;
        let created_id = server.id.clone();
        let store = ConfigStore::open(None).map_err(gateway_mcp_config_error)?;
        let saved = store
            .update_config(|config| {
                if config.mcp.servers.iter().any(|item| item.id == created_id) {
                    return Err(ConfigError::InvalidConfig(format!(
                        "mcp server '{created_id}' already exists"
                    )));
                }
                config.mcp.servers.push(server);
                Ok(())
            })
            .map_err(gateway_mcp_config_error)?
            .0;
        let runtime = self.sync_snapshot(&saved.config).await?;
        let created = saved
            .config
            .mcp
            .servers
            .iter()
            .find(|server| server.id == created_id)
            .map(redacted_mcp_server)
            .ok_or_else(|| GatewayMcpHandlerError::internal("created mcp server missing"))?;
        Ok((created, runtime))
    }

    async fn update_server(
        &self,
        id: String,
        request: GatewayMcpServerUpsertRequest,
    ) -> Result<(GatewayMcpServerConfigView, GatewayMcpRuntimeSnapshot), GatewayMcpHandlerError>
    {
        let id = normalize_mcp_server_id(&id)?;
        let store = ConfigStore::open(None).map_err(gateway_mcp_config_error)?;
        let (saved, updated_id) = store
            .update_config(|config| {
                let Some(position) = config.mcp.servers.iter().position(|item| item.id == id)
                else {
                    return Err(ConfigError::InvalidConfig(format!(
                        "mcp server '{id}' not found"
                    )));
                };
                let replacement =
                    build_mcp_server_config(request, Some(config.mcp.servers[position].clone()))
                        .map_err(|err| ConfigError::InvalidConfig(err.message))?;
                if replacement.id != id
                    && config
                        .mcp
                        .servers
                        .iter()
                        .any(|item| item.id == replacement.id)
                {
                    return Err(ConfigError::InvalidConfig(format!(
                        "mcp server '{}' already exists",
                        replacement.id
                    )));
                }
                let updated_id = replacement.id.clone();
                config.mcp.servers[position] = replacement;
                Ok(updated_id)
            })
            .map_err(gateway_mcp_config_error)?;
        let runtime = self.sync_snapshot(&saved.config).await?;
        let updated = saved
            .config
            .mcp
            .servers
            .iter()
            .find(|server| server.id == updated_id)
            .map(redacted_mcp_server)
            .ok_or_else(|| GatewayMcpHandlerError::internal("updated mcp server missing"))?;
        Ok((updated, runtime))
    }

    async fn delete_server(
        &self,
        id: String,
    ) -> Result<GatewayMcpRuntimeSnapshot, GatewayMcpHandlerError> {
        let id = normalize_mcp_server_id(&id)?;
        let store = ConfigStore::open(None).map_err(gateway_mcp_config_error)?;
        let saved = store
            .update_config(|config| {
                let original_len = config.mcp.servers.len();
                config.mcp.servers.retain(|server| server.id != id);
                if config.mcp.servers.len() == original_len {
                    return Err(ConfigError::InvalidConfig(format!(
                        "mcp server '{id}' not found"
                    )));
                }
                Ok(())
            })
            .map_err(gateway_mcp_config_error)?
            .0;
        self.sync_snapshot(&saved.config).await
    }

    async fn sync(&self) -> Result<GatewayMcpRuntimeSnapshot, GatewayMcpHandlerError> {
        let store = ConfigStore::open(None).map_err(gateway_mcp_config_error)?;
        self.sync_snapshot(&store.snapshot().config).await
    }

    async fn restart_server(
        &self,
        id: String,
    ) -> Result<GatewayMcpRuntimeSnapshot, GatewayMcpHandlerError> {
        let id = normalize_mcp_server_id(&id)?;
        let store = ConfigStore::open(None).map_err(gateway_mcp_config_error)?;
        let snapshot = McpConfigSnapshot::from_mcp_config(&store.snapshot().config.mcp);
        let manager = self.mcp_manager().await;
        let mut guard = manager.lock().await;
        guard
            .restart_server(&McpServerKey::new(&id), &snapshot)
            .await
            .map(convert_mcp_runtime_snapshot)
            .map_err(GatewayMcpHandlerError::bad_request)
    }
}

impl RuntimeMcpHandler {
    async fn mcp_manager(&self) -> Arc<tokio::sync::Mutex<klaw_mcp::McpManager>> {
        let guard = self.runtime.mcp_init.lock().await;
        guard.manager()
    }

    async fn sync_snapshot(
        &self,
        config: &AppConfig,
    ) -> Result<GatewayMcpRuntimeSnapshot, GatewayMcpHandlerError> {
        let snapshot = McpConfigSnapshot::from_mcp_config(&config.mcp);
        let manager = self.mcp_manager().await;
        let mut guard = manager.lock().await;
        guard.sync(snapshot.clone()).await;
        Ok(convert_mcp_runtime_snapshot(
            guard.runtime_snapshot(&snapshot),
        ))
    }
}

fn build_mcp_server_config(
    request: GatewayMcpServerUpsertRequest,
    existing: Option<McpServerConfig>,
) -> Result<McpServerConfig, GatewayMcpHandlerError> {
    let id = match request.id {
        Some(id) => normalize_mcp_server_id(&id)?,
        None => match existing.as_ref() {
            Some(existing) => existing.id.clone(),
            None => {
                return Err(GatewayMcpHandlerError::bad_request(
                    "mcp server id is required",
                ));
            }
        },
    };
    let default = existing.unwrap_or_default();
    Ok(McpServerConfig {
        id,
        enabled: request.enabled.unwrap_or(default.enabled),
        mode: request.mode,
        tool_timeout_seconds: request
            .tool_timeout_seconds
            .unwrap_or(default.tool_timeout_seconds),
        command: request.command.or(default.command),
        args: request.args.unwrap_or(default.args),
        env: request.env.unwrap_or(default.env),
        cwd: request.cwd.or(default.cwd),
        url: request.url.or(default.url),
        headers: request.headers.unwrap_or(default.headers),
    })
}

fn normalize_mcp_server_id(id: &str) -> Result<String, GatewayMcpHandlerError> {
    let id = id.trim();
    if id.is_empty() {
        return Err(GatewayMcpHandlerError::bad_request(
            "mcp server id cannot be empty",
        ));
    }
    Ok(id.to_string())
}

fn redacted_mcp_server(server: &McpServerConfig) -> GatewayMcpServerConfigView {
    GatewayMcpServerConfigView {
        id: server.id.clone(),
        enabled: server.enabled,
        mode: server.mode.clone(),
        tool_timeout_seconds: server.tool_timeout_seconds,
        command: server.command.clone(),
        args: server.args.clone(),
        cwd: server.cwd.clone(),
        url: server.url.clone(),
        env_keys: server.env.keys().cloned().collect(),
        header_keys: server.headers.keys().cloned().collect(),
    }
}

fn convert_mcp_runtime_snapshot(snapshot: McpRuntimeSnapshot) -> GatewayMcpRuntimeSnapshot {
    GatewayMcpRuntimeSnapshot {
        statuses: snapshot
            .statuses
            .into_iter()
            .map(|status| GatewayMcpServerStatusView {
                id: status.key.as_str().to_string(),
                mode: status.mode,
                enabled: status.enabled,
                state: status.state.as_str().to_string(),
                last_error: status.last_error,
                tool_count: status.tool_count,
            })
            .collect(),
        details: snapshot
            .details
            .into_iter()
            .map(|detail| GatewayMcpServerDetailView {
                id: detail.key.as_str().to_string(),
                tools_list_response: detail.tools_list_response,
            })
            .collect(),
    }
}

fn gateway_mcp_config_error(err: ConfigError) -> GatewayMcpHandlerError {
    match err {
        ConfigError::InvalidConfig(message) if message.contains("not found") => {
            GatewayMcpHandlerError::not_found(message)
        }
        ConfigError::InvalidConfig(message) if message.contains("already exists") => {
            GatewayMcpHandlerError::conflict(message)
        }
        ConfigError::InvalidConfig(message) => GatewayMcpHandlerError::bad_request(message),
        other => GatewayMcpHandlerError::internal(other.to_string()),
    }
}

#[cfg(test)]
mod mcp_api_tests {
    use super::*;
    use klaw_config::McpServerMode;

    fn request_with_secret_maps(
        env: Option<BTreeMap<String, String>>,
        headers: Option<BTreeMap<String, String>>,
    ) -> GatewayMcpServerUpsertRequest {
        GatewayMcpServerUpsertRequest {
            id: Some("renamed".to_string()),
            enabled: Some(true),
            mode: McpServerMode::Stdio,
            tool_timeout_seconds: Some(45),
            command: Some("npx".to_string()),
            args: Some(vec!["server".to_string()]),
            env,
            cwd: None,
            url: None,
            headers,
        }
    }

    fn existing_server() -> McpServerConfig {
        McpServerConfig {
            id: "local".to_string(),
            enabled: true,
            mode: McpServerMode::Stdio,
            tool_timeout_seconds: 60,
            command: Some("old".to_string()),
            args: vec!["old-server".to_string()],
            env: BTreeMap::from([("API_KEY".to_string(), "secret".to_string())]),
            cwd: None,
            url: None,
            headers: BTreeMap::from([("Authorization".to_string(), "Bearer secret".to_string())]),
        }
    }

    #[test]
    fn mcp_update_request_preserves_env_and_headers_when_omitted() {
        let server = build_mcp_server_config(
            request_with_secret_maps(None, None),
            Some(existing_server()),
        )
        .expect("request should build");

        assert_eq!(server.id, "renamed");
        assert_eq!(
            server.env.get("API_KEY").map(String::as_str),
            Some("secret")
        );
        assert_eq!(
            server.headers.get("Authorization").map(String::as_str),
            Some("Bearer secret")
        );
    }

    #[test]
    fn mcp_update_request_clears_env_and_headers_when_empty_maps_are_sent() {
        let server = build_mcp_server_config(
            request_with_secret_maps(Some(BTreeMap::new()), Some(BTreeMap::new())),
            Some(existing_server()),
        )
        .expect("request should build");

        assert!(server.env.is_empty());
        assert!(server.headers.is_empty());
    }

    #[test]
    fn mcp_redacted_config_returns_only_secret_keys() {
        let view = redacted_mcp_server(&existing_server());

        assert_eq!(view.env_keys, vec!["API_KEY"]);
        assert_eq!(view.header_keys, vec!["Authorization"]);
    }

    #[test]
    fn mcp_create_request_requires_id() {
        let err = build_mcp_server_config(
            GatewayMcpServerUpsertRequest {
                id: None,
                enabled: None,
                mode: McpServerMode::Stdio,
                tool_timeout_seconds: None,
                command: None,
                args: None,
                env: None,
                cwd: None,
                url: None,
                headers: None,
            },
            None,
        )
        .expect_err("missing id should fail");

        assert_eq!(err.status.as_u16(), 400);
    }
}

struct RuntimeWebhookHandler {
    runtime: Arc<RuntimeBundle>,
    channel_availability: ChannelAvailability,
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
        let channel_availability = self.channel_availability.clone();
        tokio::spawn(async move {
            process_webhook_event(runtime, channel_availability, request).await;
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
            .map_err(GatewayWebhookHandlerError::not_found)?;
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
        let channel_availability = self.channel_availability.clone();
        tokio::spawn(async move {
            process_webhook_agent(runtime, channel_availability, request, content).await;
        });

        Ok(GatewayWebhookAgentResponse {
            request_id,
            status: WebhookEventStatus::Accepted.as_str().to_string(),
            hook_id,
            session_key,
        })
    }
}

async fn process_webhook_event(
    runtime: Arc<RuntimeBundle>,
    channel_availability: ChannelAvailability,
    request: GatewayWebhookRequest,
) {
    let manager = SqliteSessionManager::from_store(runtime.session_store.clone());
    debug!(
        webhook_kind = "events",
        event_id = request.event_id.as_str(),
        session_key = request.session_key.as_str(),
        "starting webhook event processing"
    );
    if let Some(reason) = request
        .base_session_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|base_session_key| {
            let channel = if base_session_key.starts_with("dingtalk:") {
                "dingtalk"
            } else if base_session_key.starts_with("telegram:") {
                "telegram"
            } else if base_session_key.starts_with("websocket:") {
                "websocket"
            } else {
                return None;
            };
            webhook_target_disabled_reason(&channel_availability, channel, base_session_key)
        })
    {
        debug!(
            webhook_kind = "events",
            event_id = request.event_id.as_str(),
            base_session_key = request.base_session_key.as_deref().unwrap_or_default(),
            reason = %reason,
            "skipping webhook event before agent loop because target channel is disabled"
        );
        let update = UpdateWebhookEventResult {
            status: WebhookEventStatus::Processed,
            error_message: None,
            response_summary: None,
            processed_at_ms: Some(now_ms()),
        };
        if let Err(err) = manager
            .update_webhook_event_status(&request.event_id, &update)
            .await
        {
            warn!(
                error = %err,
                webhook_event_id = request.event_id.as_str(),
                "failed to persist skipped webhook event status"
            );
        }
        return;
    }
    if let Some(base_session_key) = request
        .base_session_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && webhook_session_unavailable(&manager, base_session_key).await
    {
        debug!(
            webhook_kind = "events",
            event_id = request.event_id.as_str(),
            base_session_key,
            "skipping webhook event before agent loop because session is unavailable"
        );
        let update = UpdateWebhookEventResult {
            status: WebhookEventStatus::Processed,
            error_message: None,
            response_summary: None,
            processed_at_ms: Some(now_ms()),
        };
        if let Err(err) = manager
            .update_webhook_event_status(&request.event_id, &update)
            .await
        {
            warn!(
                error = %err,
                webhook_event_id = request.event_id.as_str(),
                "failed to persist skipped webhook event status"
            );
        }
        return;
    }
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
    channel_availability: ChannelAvailability,
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
    if let Some(reason) = request
        .base_session_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|base_session_key| {
            let channel = if base_session_key.starts_with("dingtalk:") {
                "dingtalk"
            } else if base_session_key.starts_with("telegram:") {
                "telegram"
            } else if base_session_key.starts_with("websocket:") {
                "websocket"
            } else {
                return None;
            };
            webhook_target_disabled_reason(&channel_availability, channel, base_session_key)
        })
    {
        debug!(
            webhook_kind = "agents",
            request_id = request.request_id.as_str(),
            hook_id = request.hook_id.as_str(),
            base_session_key = request.base_session_key.as_deref().unwrap_or_default(),
            reason = %reason,
            "skipping webhook agent before agent loop because target channel is disabled"
        );
        let update = UpdateWebhookAgentResult {
            status: WebhookEventStatus::Processed,
            error_message: None,
            response_summary: None,
            processed_at_ms: Some(now_ms()),
        };
        if let Err(err) = manager
            .update_webhook_agent_status(&request.request_id, &update)
            .await
        {
            warn!(
                error = %err,
                webhook_request_id = request.request_id.as_str(),
                "failed to persist skipped webhook agent status"
            );
        }
        return;
    }
    if let Some(base_session_key) = request
        .base_session_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && webhook_session_unavailable(&manager, base_session_key).await
    {
        debug!(
            webhook_kind = "agents",
            request_id = request.request_id.as_str(),
            hook_id = request.hook_id.as_str(),
            base_session_key,
            "skipping webhook agent before agent loop because session is unavailable"
        );
        let update = UpdateWebhookAgentResult {
            status: WebhookEventStatus::Processed,
            error_message: None,
            response_summary: None,
            processed_at_ms: Some(now_ms()),
        };
        if let Err(err) = manager
            .update_webhook_agent_status(&request.request_id, &update)
            .await
        {
            warn!(
                error = %err,
                webhook_request_id = request.request_id.as_str(),
                "failed to persist skipped webhook agent status"
            );
        }
        return;
    }
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

pub(crate) fn webhook_target_disabled_reason(
    availability: &ChannelAvailability,
    channel: &str,
    session_key: &str,
) -> Option<String> {
    availability.disabled_reason(channel, session_key)
}

async fn webhook_session_unavailable(manager: &SqliteSessionManager, session_key: &str) -> bool {
    let Ok(session) = manager.get_session(session_key).await else {
        return true;
    };
    let Some(active_session_key) = session
        .active_session_key
        .filter(|value| !value.trim().is_empty())
    else {
        return false;
    };
    manager.get_session(&active_session_key).await.is_err()
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
    build_webhook_agent_content(&template, &request.body)
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

fn build_webhook_agent_content(template: &str, body: &Value) -> Result<String, String> {
    let body_json = serde_json::to_string_pretty(body)
        .map_err(|err| format!("failed to serialize request body: {err}"))?;
    let template = template.trim_end();
    let mut content = String::new();
    if !template.is_empty() {
        content.push_str(template);
        content.push_str("\n\n");
    }
    content.push_str("## Request JSON\n\n```json\n");
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
    use super::{
        build_webhook_agent_content, webhook_session_unavailable, webhook_target_disabled_reason,
    };
    use crate::service_loop::ChannelAvailability;
    use klaw_config::{AppConfig, ChannelsConfig, TelegramConfig};
    use klaw_session::{SessionManager, SqliteSessionManager};
    use klaw_storage::{DefaultSessionStore, StoragePaths};
    use serde_json::json;
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn create_session_manager() -> SqliteSessionManager {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let root =
            std::env::temp_dir().join(format!("klaw-runtime-webhook-test-{now_ms}-{suffix}"));
        let store = DefaultSessionStore::open(StoragePaths::from_root(root))
            .await
            .expect("session store should open");
        SqliteSessionManager::from_store(store)
    }

    #[test]
    fn build_webhook_agent_content_appends_only_pretty_json_block() {
        let content = build_webhook_agent_content(
            "# Order Hook\n\nFollow the request.",
            &json!({"order_id":"A123","status":"paid"}),
        )
        .expect("content should build");

        assert!(content.contains("# Order Hook"));
        assert!(content.contains("## Request JSON"));
        assert!(content.contains("```json"));
        assert!(content.contains("\"order_id\": \"A123\""));
        assert!(!content.contains("## Hook Context"));
        assert!(!content.contains("Original Session Key"));
    }

    #[test]
    fn webhook_target_disabled_reason_uses_channel_availability() {
        let availability = ChannelAvailability::from_app_config(&AppConfig {
            channels: ChannelsConfig {
                telegram: vec![TelegramConfig {
                    id: "bot-a".to_string(),
                    enabled: true,
                    ..TelegramConfig::default()
                }],
                ..ChannelsConfig::default()
            },
            ..AppConfig::default()
        });

        assert_eq!(
            webhook_target_disabled_reason(&availability, "telegram", "telegram:bot-b:chat-1"),
            Some("target telegram channel 'bot-b' is disabled".to_string())
        );
        assert_eq!(
            webhook_target_disabled_reason(&availability, "telegram", "telegram:bot-a:chat-1"),
            None
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn webhook_session_unavailable_tracks_base_and_active_session() {
        let manager = create_session_manager().await;

        assert!(webhook_session_unavailable(&manager, "telegram:bot-a:missing").await);

        manager
            .get_or_create_session_state(
                "telegram:bot-a:chat-1",
                "chat-1",
                "telegram",
                "openai",
                "gpt-4o-mini",
            )
            .await
            .expect("base session should be created");
        assert!(!webhook_session_unavailable(&manager, "telegram:bot-a:chat-1").await);

        manager
            .set_active_session(
                "telegram:bot-a:chat-1",
                "chat-1",
                "telegram",
                "telegram:bot-a:deleted-child",
            )
            .await
            .expect("active session should update");
        assert!(webhook_session_unavailable(&manager, "telegram:bot-a:chat-1").await);
    }
}
