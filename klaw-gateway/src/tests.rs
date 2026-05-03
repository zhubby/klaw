#[cfg(test)]
mod tests {
    use crate::{
        GatewayOptions, GatewayProviderCatalog, GatewayProviderEntry, GatewaySessionHistoryMessage,
        GatewaySessionHistoryPage, GatewayWebsocketHandler, GatewayWebsocketHandlerError,
        GatewayWebsocketServerFrame, GatewayWebsocketSubmitRequest, GatewayWorkspaceBootstrap,
        GatewayWorkspaceSession, OutboundEvent, Route, spawn_gateway, spawn_gateway_with_options,
        webhook::{
            GatewayWebhookAgentQuery, GatewayWebhookPayload, normalize_webhook_agent_request,
            normalize_webhook_request,
        },
    };
    use async_trait::async_trait;
    use futures_util::{SinkExt, StreamExt};
    use klaw_config::{GatewayAuthConfig, GatewayConfig};
    use reqwest::StatusCode;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tokio::time::timeout;
    use tokio_tungstenite::{connect_async, tungstenite::Message};

    #[derive(Clone, Default)]
    struct RecordingWebsocketHandler {
        requests: Arc<Mutex<Vec<GatewayWebsocketSubmitRequest>>>,
    }

    #[async_trait]
    impl GatewayWebsocketHandler for RecordingWebsocketHandler {
        async fn bootstrap(
            &self,
        ) -> Result<GatewayWorkspaceBootstrap, GatewayWebsocketHandlerError> {
            Ok(GatewayWorkspaceBootstrap {
                sessions: vec![
                    GatewayWorkspaceSession {
                        session_key: "websocket:older".to_string(),
                        title: "Agent 1".to_string(),
                        created_at_ms: 10,
                        model_provider: Some("openai".to_string()),
                        model: Some("gpt-4.1-mini".to_string()),
                    },
                    GatewayWorkspaceSession {
                        session_key: "websocket:newer".to_string(),
                        title: "Agent 2".to_string(),
                        created_at_ms: 20,
                        model_provider: Some("anthropic".to_string()),
                        model: Some("claude-sonnet-4-5".to_string()),
                    },
                ],
                active_session_key: Some("websocket:newer".to_string()),
            })
        }

        async fn create_session(
            &self,
        ) -> Result<GatewayWorkspaceSession, GatewayWebsocketHandlerError> {
            Ok(GatewayWorkspaceSession {
                session_key: "websocket:created".to_string(),
                title: "Agent 3".to_string(),
                created_at_ms: 30,
                model_provider: Some("openai".to_string()),
                model: Some("gpt-4.1-mini".to_string()),
            })
        }

        async fn update_session(
            &self,
            session_key: &str,
            title: String,
        ) -> Result<GatewayWorkspaceSession, GatewayWebsocketHandlerError> {
            Ok(GatewayWorkspaceSession {
                session_key: session_key.to_string(),
                title,
                created_at_ms: 20,
                model_provider: Some("anthropic".to_string()),
                model: Some("claude-sonnet-4-5".to_string()),
            })
        }

        async fn delete_session(
            &self,
            session_key: &str,
        ) -> Result<bool, GatewayWebsocketHandlerError> {
            Ok(session_key == "websocket:newer")
        }

        async fn load_session_history(
            &self,
            session_key: &str,
            before_message_id: Option<&str>,
            limit: usize,
        ) -> Result<GatewaySessionHistoryPage, GatewayWebsocketHandlerError> {
            if session_key == "websocket:history" {
                let messages = match before_message_id {
                    None => vec![GatewaySessionHistoryMessage {
                        role: "assistant".to_string(),
                        content: format!("previous answer ({limit})"),
                        timestamp_ms: 42,
                        metadata: std::collections::BTreeMap::new(),
                        message_id: Some("msg-2".to_string()),
                    }],
                    Some("msg-2") => vec![GatewaySessionHistoryMessage {
                        role: "user".to_string(),
                        content: "older question".to_string(),
                        timestamp_ms: 21,
                        metadata: std::collections::BTreeMap::new(),
                        message_id: Some("msg-1".to_string()),
                    }],
                    Some(other) => {
                        return Err(GatewayWebsocketHandlerError::invalid_request(format!(
                            "unknown cursor {other}"
                        )));
                    }
                };
                return Ok(GatewaySessionHistoryPage {
                    has_more: before_message_id.is_none(),
                    oldest_loaded_message_id: messages
                        .first()
                        .and_then(|message| message.message_id.clone()),
                    messages,
                });
            }
            Ok(GatewaySessionHistoryPage {
                messages: Vec::new(),
                has_more: false,
                oldest_loaded_message_id: None,
            })
        }

        async fn list_providers(
            &self,
        ) -> Result<GatewayProviderCatalog, GatewayWebsocketHandlerError> {
            Ok(GatewayProviderCatalog {
                default_provider: "anthropic".to_string(),
                providers: vec![
                    GatewayProviderEntry {
                        id: "anthropic".to_string(),
                        default_model: "claude-sonnet-4-5".to_string(),
                    },
                    GatewayProviderEntry {
                        id: "openai".to_string(),
                        default_model: "gpt-4.1-mini".to_string(),
                    },
                ],
            })
        }

        async fn submit(
            &self,
            request: GatewayWebsocketSubmitRequest,
            frame_tx: mpsc::UnboundedSender<GatewayWebsocketServerFrame>,
        ) -> Result<(), GatewayWebsocketHandlerError> {
            self.requests
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(request.clone());
            frame_tx
                .send(GatewayWebsocketServerFrame::Result {
                    id: request.request_id,
                    result: json!({
                        "response": {
                            "content": format!("ack: {}", request.input),
                        },
                        "session_key": request.session_key,
                        "stream": false,
                    }),
                })
                .map_err(|_| GatewayWebsocketHandlerError::internal("connection closed"))?;
            Ok(())
        }
    }

    fn test_gateway_config() -> GatewayConfig {
        GatewayConfig {
            enabled: true,
            listen_ip: "127.0.0.1".to_string(),
            listen_port: 0,
            auth: Default::default(),
            tailscale: Default::default(),
            tls: Default::default(),
            webhook: Default::default(),
        }
    }

    fn ws_url(port: u16, token: Option<&str>) -> String {
        let mut url = format!("ws://127.0.0.1:{port}{}", Route::WsChat.as_str());
        if let Some(token) = token {
            url.push_str("?token=");
            url.push_str(token);
        }
        url
    }

    #[tokio::test]
    async fn spawn_gateway_uses_actual_random_port() {
        let config = GatewayConfig {
            enabled: true,
            listen_ip: "127.0.0.1".to_string(),
            listen_port: 0,
            auth: Default::default(),
            tailscale: Default::default(),
            tls: Default::default(),
            webhook: Default::default(),
        };

        let handle = match spawn_gateway(&config).await {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };
        assert!(handle.info().actual_port > 0);
        assert!(
            handle
                .info()
                .ws_url
                .contains(&handle.info().actual_port.to_string())
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn gateway_root_route_serves_home_page_and_logo() {
        let config = GatewayConfig {
            enabled: true,
            listen_ip: "127.0.0.1".to_string(),
            listen_port: 0,
            auth: Default::default(),
            tailscale: Default::default(),
            tls: Default::default(),
            webhook: Default::default(),
        };

        let handle = match spawn_gateway(&config).await {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let base_url = format!("http://127.0.0.1:{}", handle.info().actual_port);
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("reqwest client");

        let home_response = client
            .get(format!("{base_url}{}", Route::Home.as_str()))
            .send()
            .await
            .expect("home page should respond");
        assert_eq!(home_response.status(), StatusCode::OK);
        let home_html = home_response
            .text()
            .await
            .expect("home page body should load");
        assert!(home_html.contains("Little Claws, Big Conversations."));
        assert!(home_html.contains(Route::HomeLogo.as_str()));
        assert!(home_html.contains("href=\"/chat\""));
        assert!(home_html.contains("Open web chat"));
        assert!(!home_html.contains("Klaw Gateway is the friendly little harbor of the system."));

        let logo_response = client
            .get(format!("{base_url}{}", Route::HomeLogo.as_str()))
            .send()
            .await
            .expect("logo should respond");
        assert_eq!(logo_response.status(), StatusCode::OK);
        assert_eq!(
            logo_response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("image/webp")
        );
        assert!(
            !logo_response
                .bytes()
                .await
                .expect("logo body should load")
                .is_empty()
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn gateway_chat_route_serves_embedded_webui_assets() {
        let config = test_gateway_config();

        let handle = match spawn_gateway(&config).await {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let base_url = format!("http://127.0.0.1:{}", handle.info().actual_port);
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("reqwest client");

        let chat_html = client
            .get(format!("{base_url}{}", Route::Chat.as_str()))
            .send()
            .await
            .expect("chat page should respond");
        assert_eq!(chat_html.status(), StatusCode::OK);
        let body = chat_html.text().await.expect("chat body");
        assert!(body.contains("klaw_chat_canvas"));
        assert!(body.contains(Route::ChatDistJs.as_str()));

        let js = client
            .get(format!("{base_url}{}", Route::ChatDistJs.as_str()))
            .send()
            .await
            .expect("chat js should respond");
        assert_eq!(js.status(), StatusCode::OK);
        assert_eq!(
            js.headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/javascript; charset=utf-8")
        );
        assert!(!js.bytes().await.expect("js body").is_empty());

        let wasm = client
            .get(format!("{base_url}{}", Route::ChatDistWasm.as_str()))
            .send()
            .await
            .expect("chat wasm should respond");
        assert_eq!(wasm.status(), StatusCode::OK);
        assert_eq!(
            wasm.headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/wasm")
        );
        assert!(wasm.bytes().await.expect("wasm body").starts_with(b"\0asm"));

        handle.shutdown().await.expect("gateway should stop");
    }

    #[test]
    fn exported_route_constants_match_expected_paths() {
        assert_eq!(Route::Home.as_str(), "/");
        assert_eq!(Route::HomeLogo.as_str(), "/logo.webp");
        assert_eq!(Route::Chat.as_str(), "/chat");
        assert_eq!(Route::ChatDistJs.as_str(), "/chat/dist/klaw_webui.js");
        assert_eq!(
            Route::ChatDistWasm.as_str(),
            "/chat/dist/klaw_webui_bg.wasm"
        );
        assert_eq!(Route::WsChat.as_str(), "/ws/chat");
        assert_eq!(Route::WebhookEvents.as_str(), "/webhook/events");
        assert_eq!(Route::WebhookAgents.as_str(), "/webhook/agents");
    }

    #[test]
    fn normalize_webhook_request_applies_defaults() {
        let request = normalize_webhook_request(
            GatewayWebhookPayload {
                source: "github".to_string(),
                event_type: "issue_comment.created".to_string(),
                content: "New comment".to_string(),
                base_session_key: Some("telegram:chat-1".to_string()),
                session_key: None,
                chat_id: None,
                sender_id: None,
                payload: Some(json!({"action":"created"})),
                metadata: None,
            },
            None,
        )
        .expect("payload should normalize");

        assert!(request.session_key.starts_with("webhook:github:"));
        assert_eq!(request.chat_id, request.session_key);
        assert_eq!(request.base_session_key.as_deref(), Some("telegram:chat-1"));
        assert_eq!(request.sender_id, "github:webhook");
        assert_eq!(
            request.metadata.get("trigger.kind"),
            Some(&json!("webhook"))
        );
        assert_eq!(
            request.metadata.get("webhook.base_session_key"),
            Some(&json!("telegram:chat-1"))
        );
    }

    #[test]
    fn normalize_webhook_agent_request_applies_defaults() {
        let request = normalize_webhook_agent_request(
            GatewayWebhookAgentQuery {
                hook_id: "order_sync".to_string(),
                base_session_key: Some("dingtalk:acc:chat-1".to_string()),
                session_key: None,
                chat_id: None,
                sender_id: None,
                provider: None,
                model: None,
            },
            json!({"order_id":"A123","status":"paid"}),
            None,
        )
        .expect("payload should normalize");

        assert!(request.session_key.starts_with("webhook:order_sync:"));
        assert_eq!(request.chat_id, request.session_key);
        assert_eq!(
            request.base_session_key.as_deref(),
            Some("dingtalk:acc:chat-1")
        );
        assert_eq!(request.sender_id, "webhook-agent:order_sync");
        assert_eq!(request.provider, None);
        assert_eq!(request.model, None);
        assert_eq!(request.metadata.get("webhook.kind"), Some(&json!("agents")));
        assert_eq!(
            request.metadata.get("webhook.base_session_key"),
            Some(&json!("dingtalk:acc:chat-1"))
        );
    }

    #[test]
    fn normalize_webhook_agent_request_accepts_legacy_session_key_alias() {
        let request = normalize_webhook_agent_request(
            GatewayWebhookAgentQuery {
                hook_id: "order_sync".to_string(),
                base_session_key: None,
                session_key: Some("telegram:acc:chat-legacy".to_string()),
                chat_id: None,
                sender_id: None,
                provider: None,
                model: None,
            },
            json!({"order_id":"A123"}),
            None,
        )
        .expect("legacy alias should normalize");

        assert_eq!(
            request.base_session_key.as_deref(),
            Some("telegram:acc:chat-legacy")
        );
    }

    #[tokio::test]
    async fn websocket_rejects_connections_without_required_token() {
        let mut config = test_gateway_config();
        config.auth = GatewayAuthConfig {
            enabled: true,
            token: Some("secret-token".to_string()),
            env_key: None,
        };

        let handle = match spawn_gateway_with_options(&config, GatewayOptions::default()).await {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let err = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect_err("missing token should fail");
        assert!(err.to_string().contains("401"));

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_v1_initialize_uses_json_rpc_envelope_and_capability_result() {
        let config = test_gateway_config();
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(RecordingWebsocketHandler::default())),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let _connected = socket.next().await;

        socket
            .send(Message::Text(
                json!({
                    "id": "init-1",
                    "method": "initialize",
                    "params": {
                        "client_info": {
                            "name": "test-client",
                            "title": "Test Client",
                            "version": "0.1.0"
                        },
                        "capabilities": {
                            "protocol_version": "v1",
                            "schema": true,
                            "turns": true,
                            "items": true
                        }
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("initialize should send");

        let frame = socket
            .next()
            .await
            .expect("initialize response")
            .expect("initialize message");
        let Message::Text(text) = frame else {
            panic!("unexpected initialize frame: {frame:?}");
        };
        let frame = serde_json::from_str::<serde_json::Value>(&text)
            .expect("initialize response should parse");

        assert_eq!(
            frame.get("id").and_then(|value| value.as_str()),
            Some("init-1")
        );
        assert!(frame.get("type").is_none());
        assert_eq!(
            frame
                .pointer("/result/protocol_name")
                .and_then(|value| value.as_str()),
            Some("gateway.websocket.v1")
        );
        assert_eq!(
            frame
                .pointer("/result/capabilities/schema")
                .and_then(|value| value.as_bool()),
            Some(true)
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_v1_session_list_returns_workspace_bootstrap() {
        let config = test_gateway_config();
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(RecordingWebsocketHandler::default())),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let _connected = socket.next().await;

        socket
            .send(Message::Text(
                json!({
                    "id": "sessions-v1",
                    "method": "session/list",
                    "params": {}
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("session/list should send");

        let frame = socket
            .next()
            .await
            .expect("session/list response")
            .expect("session/list message");
        let Message::Text(text) = frame else {
            panic!("unexpected session/list frame: {frame:?}");
        };
        let frame =
            serde_json::from_str::<serde_json::Value>(&text).expect("session/list should parse");
        assert_eq!(
            frame.get("id").and_then(|value| value.as_str()),
            Some("sessions-v1")
        );
        assert!(frame.get("type").is_none());
        assert_eq!(
            frame
                .pointer("/result/sessions/0/session_key")
                .and_then(|value| value.as_str()),
            Some("websocket:newer")
        );
        assert_eq!(
            frame
                .pointer("/result/active_session_key")
                .and_then(|value| value.as_str()),
            Some("websocket:newer")
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_v1_provider_list_returns_runtime_provider_catalog() {
        let config = test_gateway_config();
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(RecordingWebsocketHandler::default())),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let _connected = socket.next().await;

        socket
            .send(Message::Text(
                json!({
                    "id": "providers-v1",
                    "method": "provider/list",
                    "params": {}
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("provider/list should send");

        let frame = socket
            .next()
            .await
            .expect("provider/list response")
            .expect("provider/list message");
        let Message::Text(text) = frame else {
            panic!("unexpected provider/list frame: {frame:?}");
        };
        let frame =
            serde_json::from_str::<serde_json::Value>(&text).expect("provider/list should parse");
        assert_eq!(
            frame.get("id").and_then(|value| value.as_str()),
            Some("providers-v1")
        );
        assert_eq!(
            frame
                .pointer("/result/default_provider")
                .and_then(|value| value.as_str()),
            Some("anthropic")
        );
        assert_eq!(
            frame
                .pointer("/result/providers/0/id")
                .and_then(|value| value.as_str()),
            Some("anthropic")
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_v1_session_create_update_delete_and_subscribe_use_rpc_frames() {
        let config = test_gateway_config();
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(RecordingWebsocketHandler::default())),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let _connected = socket.next().await;

        socket
            .send(Message::Text(
                json!({
                    "id": "create-v1",
                    "method": "session/create",
                    "params": {}
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("session/create should send");
        let create = socket
            .next()
            .await
            .expect("create response")
            .expect("create message");
        let Message::Text(text) = create else {
            panic!("unexpected create frame: {create:?}");
        };
        let create = serde_json::from_str::<serde_json::Value>(&text).expect("create should parse");
        assert_eq!(
            create
                .pointer("/result/session_key")
                .and_then(|value| value.as_str()),
            Some("websocket:created")
        );

        socket
            .send(Message::Text(
                json!({
                    "id": "update-v1",
                    "method": "session/update",
                    "params": {
                        "session_key": "websocket:newer",
                        "title": "Renamed v1"
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("session/update should send");
        let update = socket
            .next()
            .await
            .expect("update response")
            .expect("update message");
        let Message::Text(text) = update else {
            panic!("unexpected update frame: {update:?}");
        };
        let update = serde_json::from_str::<serde_json::Value>(&text).expect("update should parse");
        assert_eq!(
            update
                .pointer("/result/title")
                .and_then(|value| value.as_str()),
            Some("Renamed v1")
        );
        assert_eq!(
            update
                .pointer("/result/updated")
                .and_then(|value| value.as_bool()),
            Some(true)
        );

        socket
            .send(Message::Text(
                json!({
                    "id": "subscribe-v1",
                    "method": "session/subscribe",
                    "params": { "session_key": "websocket:newer" }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("session/subscribe should send");
        let subscribe = socket
            .next()
            .await
            .expect("subscribe response")
            .expect("subscribe message");
        let Message::Text(text) = subscribe else {
            panic!("unexpected subscribe frame: {subscribe:?}");
        };
        let subscribe =
            serde_json::from_str::<serde_json::Value>(&text).expect("subscribe should parse");
        assert_eq!(
            subscribe
                .pointer("/result/session_key")
                .and_then(|value| value.as_str()),
            Some("websocket:newer")
        );
        let subscribed_event = socket
            .next()
            .await
            .expect("subscribed notification")
            .expect("subscribed notification message");
        let Message::Text(text) = subscribed_event else {
            panic!("unexpected subscribed event: {subscribed_event:?}");
        };
        let subscribed_event = serde_json::from_str::<serde_json::Value>(&text)
            .expect("subscribed event should parse");
        assert_eq!(
            subscribed_event
                .get("method")
                .and_then(|value| value.as_str()),
            Some("session/subscribed")
        );

        socket
            .send(Message::Text(
                json!({
                    "id": "delete-v1",
                    "method": "session/delete",
                    "params": { "session_key": "websocket:newer" }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("session/delete should send");
        let delete = socket
            .next()
            .await
            .expect("delete response")
            .expect("delete message");
        let Message::Text(text) = delete else {
            panic!("unexpected delete frame: {delete:?}");
        };
        let delete = serde_json::from_str::<serde_json::Value>(&text).expect("delete should parse");
        assert_eq!(
            delete
                .pointer("/result/deleted")
                .and_then(|value| value.as_bool()),
            Some(true)
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_v1_thread_history_loads_paginated_session_history() {
        let config = test_gateway_config();
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(RecordingWebsocketHandler::default())),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let _connected = socket.next().await;

        socket
            .send(Message::Text(
                json!({
                    "id": "history-v1",
                    "method": "thread/history",
                    "params": {
                        "session_key": "websocket:history",
                        "before_message_id": null,
                        "limit": 30
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("thread/history should send");

        let frame = socket
            .next()
            .await
            .expect("history response")
            .expect("history message");
        let Message::Text(text) = frame else {
            panic!("unexpected history frame: {frame:?}");
        };
        let frame = serde_json::from_str::<serde_json::Value>(&text).expect("history should parse");
        assert_eq!(
            frame
                .pointer("/result/session_key")
                .and_then(|value| value.as_str()),
            Some("websocket:history")
        );
        assert_eq!(
            frame
                .pointer("/result/messages/0/content")
                .and_then(|value| value.as_str()),
            Some("previous answer (30)")
        );
        assert_eq!(
            frame
                .pointer("/result/has_more")
                .and_then(|value| value.as_bool()),
            Some(true)
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_v1_turn_start_links_request_session_thread_turn_and_handler_metadata() {
        let config = test_gateway_config();
        let handler = RecordingWebsocketHandler::default();
        let requests = Arc::clone(&handler.requests);
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(handler)),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let _connected = socket.next().await;

        socket
            .send(Message::Text(
                json!({
                    "id": "turn-req-1",
                    "method": "turn/start",
                    "params": {
                        "session_id": "websocket:v1-session",
                        "thread_id": "thr_v1_session",
                        "turn_id": "turn_client_1",
                        "input": [{ "type": "text", "text": "hello v1" }],
                        "model_provider": "anthropic",
                        "model": "claude-opus-4-1"
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("turn/start should send");

        let first = socket
            .next()
            .await
            .expect("turn/start result")
            .expect("turn/start result message");
        let Message::Text(text) = first else {
            panic!("unexpected turn/start frame: {first:?}");
        };
        let first = serde_json::from_str::<serde_json::Value>(&text)
            .expect("turn/start result should parse");
        assert_eq!(
            first.get("id").and_then(|value| value.as_str()),
            Some("turn-req-1")
        );
        assert_eq!(
            first
                .pointer("/result/turn/turn_id")
                .and_then(|value| value.as_str()),
            Some("turn_client_1")
        );

        let second = socket
            .next()
            .await
            .expect("turn started event")
            .expect("turn started message");
        let Message::Text(text) = second else {
            panic!("unexpected turn event frame: {second:?}");
        };
        let second =
            serde_json::from_str::<serde_json::Value>(&text).expect("turn event should parse");
        assert_eq!(
            second.get("method").and_then(|value| value.as_str()),
            Some("turn/started")
        );
        assert_eq!(
            second
                .pointer("/params/thread_id")
                .and_then(|value| value.as_str()),
            Some("thr_v1_session")
        );
        assert_eq!(
            second
                .pointer("/params/request_id")
                .and_then(|value| value.as_str()),
            Some("turn-req-1")
        );

        tokio::time::sleep(Duration::from_millis(25)).await;
        let recorded = requests.lock().unwrap_or_else(|err| err.into_inner());
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].session_key, "websocket:v1-session");
        assert_eq!(recorded[0].chat_id, "thr_v1_session");
        assert_eq!(recorded[0].request_id, "turn-req-1");
        assert_eq!(recorded[0].input, "hello v1");
        assert_eq!(
            recorded[0].metadata.get("channel.websocket.v1.thread_id"),
            Some(&json!("thr_v1_session"))
        );
        assert_eq!(
            recorded[0].metadata.get("channel.websocket.v1.turn_id"),
            Some(&json!("turn_client_1"))
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_v1_server_request_responses_emit_resolved_notification() {
        let config = test_gateway_config();
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(RecordingWebsocketHandler::default())),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let _connected = socket.next().await;

        socket
            .send(Message::Text(
                json!({
                    "id": "approval-response-1",
                    "method": "approval/respond",
                    "params": {
                        "request_id": "srv_req_1",
                        "thread_id": "thr_1",
                        "turn_id": "turn_1",
                        "decision": "accept"
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("approval response should send");

        let result = socket
            .next()
            .await
            .expect("approval response result")
            .expect("approval response message");
        let Message::Text(text) = result else {
            panic!("unexpected approval result frame: {result:?}");
        };
        let result =
            serde_json::from_str::<serde_json::Value>(&text).expect("approval result should parse");
        assert_eq!(
            result.get("id").and_then(|value| value.as_str()),
            Some("approval-response-1")
        );
        assert_eq!(
            result
                .pointer("/result/resolved/request_id")
                .and_then(|value| value.as_str()),
            Some("srv_req_1")
        );

        let resolved = socket
            .next()
            .await
            .expect("resolved notification")
            .expect("resolved message");
        let Message::Text(text) = resolved else {
            panic!("unexpected resolved frame: {resolved:?}");
        };
        let resolved = serde_json::from_str::<serde_json::Value>(&text)
            .expect("resolved notification should parse");
        assert_eq!(
            resolved.get("method").and_then(|value| value.as_str()),
            Some("serverRequest/resolved")
        );
        assert_eq!(
            resolved
                .pointer("/params/request_id")
                .and_then(|value| value.as_str()),
            Some("srv_req_1")
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_v1_turn_cancel_emits_interrupted_terminal_event() {
        let config = test_gateway_config();
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(RecordingWebsocketHandler::default())),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let _connected = socket.next().await;

        socket
            .send(Message::Text(
                json!({
                    "id": "cancel-1",
                    "method": "turn/cancel",
                    "params": {
                        "session_id": "websocket:v1-session",
                        "thread_id": "thr_v1",
                        "turn_id": "turn_v1"
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("turn/cancel should send");

        let result = socket
            .next()
            .await
            .expect("cancel result")
            .expect("cancel result message");
        let Message::Text(text) = result else {
            panic!("unexpected cancel result frame: {result:?}");
        };
        let result =
            serde_json::from_str::<serde_json::Value>(&text).expect("cancel result should parse");
        assert_eq!(
            result.get("id").and_then(|value| value.as_str()),
            Some("cancel-1")
        );
        assert_eq!(
            result
                .pointer("/result/status")
                .and_then(|value| value.as_str()),
            Some("interrupted")
        );

        let event = socket
            .next()
            .await
            .expect("interrupted event")
            .expect("interrupted message");
        let Message::Text(text) = event else {
            panic!("unexpected interrupted frame: {event:?}");
        };
        let event = serde_json::from_str::<serde_json::Value>(&text)
            .expect("interrupted event should parse");
        assert_eq!(
            event.get("method").and_then(|value| value.as_str()),
            Some("turn/interrupted")
        );
        assert_eq!(
            event
                .pointer("/params/turn_id")
                .and_then(|value| value.as_str()),
            Some("turn_v1")
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_rejects_text_frames_over_protocol_payload_limit() {
        let config = test_gateway_config();
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(RecordingWebsocketHandler::default())),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let _connected = socket.next().await;

        socket
            .send(Message::Text(
                "x".repeat(crate::GATEWAY_WEBSOCKET_MAX_TEXT_FRAME_BYTES + 1)
                    .into(),
            ))
            .await
            .expect("oversized text frame should send");

        let frame = socket
            .next()
            .await
            .expect("oversized frame response")
            .expect("oversized response message");
        let Message::Text(text) = frame else {
            panic!("unexpected oversized response frame: {frame:?}");
        };
        let frame = serde_json::from_str::<serde_json::Value>(&text)
            .expect("oversized response should parse");
        assert_eq!(
            frame
                .pointer("/error/code")
                .and_then(|value| value.as_str()),
            Some("payload_too_large")
        );
        assert_eq!(
            frame
                .pointer("/error/data/max_bytes")
                .and_then(serde_json::Value::as_u64),
            Some(crate::GATEWAY_WEBSOCKET_MAX_TEXT_FRAME_BYTES as u64)
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_submit_routes_structured_request_to_handler() {
        let config = test_gateway_config();
        let handler = RecordingWebsocketHandler::default();
        let requests = Arc::clone(&handler.requests);
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(handler)),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let connected = socket
            .next()
            .await
            .expect("connected frame")
            .expect("connected message");
        let connected = match connected {
            Message::Text(text) => serde_json::from_str::<GatewayWebsocketServerFrame>(&text)
                .expect("valid connected frame"),
            other => panic!("unexpected frame: {other:?}"),
        };
        match connected {
            GatewayWebsocketServerFrame::Event { event, .. } => {
                assert_eq!(event, OutboundEvent::SessionConnected);
            }
            other => panic!("unexpected connected frame: {other:?}"),
        }

        socket
            .send(Message::Text(
                json!({
                    "type": "method",
                    "id": "sub-1",
                    "method": "session.subscribe",
                    "params": { "session_key": "websocket:test-session" }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("subscribe should send");

        for _ in 0..2 {
            let _ = timeout(Duration::from_millis(250), socket.next())
                .await
                .expect("subscribe ack frame should arrive");
        }

        socket
            .send(Message::Text(
                json!({
                    "type": "method",
                    "id": "req-1",
                    "method": "session.submit",
                    "params": {
                        "input": "hello gateway",
                        "channel_id": "default",
                        "model_provider": "anthropic",
                        "model": "claude-opus-4-1"
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("submit should send");

        let result = socket
            .next()
            .await
            .expect("submit response")
            .expect("submit message");
        let result = match result {
            Message::Text(text) => serde_json::from_str::<GatewayWebsocketServerFrame>(&text)
                .expect("valid result frame"),
            other => panic!("unexpected frame: {other:?}"),
        };
        match result {
            GatewayWebsocketServerFrame::Result { id, result } => {
                assert_eq!(id, "req-1");
                assert_eq!(
                    result
                        .get("response")
                        .and_then(|response| response.get("content"))
                        .and_then(|value| value.as_str()),
                    Some("ack: hello gateway")
                );
            }
            other => panic!("unexpected submit frame: {other:?}"),
        }

        let recorded = requests.lock().unwrap_or_else(|err| err.into_inner());
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].session_key, "websocket:test-session");
        assert_eq!(recorded[0].chat_id, "websocket:test-session");
        assert_eq!(recorded[0].channel_id, "default");
        assert_eq!(recorded[0].input, "hello gateway");
        assert_eq!(
            recorded[0].metadata.get("channel.websocket.model_provider"),
            Some(&json!("anthropic"))
        );
        assert_eq!(
            recorded[0].metadata.get("channel.websocket.model"),
            Some(&json!("claude-opus-4-1"))
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_submit_accepts_attachment_only_payload() {
        let config = test_gateway_config();
        let handler = RecordingWebsocketHandler::default();
        let requests = Arc::clone(&handler.requests);
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(handler)),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let _ = socket.next().await;

        socket
            .send(Message::Text(
                json!({
                    "type": "method",
                    "id": "sub-attachments",
                    "method": "session.subscribe",
                    "params": { "session_key": "websocket:attachments-only" }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("subscribe should send");

        for _ in 0..2 {
            let _ = timeout(Duration::from_millis(250), socket.next())
                .await
                .expect("subscribe ack frame should arrive");
        }

        socket
            .send(Message::Text(
                json!({
                    "type": "method",
                    "id": "req-attachments-only",
                    "method": "session.submit",
                    "params": {
                        "input": "",
                        "attachments": [{
                            "archive_id": "archive-1",
                            "filename": "report.pdf",
                            "mime_type": "application/pdf",
                            "size_bytes": 42
                        }]
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("submit should send");

        let result = socket
            .next()
            .await
            .expect("submit response")
            .expect("submit message");
        let result = match result {
            Message::Text(text) => serde_json::from_str::<GatewayWebsocketServerFrame>(&text)
                .expect("valid result frame"),
            other => panic!("unexpected frame: {other:?}"),
        };
        match result {
            GatewayWebsocketServerFrame::Result { id, .. } => {
                assert_eq!(id, "req-attachments-only");
            }
            other => panic!("unexpected submit frame: {other:?}"),
        }

        let recorded = requests.lock().unwrap_or_else(|err| err.into_inner());
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].session_key, "websocket:attachments-only");
        assert_eq!(recorded[0].input, "");
        assert_eq!(recorded[0].attachments.len(), 1);
        assert_eq!(recorded[0].attachments[0].archive_id, "archive-1");

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_workspace_bootstrap_returns_sessions_sorted_by_created_at_desc() {
        let config = test_gateway_config();
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(RecordingWebsocketHandler::default())),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let _ = socket.next().await;

        socket
            .send(Message::Text(
                json!({
                    "type": "method",
                    "id": "bootstrap-1",
                    "method": "workspace.bootstrap",
                    "params": {}
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("bootstrap should send");

        let frame = socket
            .next()
            .await
            .expect("bootstrap response")
            .expect("bootstrap frame");
        let frame = match frame {
            Message::Text(text) => serde_json::from_str::<GatewayWebsocketServerFrame>(&text)
                .expect("valid result frame"),
            other => panic!("unexpected frame: {other:?}"),
        };

        match frame {
            GatewayWebsocketServerFrame::Result { id, result } => {
                assert_eq!(id, "bootstrap-1");
                let sessions = result
                    .get("sessions")
                    .and_then(|value| value.as_array())
                    .expect("sessions array");
                assert_eq!(sessions.len(), 2);
                assert_eq!(
                    sessions[0]
                        .get("session_key")
                        .and_then(|value| value.as_str()),
                    Some("websocket:newer")
                );
                assert_eq!(
                    sessions[1]
                        .get("session_key")
                        .and_then(|value| value.as_str()),
                    Some("websocket:older")
                );
                assert_eq!(
                    result
                        .get("active_session_key")
                        .and_then(|value| value.as_str()),
                    Some("websocket:newer")
                );
                assert_eq!(
                    sessions[0]
                        .get("model_provider")
                        .and_then(|value| value.as_str()),
                    Some("anthropic")
                );
                assert_eq!(
                    sessions[0].get("model").and_then(|value| value.as_str()),
                    Some("claude-sonnet-4-5")
                );
            }
            other => panic!("unexpected bootstrap frame: {other:?}"),
        }

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_session_create_returns_created_session_metadata() {
        let config = test_gateway_config();
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(RecordingWebsocketHandler::default())),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let _ = socket.next().await;

        socket
            .send(Message::Text(
                json!({
                    "type": "method",
                    "id": "create-1",
                    "method": "session.create",
                    "params": {}
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("create should send");

        let frame = socket
            .next()
            .await
            .expect("create response")
            .expect("create frame");
        let frame = match frame {
            Message::Text(text) => serde_json::from_str::<GatewayWebsocketServerFrame>(&text)
                .expect("valid result frame"),
            other => panic!("unexpected frame: {other:?}"),
        };

        match frame {
            GatewayWebsocketServerFrame::Result { id, result } => {
                assert_eq!(id, "create-1");
                assert_eq!(
                    result.get("session_key").and_then(|value| value.as_str()),
                    Some("websocket:created")
                );
                assert_eq!(
                    result.get("title").and_then(|value| value.as_str()),
                    Some("Agent 3")
                );
                assert_eq!(
                    result.get("created_at_ms").and_then(|value| value.as_i64()),
                    Some(30)
                );
                assert_eq!(
                    result
                        .get("model_provider")
                        .and_then(|value| value.as_str()),
                    Some("openai")
                );
                assert_eq!(
                    result.get("model").and_then(|value| value.as_str()),
                    Some("gpt-4.1-mini")
                );
            }
            other => panic!("unexpected create frame: {other:?}"),
        }

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_session_update_returns_updated_session_metadata() {
        let config = test_gateway_config();
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(RecordingWebsocketHandler::default())),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let url = ws_url(handle.info().actual_port, None);
        let (mut socket, _) = connect_async(url).await.expect("websocket should connect");
        let _connected = socket
            .next()
            .await
            .expect("connected frame")
            .expect("frame ok");

        socket
            .send(Message::Text(
                json!({
                    "type": "method",
                    "id": "update-1",
                    "method": "session.update",
                    "params": {
                        "session_key": "websocket:newer",
                        "title": "Renamed agent"
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("update should send");

        let frame = socket
            .next()
            .await
            .expect("update result frame")
            .expect("frame should decode");
        let payload = match frame {
            Message::Text(text) => serde_json::from_str::<serde_json::Value>(&text)
                .expect("frame payload should parse"),
            other => panic!("unexpected websocket frame: {other:?}"),
        };
        assert_eq!(
            payload.get("type").and_then(|value| value.as_str()),
            Some("result")
        );
        let result = payload.get("result").expect("result payload");
        assert_eq!(
            result.get("session_key").and_then(|value| value.as_str()),
            Some("websocket:newer")
        );
        assert_eq!(
            result.get("title").and_then(|value| value.as_str()),
            Some("Renamed agent")
        );
        assert_eq!(
            result.get("updated").and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            result
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("anthropic")
        );
        assert_eq!(
            result.get("model").and_then(|value| value.as_str()),
            Some("claude-sonnet-4-5")
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_provider_list_returns_runtime_provider_catalog() {
        let config = test_gateway_config();
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(RecordingWebsocketHandler::default())),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let _ = socket.next().await;

        socket
            .send(Message::Text(
                json!({
                    "type": "method",
                    "id": "providers-1",
                    "method": "provider.list",
                    "params": {}
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("provider.list should send");

        let frame = socket
            .next()
            .await
            .expect("provider.list response")
            .expect("provider.list frame");
        let frame = match frame {
            Message::Text(text) => serde_json::from_str::<GatewayWebsocketServerFrame>(&text)
                .expect("valid provider.list frame"),
            other => panic!("unexpected frame: {other:?}"),
        };

        match frame {
            GatewayWebsocketServerFrame::Result { id, result } => {
                assert_eq!(id, "providers-1");
                assert_eq!(
                    result
                        .get("default_provider")
                        .and_then(|value| value.as_str()),
                    Some("anthropic")
                );
                let providers = result
                    .get("providers")
                    .and_then(|value| value.as_array())
                    .expect("providers array");
                assert_eq!(providers.len(), 2);
                assert_eq!(
                    providers[0].get("id").and_then(|value| value.as_str()),
                    Some("anthropic")
                );
                assert_eq!(
                    providers[0]
                        .get("default_model")
                        .and_then(|value| value.as_str()),
                    Some("claude-sonnet-4-5")
                );
            }
            other => panic!("unexpected provider.list frame: {other:?}"),
        }

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_session_delete_returns_deleted_flag() {
        let config = test_gateway_config();
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(RecordingWebsocketHandler::default())),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let url = ws_url(handle.info().actual_port, None);
        let (mut socket, _) = connect_async(url).await.expect("websocket should connect");
        let _connected = socket
            .next()
            .await
            .expect("connected frame")
            .expect("frame ok");

        socket
            .send(Message::Text(
                json!({
                    "type": "method",
                    "id": "delete-1",
                    "method": "session.delete",
                    "params": {
                        "session_key": "websocket:newer"
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("delete should send");

        let frame = socket
            .next()
            .await
            .expect("delete result frame")
            .expect("frame should decode");
        let payload = match frame {
            Message::Text(text) => serde_json::from_str::<serde_json::Value>(&text)
                .expect("frame payload should parse"),
            other => panic!("unexpected websocket frame: {other:?}"),
        };
        assert_eq!(
            payload.get("type").and_then(|value| value.as_str()),
            Some("result")
        );
        let result = payload.get("result").expect("result payload");
        assert_eq!(
            result.get("session_key").and_then(|value| value.as_str()),
            Some("websocket:newer")
        );
        assert_eq!(
            result.get("deleted").and_then(|value| value.as_bool()),
            Some(true)
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_subscribe_only_acknowledges_realtime_subscription() {
        let config = test_gateway_config();
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(RecordingWebsocketHandler::default())),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let _ = socket.next().await;

        socket
            .send(Message::Text(
                json!({
                    "type": "method",
                    "id": "sub-history-1",
                    "method": "session.subscribe",
                    "params": { "session_key": "websocket:history" }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("subscribe should send");

        let first = socket
            .next()
            .await
            .expect("subscribe result")
            .expect("subscribe result frame");
        let second = socket
            .next()
            .await
            .expect("subscribed event")
            .expect("subscribed event frame");
        let first = match first {
            Message::Text(text) => serde_json::from_str::<GatewayWebsocketServerFrame>(&text)
                .expect("valid result frame"),
            other => panic!("unexpected frame: {other:?}"),
        };
        let second = match second {
            Message::Text(text) => serde_json::from_str::<GatewayWebsocketServerFrame>(&text)
                .expect("valid event frame"),
            other => panic!("unexpected frame: {other:?}"),
        };

        match first {
            GatewayWebsocketServerFrame::Result { id, .. } => {
                assert_eq!(id, "sub-history-1");
            }
            other => panic!("unexpected subscribe result: {other:?}"),
        }
        match second {
            GatewayWebsocketServerFrame::Event { event, payload } => {
                assert_eq!(event, OutboundEvent::SessionSubscribed);
                assert_eq!(
                    payload.get("session_key").and_then(|value| value.as_str()),
                    Some("websocket:history")
                );
            }
            other => panic!("unexpected subscribe event: {other:?}"),
        }
        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_history_load_returns_paginated_result() {
        let config = test_gateway_config();
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_handler: Some(Arc::new(RecordingWebsocketHandler::default())),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let _ = socket.next().await;

        socket
            .send(Message::Text(
                json!({
                    "type": "method",
                    "id": "history-1",
                    "method": "session.history.load",
                    "params": {
                        "session_key": "websocket:history",
                        "limit": 10
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("history.load should send");

        let frame = timeout(Duration::from_millis(250), socket.next())
            .await
            .expect("history result should arrive before timeout")
            .expect("history result")
            .expect("history result frame");
        let frame = match frame {
            Message::Text(text) => serde_json::from_str::<GatewayWebsocketServerFrame>(&text)
                .expect("valid result frame"),
            other => panic!("unexpected frame: {other:?}"),
        };
        match frame {
            GatewayWebsocketServerFrame::Result { id, result } => {
                assert_eq!(id, "history-1");
                assert_eq!(
                    result.get("session_key").and_then(|value| value.as_str()),
                    Some("websocket:history")
                );
                assert_eq!(
                    result.get("has_more").and_then(|value| value.as_bool()),
                    Some(true)
                );
                assert_eq!(
                    result
                        .get("oldest_loaded_message_id")
                        .and_then(|value| value.as_str()),
                    Some("msg-2")
                );
                assert_eq!(
                    result
                        .get("messages")
                        .and_then(|value| value.as_array())
                        .and_then(|messages| messages.first())
                        .and_then(|message| message.get("content"))
                        .and_then(|value| value.as_str()),
                    Some("previous answer (10)")
                );
            }
            other => panic!("unexpected history result: {other:?}"),
        }

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_connection_keeps_realtime_delivery_for_all_subscribed_sessions() {
        let config = test_gateway_config();
        let broadcaster = Arc::new(crate::state::GatewayWebsocketBroadcaster::new());
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                websocket_broadcaster: Some(Arc::clone(&broadcaster)),
                websocket_handler: Some(Arc::new(RecordingWebsocketHandler::default())),
                ..GatewayOptions::default()
            },
        )
        .await
        {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let _ = socket.next().await;

        for (request_id, session_key) in [("sub-a", "websocket:alpha"), ("sub-b", "websocket:beta")]
        {
            socket
                .send(Message::Text(
                    json!({
                        "type": "method",
                        "id": request_id,
                        "method": "session.subscribe",
                        "params": { "session_key": session_key }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .expect("subscribe should send");

            for _ in 0..2 {
                let _ = timeout(Duration::from_millis(250), socket.next())
                    .await
                    .expect("subscribe response should arrive before timeout")
                    .expect("subscribe frame should exist");
            }
        }

        let delivered = broadcaster
            .broadcast_to_session(
                "websocket:alpha",
                GatewayWebsocketServerFrame::Event {
                    event: OutboundEvent::SessionMessage,
                    payload: json!({
                        "session_key": "websocket:alpha",
                        "response": {
                            "content": "background alpha reply",
                        },
                        "role": "assistant",
                        "timestamp_ms": 99,
                    }),
                },
            )
            .await;
        assert_eq!(delivered, 1);

        let frame = timeout(Duration::from_millis(250), socket.next())
            .await
            .expect("realtime alpha event should arrive before timeout")
            .expect("realtime alpha event")
            .expect("realtime alpha frame");
        let frame = match frame {
            Message::Text(text) => serde_json::from_str::<GatewayWebsocketServerFrame>(&text)
                .expect("valid websocket event frame"),
            other => panic!("unexpected frame: {other:?}"),
        };
        match frame {
            GatewayWebsocketServerFrame::Event { event, payload } => {
                assert_eq!(event, OutboundEvent::SessionMessage);
                assert_eq!(
                    payload.get("session_key").and_then(|value| value.as_str()),
                    Some("websocket:alpha")
                );
                assert_eq!(
                    payload
                        .get("response")
                        .and_then(|response| response.get("content"))
                        .and_then(|value| value.as_str()),
                    Some("background alpha reply")
                );
            }
            other => panic!("unexpected realtime frame: {other:?}"),
        }

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_unknown_method_returns_structured_error() {
        let config = test_gateway_config();
        let handle = match spawn_gateway(&config).await {
            Ok(handle) => handle,
            Err(crate::GatewayError::Bind(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(err) => panic!("gateway should start: {err}"),
        };

        let (mut socket, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("websocket should connect");
        let _ = socket.next().await;
        socket
            .send(Message::Text(
                json!({
                    "type": "method",
                    "id": "bad-1",
                    "method": "session.unknown",
                    "params": {}
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("bad method should send");

        let frame = socket
            .next()
            .await
            .expect("error response")
            .expect("error frame");
        let frame = match frame {
            Message::Text(text) => serde_json::from_str::<GatewayWebsocketServerFrame>(&text)
                .expect("valid error frame"),
            other => panic!("unexpected frame: {other:?}"),
        };
        match frame {
            GatewayWebsocketServerFrame::Error { id, error } => {
                assert_eq!(id.as_deref(), Some("bad-1"));
                assert_eq!(error.code, "unknown_method");
            }
            other => panic!("unexpected error frame: {other:?}"),
        }

        handle.shutdown().await.expect("gateway should stop");
    }
}
