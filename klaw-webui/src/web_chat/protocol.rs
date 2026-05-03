use serde::{Deserialize, Serialize};
use serde_json::Value;
use web_sys::WebSocket;

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(super) enum ClientRpcMessage<'a> {
    Request {
        id: &'a str,
        method: &'a str,
        #[serde(default)]
        params: Value,
    },
    Notification {
        method: &'a str,
        #[serde(default)]
        params: Value,
    },
}

#[derive(Clone, Debug)]
pub(super) enum RpcFrame {
    Error {
        #[allow(dead_code)]
        id: Option<String>,
        error: ServerErrorFrame,
    },
    Success {
        #[allow(dead_code)]
        id: String,
        result: Value,
    },
    Request {
        #[allow(dead_code)]
        id: String,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
}

impl<'de> Deserialize<'de> for RpcFrame {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        if value.get("type").is_some() {
            return Err(serde::de::Error::custom(
                "legacy websocket frames are not accepted by the v1 webui client",
            ));
        }
        if value.get("error").is_some() {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct ErrorFrame {
                #[serde(default)]
                id: Option<String>,
                error: ServerErrorFrame,
            }
            let frame = ErrorFrame::deserialize(value).map_err(serde::de::Error::custom)?;
            return Ok(Self::Error {
                id: frame.id,
                error: frame.error,
            });
        }
        if value.get("result").is_some() {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct SuccessFrame {
                id: String,
                #[serde(default)]
                result: Value,
            }
            let frame = SuccessFrame::deserialize(value).map_err(serde::de::Error::custom)?;
            return Ok(Self::Success {
                id: frame.id,
                result: frame.result,
            });
        }
        if value.get("method").is_some() {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct MethodFrame {
                #[serde(default)]
                id: Option<String>,
                method: String,
                #[serde(default)]
                params: Value,
            }
            let frame = MethodFrame::deserialize(value).map_err(serde::de::Error::custom)?;
            return Ok(match frame.id {
                Some(id) => Self::Request {
                    id,
                    method: frame.method,
                    params: frame.params,
                },
                None => Self::Notification {
                    method: frame.method,
                    params: frame.params,
                },
            });
        }
        Err(serde::de::Error::custom(
            "websocket frame is not a v1 rpc message",
        ))
    }
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct ServerErrorFrame {
    pub(super) code: String,
    pub(super) message: String,
}

pub(super) fn send_rpc_request(
    ws: &WebSocket,
    id: &str,
    method: &str,
    params: Value,
) -> Result<(), String> {
    let frame = ClientRpcMessage::Request { id, method, params };
    let payload = serde_json::to_string(&frame)
        .map_err(|err| format!("serialize websocket rpc request: {err}"))?;
    ws.send_with_str(&payload)
        .map_err(|_| format!("websocket send failed for rpc request '{method}'"))
}

pub(super) fn send_rpc_notification(
    ws: &WebSocket,
    method: &str,
    params: Value,
) -> Result<(), String> {
    let frame = ClientRpcMessage::Notification { method, params };
    let payload = serde_json::to_string(&frame)
        .map_err(|err| format!("serialize websocket rpc notification: {err}"))?;
    ws.send_with_str(&payload)
        .map_err(|_| format!("websocket send failed for rpc notification '{method}'"))
}
