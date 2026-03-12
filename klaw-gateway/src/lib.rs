use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use futures_util::{sink::SinkExt, stream::StreamExt};
use klaw_config::GatewayConfig;
use serde::Deserialize;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use thiserror::Error;
use tokio::sync::{broadcast, RwLock};
use tracing::info;

const ROOM_BUFFER_SIZE: usize = 256;

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("invalid listen address '{0}:{1}': {2}")]
    InvalidListenAddress(String, u16, std::net::AddrParseError),
    #[error("TLS listener is not implemented yet; set gateway.tls.enabled=false")]
    TlsNotImplemented,
    #[error("failed to bind gateway listener: {0}")]
    Bind(#[source] std::io::Error),
    #[error("gateway server failed: {0}")]
    Serve(#[source] std::io::Error),
}

#[derive(Debug, Default)]
struct GatewayState {
    rooms: RwLock<HashMap<String, broadcast::Sender<String>>>,
}

#[derive(Debug, Deserialize)]
struct ChatQuery {
    session_key: Option<String>,
}

pub async fn run_gateway(config: &GatewayConfig) -> Result<(), GatewayError> {
    if config.tls.enabled {
        return Err(GatewayError::TlsNotImplemented);
    }

    let socket_addr: SocketAddr = format!("{}:{}", config.listen_ip, config.listen_port)
        .parse()
        .map_err(|err| {
            GatewayError::InvalidListenAddress(config.listen_ip.clone(), config.listen_port, err)
        })?;

    let state = Arc::new(GatewayState::default());
    let app = Router::new()
        .route("/ws/chat", get(ws_chat_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(socket_addr)
        .await
        .map_err(GatewayError::Bind)?;
    info!(listen_addr = %socket_addr, "gateway websocket server started");

    axum::serve(listener, app)
        .await
        .map_err(GatewayError::Serve)
}

async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<GatewayState>>,
    Query(query): Query<ChatQuery>,
) -> Response {
    let Some(session_key) = query
        .session_key
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return (StatusCode::BAD_REQUEST, "missing non-empty `session_key`").into_response();
    };

    ws.on_upgrade(move |socket| handle_socket(state, session_key, socket))
}

async fn handle_socket(state: Arc<GatewayState>, session_key: String, socket: WebSocket) {
    let tx = room_sender(&state, &session_key).await;
    let mut rx = tx.subscribe();
    let (mut ws_sink, mut ws_stream) = socket.split();
    let send_key = session_key.clone();
    let send_state = Arc::clone(&state);

    let send_task = tokio::spawn(async move {
        while let Ok(message) = rx.recv().await {
            if ws_sink.send(Message::Text(message.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(message)) = ws_stream.next().await {
        match message {
            Message::Text(text) => {
                let _ = tx.send(text.to_string());
            }
            Message::Binary(bytes) => {
                let payload = String::from_utf8_lossy(&bytes).to_string();
                let _ = tx.send(payload);
            }
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => {}
        }
    }

    send_task.abort();
    cleanup_room(send_state, send_key).await;
}

async fn room_sender(state: &GatewayState, session_key: &str) -> broadcast::Sender<String> {
    if let Some(sender) = state.rooms.read().await.get(session_key).cloned() {
        return sender;
    }

    let mut rooms = state.rooms.write().await;
    rooms
        .entry(session_key.to_string())
        .or_insert_with(|| {
            let (sender, _) = broadcast::channel(ROOM_BUFFER_SIZE);
            sender
        })
        .clone()
}

async fn cleanup_room(state: Arc<GatewayState>, session_key: String) {
    let mut rooms = state.rooms.write().await;
    let Some(sender) = rooms.get(&session_key) else {
        return;
    };
    if sender.receiver_count() == 0 {
        rooms.remove(&session_key);
    }
}
