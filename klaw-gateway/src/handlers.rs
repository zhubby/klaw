use crate::state::GatewayState;
use axum::{
    body::Body,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use std::sync::Arc;

pub(crate) async fn health_live_handler(State(state): State<Arc<GatewayState>>) -> Response {
    let status = state.health.liveness();
    let code = if status.is_healthy() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (code, format!("{}\n", status.as_str())).into_response()
}

pub(crate) async fn health_ready_handler(State(state): State<Arc<GatewayState>>) -> Response {
    let status = state.health.readiness();
    let code = if status.is_healthy() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (code, format!("{}\n", status.as_str())).into_response()
}

pub(crate) async fn health_status_handler(State(state): State<Arc<GatewayState>>) -> Response {
    let status = state.health.overall_status();
    let components: Vec<serde_json::Value> = state
        .health
        .all_components()
        .into_iter()
        .map(|component| {
            serde_json::json!({
                "name": component.name,
                "status": component.status.as_str(),
                "message": component.message,
            })
        })
        .collect();
    let body = serde_json::json!({
        "status": status.as_str(),
        "components": components,
    });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap_or_default()))
        .unwrap()
}

pub(crate) async fn metrics_handler(State(state): State<Arc<GatewayState>>) -> Response {
    match &state.prometheus {
        Some(exporter) => match exporter.render_metrics() {
            Ok(body) => Response::builder()
                .status(StatusCode::OK)
                .header(
                    header::CONTENT_TYPE,
                    "text/plain; version=0.0.4; charset=utf-8",
                )
                .body(Body::from(body))
                .unwrap_or_else(|_| {
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("failed to build response"))
                        .unwrap()
                }),
            Err(err) => {
                tracing::warn!(error = %err, "failed to render prometheus metrics");
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from(format!("failed to render metrics: {err}")))
                    .unwrap()
            }
        },
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Prometheus metrics not enabled\n"))
            .unwrap(),
    }
}
