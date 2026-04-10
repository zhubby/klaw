use serde::{Deserialize, Serialize};
use serde_json::Value;
use web_sys::WebSocket;

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ClientFrame<'a> {
    Method {
        id: &'a str,
        method: &'a str,
        #[serde(default)]
        params: Value,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ServerFrame {
    Event {
        event: String,
        #[serde(default)]
        payload: Value,
    },
    Result {
        #[allow(dead_code)]
        id: String,
        #[serde(default)]
        result: Value,
    },
    Error {
        #[allow(dead_code)]
        id: Option<String>,
        error: ServerErrorFrame,
    },
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct ServerErrorFrame {
    pub(super) code: String,
    pub(super) message: String,
}

pub(super) fn send_method(
    ws: &WebSocket,
    id: &str,
    method: &str,
    params: Value,
) -> Result<(), String> {
    let frame = ClientFrame::Method { id, method, params };
    let payload = serde_json::to_string(&frame)
        .map_err(|err| format!("serialize websocket method: {err}"))?;
    ws.send_with_str(&payload)
        .map_err(|_| format!("websocket send failed for method '{method}'"))
}
