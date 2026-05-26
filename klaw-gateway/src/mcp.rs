use async_trait::async_trait;
use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use klaw_config::McpServerMode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

use crate::state::GatewayState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GatewayMcpServerConfigView {
    pub id: String,
    pub enabled: bool,
    pub mode: McpServerMode,
    pub tool_timeout_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default)]
    pub env_keys: Vec<String>,
    #[serde(default)]
    pub header_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GatewayMcpRuntimeSnapshot {
    pub statuses: Vec<GatewayMcpServerStatusView>,
    pub details: Vec<GatewayMcpServerDetailView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GatewayMcpServerStatusView {
    pub id: String,
    pub mode: McpServerMode,
    pub enabled: bool,
    pub state: String,
    pub last_error: Option<String>,
    pub tool_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GatewayMcpServerDetailView {
    pub id: String,
    pub tools_list_response: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GatewayMcpServerUpsertRequest {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    pub mode: McpServerMode,
    #[serde(default)]
    pub tool_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone)]
pub struct GatewayMcpHandlerError {
    pub status: StatusCode,
    pub message: String,
}

impl GatewayMcpHandlerError {
    #[must_use]
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

#[async_trait]
pub trait GatewayMcpHandler: Send + Sync {
    async fn status(&self) -> Result<GatewayMcpRuntimeSnapshot, GatewayMcpHandlerError>;

    async fn list_servers(
        &self,
    ) -> Result<(Vec<GatewayMcpServerConfigView>, GatewayMcpRuntimeSnapshot), GatewayMcpHandlerError>;

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
    >;

    async fn create_server(
        &self,
        request: GatewayMcpServerUpsertRequest,
    ) -> Result<(GatewayMcpServerConfigView, GatewayMcpRuntimeSnapshot), GatewayMcpHandlerError>;

    async fn update_server(
        &self,
        id: String,
        request: GatewayMcpServerUpsertRequest,
    ) -> Result<(GatewayMcpServerConfigView, GatewayMcpRuntimeSnapshot), GatewayMcpHandlerError>;

    async fn delete_server(
        &self,
        id: String,
    ) -> Result<GatewayMcpRuntimeSnapshot, GatewayMcpHandlerError>;

    async fn sync(&self) -> Result<GatewayMcpRuntimeSnapshot, GatewayMcpHandlerError>;

    async fn restart_server(
        &self,
        id: String,
    ) -> Result<GatewayMcpRuntimeSnapshot, GatewayMcpHandlerError>;
}

#[derive(Debug, Serialize)]
struct McpStatusResponse {
    success: bool,
    runtime: Option<GatewayMcpRuntimeSnapshot>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct McpServersResponse {
    success: bool,
    servers: Vec<GatewayMcpServerConfigView>,
    runtime: Option<GatewayMcpRuntimeSnapshot>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct McpServerResponse {
    success: bool,
    server: Option<GatewayMcpServerConfigView>,
    status: Option<GatewayMcpServerStatusView>,
    detail: Option<GatewayMcpServerDetailView>,
    runtime: Option<GatewayMcpRuntimeSnapshot>,
    error: Option<String>,
}

fn mcp_unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(McpStatusResponse {
            success: false,
            runtime: None,
            error: Some("mcp service not available".to_string()),
        }),
    )
        .into_response()
}

fn mcp_error(err: GatewayMcpHandlerError) -> Response {
    (
        err.status,
        Json(McpStatusResponse {
            success: false,
            runtime: None,
            error: Some(err.message),
        }),
    )
        .into_response()
}

pub async fn mcp_status_handler(State(state): State<Arc<GatewayState>>) -> Response {
    let Some(mcp) = state.mcp.as_ref() else {
        return mcp_unavailable();
    };
    match mcp.handler.status().await {
        Ok(runtime) => Json(McpStatusResponse {
            success: true,
            runtime: Some(runtime),
            error: None,
        })
        .into_response(),
        Err(err) => mcp_error(err),
    }
}

pub async fn mcp_list_servers_handler(State(state): State<Arc<GatewayState>>) -> Response {
    let Some(mcp) = state.mcp.as_ref() else {
        return mcp_unavailable();
    };
    match mcp.handler.list_servers().await {
        Ok((servers, runtime)) => Json(McpServersResponse {
            success: true,
            servers,
            runtime: Some(runtime),
            error: None,
        })
        .into_response(),
        Err(err) => mcp_error(err),
    }
}

pub async fn mcp_get_server_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Response {
    let Some(mcp) = state.mcp.as_ref() else {
        return mcp_unavailable();
    };
    match mcp.handler.get_server(id).await {
        Ok((server, status, detail)) => Json(McpServerResponse {
            success: true,
            server: Some(server),
            status,
            detail,
            runtime: None,
            error: None,
        })
        .into_response(),
        Err(err) => mcp_error(err),
    }
}

pub async fn mcp_create_server_handler(
    State(state): State<Arc<GatewayState>>,
    request: Result<Json<GatewayMcpServerUpsertRequest>, JsonRejection>,
) -> Response {
    let Some(mcp) = state.mcp.as_ref() else {
        return mcp_unavailable();
    };
    let request = match request {
        Ok(Json(request)) => request,
        Err(err) => {
            return mcp_error(GatewayMcpHandlerError::bad_request(format!(
                "invalid mcp server payload: {err}"
            )));
        }
    };
    match mcp.handler.create_server(request).await {
        Ok((server, runtime)) => Json(McpServerResponse {
            success: true,
            server: Some(server),
            status: None,
            detail: None,
            runtime: Some(runtime),
            error: None,
        })
        .into_response(),
        Err(err) => mcp_error(err),
    }
}

pub async fn mcp_update_server_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
    request: Result<Json<GatewayMcpServerUpsertRequest>, JsonRejection>,
) -> Response {
    let Some(mcp) = state.mcp.as_ref() else {
        return mcp_unavailable();
    };
    let request = match request {
        Ok(Json(request)) => request,
        Err(err) => {
            return mcp_error(GatewayMcpHandlerError::bad_request(format!(
                "invalid mcp server payload: {err}"
            )));
        }
    };
    match mcp.handler.update_server(id, request).await {
        Ok((server, runtime)) => Json(McpServerResponse {
            success: true,
            server: Some(server),
            status: None,
            detail: None,
            runtime: Some(runtime),
            error: None,
        })
        .into_response(),
        Err(err) => mcp_error(err),
    }
}

pub async fn mcp_delete_server_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Response {
    let Some(mcp) = state.mcp.as_ref() else {
        return mcp_unavailable();
    };
    match mcp.handler.delete_server(id).await {
        Ok(runtime) => Json(McpStatusResponse {
            success: true,
            runtime: Some(runtime),
            error: None,
        })
        .into_response(),
        Err(err) => mcp_error(err),
    }
}

pub async fn mcp_sync_handler(State(state): State<Arc<GatewayState>>) -> Response {
    let Some(mcp) = state.mcp.as_ref() else {
        return mcp_unavailable();
    };
    match mcp.handler.sync().await {
        Ok(runtime) => Json(McpStatusResponse {
            success: true,
            runtime: Some(runtime),
            error: None,
        })
        .into_response(),
        Err(err) => mcp_error(err),
    }
}

pub async fn mcp_restart_server_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> Response {
    let Some(mcp) = state.mcp.as_ref() else {
        return mcp_unavailable();
    };
    match mcp.handler.restart_server(id).await {
        Ok(runtime) => Json(McpStatusResponse {
            success: true,
            runtime: Some(runtime),
            error: None,
        })
        .into_response(),
        Err(err) => mcp_error(err),
    }
}
