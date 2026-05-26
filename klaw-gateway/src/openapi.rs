#![allow(dead_code)]

use crate::routes::Route;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use utoipa::{IntoParams, OpenApi, ToSchema};
use utoipa_scalar::{Scalar, Servable};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Klaw Gateway API",
        version = env!("CARGO_PKG_VERSION"),
        description = "HTTP management APIs exposed by the Klaw gateway."
    ),
    paths(
        openapi_json_docs,
        scalar_docs,
        ws_chat_docs,
        webhook_events_docs,
        webhook_agents_docs,
        archive_upload_docs,
        archive_download_docs,
        archive_list_docs,
        archive_get_docs,
        providers_list_docs,
        mcp_status_docs,
        mcp_list_servers_docs,
        mcp_create_server_docs,
        mcp_get_server_docs,
        mcp_update_server_docs,
        mcp_delete_server_docs,
        mcp_sync_docs,
        mcp_restart_server_docs,
        health_live_docs,
        health_ready_docs,
        health_status_docs,
        metrics_docs
    ),
    components(schemas(
        ErrorResponse,
        HealthStatusResponse,
        HealthComponent,
        ArchiveRecordSchema,
        ArchiveUploadResponseSchema,
        ArchiveListQuerySchema,
        ArchiveListResponseSchema,
        ArchiveGetResponseSchema,
        ProviderInfoSchema,
        ProvidersListResponseSchema,
        GatewayWebhookPayloadSchema,
        GatewayWebhookResponseSchema,
        GatewayWebhookAgentQuerySchema,
        GatewayWebhookAgentResponseSchema,
        GatewayMcpServerConfigViewSchema,
        GatewayMcpServerUpsertRequestSchema,
        GatewayMcpRuntimeSnapshotSchema,
        GatewayMcpServerStatusViewSchema,
        GatewayMcpServerDetailViewSchema,
        McpStatusResponseSchema,
        McpServersResponseSchema,
        McpServerResponseSchema
    )),
    tags(
        (name = "docs", description = "OpenAPI and Scalar documentation endpoints"),
        (name = "websocket", description = "Gateway WebSocket upgrade endpoint"),
        (name = "webhook", description = "Webhook ingestion endpoints"),
        (name = "archive", description = "Archive upload, lookup, and download endpoints"),
        (name = "providers", description = "Model provider discovery endpoints"),
        (name = "mcp", description = "MCP management endpoints"),
        (name = "health", description = "Health and metrics endpoints")
    )
)]
struct GatewayApiDoc;

pub(crate) async fn openapi_json_handler() -> Json<utoipa::openapi::OpenApi> {
    Json(gateway_openapi())
}

pub(crate) fn scalar_router() -> Scalar<utoipa::openapi::OpenApi> {
    Scalar::with_url(Route::Scalar.as_str(), gateway_openapi()).custom_html(SCALAR_HTML)
}

fn gateway_openapi() -> utoipa::openapi::OpenApi {
    GatewayApiDoc::openapi()
}

const SCALAR_HTML: &str = r#"<!doctype html>
<html>
<head>
    <title>Klaw Gateway API</title>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1"/>
    <meta name="openapi-url" content="/openapi.json"/>
</head>
<body>
<script id="api-reference" type="application/json">
    $spec
</script>
<script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
</body>
</html>
"#;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct ErrorResponse {
    success: bool,
    error: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct HealthStatusResponse {
    status: String,
    components: Vec<HealthComponent>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct HealthComponent {
    name: String,
    status: String,
    message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct ArchiveRecordSchema {
    id: String,
    source_kind: String,
    media_kind: String,
    mime_type: Option<String>,
    extension: Option<String>,
    original_filename: Option<String>,
    content_sha256: String,
    size_bytes: i64,
    storage_rel_path: String,
    session_key: Option<String>,
    channel: Option<String>,
    chat_id: Option<String>,
    message_id: Option<String>,
    metadata_json: String,
    created_at_ms: i64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct ArchiveUploadResponseSchema {
    success: bool,
    record: Option<ArchiveRecordSchema>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, IntoParams)]
struct ArchiveListQuerySchema {
    session_key: Option<String>,
    chat_id: Option<String>,
    source_kind: Option<String>,
    media_kind: Option<String>,
    filename: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct ArchiveListResponseSchema {
    success: bool,
    records: Vec<ArchiveRecordSchema>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct ArchiveGetResponseSchema {
    success: bool,
    record: Option<ArchiveRecordSchema>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct ProviderInfoSchema {
    id: String,
    name: Option<String>,
    base_url: String,
    wire_api: String,
    default_model: String,
    stream: bool,
    has_api_key: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct ProvidersListResponseSchema {
    success: bool,
    providers: Vec<ProviderInfoSchema>,
    default_provider: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct GatewayWebhookPayloadSchema {
    source: String,
    event_type: String,
    content: String,
    base_session_key: Option<String>,
    session_key: Option<String>,
    chat_id: Option<String>,
    sender_id: Option<String>,
    payload: Option<Value>,
    metadata: Option<BTreeMap<String, Value>>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct GatewayWebhookResponseSchema {
    event_id: String,
    status: String,
    session_key: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, IntoParams)]
struct GatewayWebhookAgentQuerySchema {
    hook_id: String,
    base_session_key: Option<String>,
    session_key: Option<String>,
    chat_id: Option<String>,
    sender_id: Option<String>,
    provider: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct GatewayWebhookAgentResponseSchema {
    request_id: String,
    status: String,
    hook_id: String,
    session_key: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct GatewayMcpServerConfigViewSchema {
    id: String,
    enabled: bool,
    #[schema(example = "stdio")]
    mode: String,
    tool_timeout_seconds: u64,
    command: Option<String>,
    args: Vec<String>,
    cwd: Option<String>,
    url: Option<String>,
    /// Environment variable names only. Secret values are never returned.
    env_keys: Vec<String>,
    /// Header names only. Secret values are never returned.
    header_keys: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct GatewayMcpServerUpsertRequestSchema {
    id: Option<String>,
    enabled: Option<bool>,
    #[schema(example = "stdio")]
    mode: String,
    tool_timeout_seconds: Option<u64>,
    command: Option<String>,
    args: Option<Vec<String>>,
    env: Option<BTreeMap<String, String>>,
    cwd: Option<String>,
    url: Option<String>,
    headers: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct GatewayMcpRuntimeSnapshotSchema {
    statuses: Vec<GatewayMcpServerStatusViewSchema>,
    details: Vec<GatewayMcpServerDetailViewSchema>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct GatewayMcpServerStatusViewSchema {
    id: String,
    #[schema(example = "stdio")]
    mode: String,
    enabled: bool,
    state: String,
    last_error: Option<String>,
    tool_count: usize,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct GatewayMcpServerDetailViewSchema {
    id: String,
    tools_list_response: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct McpStatusResponseSchema {
    success: bool,
    runtime: Option<GatewayMcpRuntimeSnapshotSchema>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct McpServersResponseSchema {
    success: bool,
    servers: Vec<GatewayMcpServerConfigViewSchema>,
    runtime: Option<GatewayMcpRuntimeSnapshotSchema>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct McpServerResponseSchema {
    success: bool,
    server: Option<GatewayMcpServerConfigViewSchema>,
    status: Option<GatewayMcpServerStatusViewSchema>,
    detail: Option<GatewayMcpServerDetailViewSchema>,
    runtime: Option<GatewayMcpRuntimeSnapshotSchema>,
    error: Option<String>,
}

#[utoipa::path(
    get,
    path = "/openapi.json",
    tag = "docs",
    responses((status = 200, description = "Gateway OpenAPI document"))
)]
fn openapi_json_docs() {}

#[utoipa::path(
    get,
    path = "/scalar",
    tag = "docs",
    responses((status = 200, description = "Scalar API reference UI", content_type = "text/html"))
)]
fn scalar_docs() {}

#[utoipa::path(
    get,
    path = "/ws/chat",
    tag = "websocket",
    responses((status = 101, description = "WebSocket upgrade for the Gateway chat protocol"))
)]
fn ws_chat_docs() {}

#[utoipa::path(
    post,
    path = "/webhook/events",
    tag = "webhook",
    request_body = GatewayWebhookPayloadSchema,
    responses(
        (status = 202, description = "Webhook event accepted", body = GatewayWebhookResponseSchema),
        (status = 400, description = "Invalid webhook payload", body = ErrorResponse),
        (status = 401, description = "Invalid webhook auth", body = String)
    )
)]
fn webhook_events_docs() {}

#[utoipa::path(
    post,
    path = "/webhook/agents",
    tag = "webhook",
    params(GatewayWebhookAgentQuerySchema),
    request_body = Value,
    responses(
        (status = 202, description = "Webhook agent request accepted", body = GatewayWebhookAgentResponseSchema),
        (status = 400, description = "Invalid webhook agent payload", body = String),
        (status = 401, description = "Invalid webhook auth", body = String)
    )
)]
fn webhook_agents_docs() {}

#[utoipa::path(
    post,
    path = "/archive/upload",
    tag = "archive",
    request_body(content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Archive file uploaded", body = ArchiveUploadResponseSchema),
        (status = 400, description = "Invalid upload", body = ArchiveUploadResponseSchema),
        (status = 503, description = "Archive service unavailable", body = ArchiveUploadResponseSchema)
    )
)]
fn archive_upload_docs() {}

#[utoipa::path(
    get,
    path = "/archive/download/{id}",
    tag = "archive",
    params(("id" = String, Path, description = "Archive record id")),
    responses(
        (status = 200, description = "Archive file bytes", content_type = "application/octet-stream"),
        (status = 404, description = "Archive file not found", body = ErrorResponse),
        (status = 503, description = "Archive service unavailable", body = ErrorResponse)
    )
)]
fn archive_download_docs() {}

#[utoipa::path(
    get,
    path = "/archive/list",
    tag = "archive",
    params(ArchiveListQuerySchema),
    responses(
        (status = 200, description = "Archive records", body = ArchiveListResponseSchema),
        (status = 503, description = "Archive service unavailable", body = ArchiveListResponseSchema)
    )
)]
fn archive_list_docs() {}

#[utoipa::path(
    get,
    path = "/archive/{id}",
    tag = "archive",
    params(("id" = String, Path, description = "Archive record id")),
    responses(
        (status = 200, description = "Archive record", body = ArchiveGetResponseSchema),
        (status = 404, description = "Archive record not found", body = ArchiveGetResponseSchema),
        (status = 503, description = "Archive service unavailable", body = ArchiveGetResponseSchema)
    )
)]
fn archive_get_docs() {}

#[utoipa::path(
    get,
    path = "/providers/list",
    tag = "providers",
    responses(
        (status = 200, description = "Configured providers", body = ProvidersListResponseSchema),
        (status = 503, description = "Providers service unavailable", body = ProvidersListResponseSchema)
    )
)]
fn providers_list_docs() {}

#[utoipa::path(
    get,
    path = "/mcp/status",
    tag = "mcp",
    responses(
        (status = 200, description = "MCP runtime snapshot", body = McpStatusResponseSchema),
        (status = 503, description = "MCP service unavailable", body = McpStatusResponseSchema)
    )
)]
fn mcp_status_docs() {}

#[utoipa::path(
    get,
    path = "/mcp/servers",
    tag = "mcp",
    responses(
        (status = 200, description = "Redacted MCP server configs and runtime snapshot", body = McpServersResponseSchema),
        (status = 503, description = "MCP service unavailable", body = McpStatusResponseSchema)
    )
)]
fn mcp_list_servers_docs() {}

#[utoipa::path(
    post,
    path = "/mcp/servers",
    tag = "mcp",
    request_body = GatewayMcpServerUpsertRequestSchema,
    responses(
        (status = 200, description = "Created MCP server and synchronized runtime", body = McpServerResponseSchema),
        (status = 400, description = "Invalid MCP server config", body = McpStatusResponseSchema),
        (status = 409, description = "MCP server id already exists", body = McpStatusResponseSchema),
        (status = 503, description = "MCP service unavailable", body = McpStatusResponseSchema)
    )
)]
fn mcp_create_server_docs() {}

#[utoipa::path(
    get,
    path = "/mcp/servers/{id}",
    tag = "mcp",
    params(("id" = String, Path, description = "MCP server id")),
    responses(
        (status = 200, description = "Redacted MCP server config and runtime detail", body = McpServerResponseSchema),
        (status = 404, description = "MCP server not found", body = McpStatusResponseSchema),
        (status = 503, description = "MCP service unavailable", body = McpStatusResponseSchema)
    )
)]
fn mcp_get_server_docs() {}

#[utoipa::path(
    put,
    path = "/mcp/servers/{id}",
    tag = "mcp",
    params(("id" = String, Path, description = "Existing MCP server id to replace")),
    request_body = GatewayMcpServerUpsertRequestSchema,
    responses(
        (status = 200, description = "Updated MCP server and synchronized runtime", body = McpServerResponseSchema),
        (status = 400, description = "Invalid MCP server config", body = McpStatusResponseSchema),
        (status = 404, description = "MCP server not found", body = McpStatusResponseSchema),
        (status = 409, description = "Renamed MCP server id already exists", body = McpStatusResponseSchema),
        (status = 503, description = "MCP service unavailable", body = McpStatusResponseSchema)
    )
)]
fn mcp_update_server_docs() {}

#[utoipa::path(
    delete,
    path = "/mcp/servers/{id}",
    tag = "mcp",
    params(("id" = String, Path, description = "MCP server id")),
    responses(
        (status = 200, description = "Deleted MCP server and synchronized runtime", body = McpStatusResponseSchema),
        (status = 404, description = "MCP server not found", body = McpStatusResponseSchema),
        (status = 503, description = "MCP service unavailable", body = McpStatusResponseSchema)
    )
)]
fn mcp_delete_server_docs() {}

#[utoipa::path(
    post,
    path = "/mcp/sync",
    tag = "mcp",
    responses(
        (status = 200, description = "MCP runtime synchronized from disk config", body = McpStatusResponseSchema),
        (status = 503, description = "MCP service unavailable or busy", body = McpStatusResponseSchema)
    )
)]
fn mcp_sync_docs() {}

#[utoipa::path(
    post,
    path = "/mcp/servers/{id}/restart",
    tag = "mcp",
    params(("id" = String, Path, description = "MCP server id")),
    responses(
        (status = 200, description = "Restarted stdio MCP server and returned runtime snapshot", body = McpStatusResponseSchema),
        (status = 400, description = "Server cannot be restarted", body = McpStatusResponseSchema),
        (status = 404, description = "MCP server not found", body = McpStatusResponseSchema),
        (status = 503, description = "MCP service unavailable or busy", body = McpStatusResponseSchema)
    )
)]
fn mcp_restart_server_docs() {}

#[utoipa::path(
    get,
    path = "/health/live",
    tag = "health",
    responses((status = 200, description = "Gateway is live", body = String, content_type = "text/plain"))
)]
fn health_live_docs() {}

#[utoipa::path(
    get,
    path = "/health/ready",
    tag = "health",
    responses((status = 200, description = "Gateway is ready", body = String, content_type = "text/plain"))
)]
fn health_ready_docs() {}

#[utoipa::path(
    get,
    path = "/health/status",
    tag = "health",
    responses((status = 200, description = "Gateway health status", body = HealthStatusResponse))
)]
fn health_status_docs() {}

#[utoipa::path(
    get,
    path = "/metrics",
    tag = "health",
    responses(
        (status = 200, description = "Prometheus metrics", body = String, content_type = "text/plain"),
        (status = 404, description = "Prometheus metrics are not enabled", body = String, content_type = "text/plain")
    )
)]
fn metrics_docs() {}
