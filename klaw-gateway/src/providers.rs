use axum::{Json, extract::{Query, State}, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::state::GatewayState;

#[derive(Debug, Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: Option<String>,
    pub base_url: String,
    pub wire_api: String,
    pub default_model: String,
    pub stream: bool,
    pub has_api_key: bool,
}

#[derive(Debug, Deserialize)]
pub struct ProvidersListQuery {
    #[serde(default)]
    pub include_disabled: bool,
}

#[derive(Debug, Serialize)]
pub struct ProvidersListResponse {
    pub success: bool,
    pub providers: Vec<ProviderInfo>,
    pub default_provider: Option<String>,
    pub error: Option<String>,
}

pub async fn providers_list_handler(
    State(state): State<Arc<GatewayState>>,
    Query(_params): Query<ProvidersListQuery>,
) -> impl IntoResponse {
    let Some(ref providers_state) = state.providers else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ProvidersListResponse {
                success: false,
                providers: vec![],
                default_provider: None,
                error: Some("providers service not available".to_string()),
            }),
        );
    };

    let providers: Vec<ProviderInfo> = providers_state
        .providers
        .iter()
        .map(|(id, config)| {
            let has_api_key = config.api_key.is_some()
                || config
                    .env_key
                    .as_ref()
                    .and_then(|key| std::env::var(key).ok())
                    .is_some();

            ProviderInfo {
                id: id.clone(),
                name: config.name.clone(),
                base_url: config.base_url.clone(),
                wire_api: config.wire_api.clone(),
                default_model: config.default_model.clone(),
                stream: config.stream,
                has_api_key,
            }
        })
        .collect();

    info!(count = providers.len(), "providers list retrieved");

    (
        StatusCode::OK,
        Json(ProvidersListResponse {
            success: true,
            providers,
            default_provider: Some(providers_state.default_provider.clone()),
            error: None,
        }),
    )
}
