#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use crate::{
        GatewayMcpHandler, GatewayMcpHandlerError, GatewayMcpRuntimeSnapshot,
        GatewayMcpServerConfigView, GatewayMcpServerDetailView, GatewayMcpServerStatusView,
        GatewayMcpServerUpsertRequest, GatewayOptions, GatewayProviderCatalog,
        GatewayProviderEntry, GatewaySessionHistoryMessage, GatewaySessionHistoryPage,
        GatewayWebsocketHandler, GatewayWebsocketHandlerError, GatewayWebsocketServerFrame,
        GatewayWebsocketSubmitRequest, GatewayWorkspaceBootstrap, GatewayWorkspaceSession, Route,
        protocol::{GatewayProtocolMethod, GatewayRpcMessage},
        spawn_gateway, spawn_gateway_with_options,
        webhook::{
            GatewayWebhookAgentQuery, GatewayWebhookPayload, normalize_webhook_agent_request,
            normalize_webhook_request,
        },
    };
    use async_trait::async_trait;
    use futures_util::{SinkExt, StreamExt};
    use klaw_config::{GatewayAuthConfig, GatewayConfig, McpServerMode};
    use reqwest::StatusCode;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tokio::time::timeout;
    use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

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
            frame_tx: mpsc::Sender<GatewayWebsocketServerFrame>,
        ) -> Result<(), GatewayWebsocketHandlerError> {
            self.requests
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(request.clone());
            if request.input == "__fail__" {
                return Err(GatewayWebsocketHandlerError::internal(
                    "simulated submit failure",
                ));
            }
            if request.input == "__hold__" {
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
            if request.input == "__stream__" {
                frame_tx
                    .try_send(GatewayWebsocketServerFrame::Protocol(
                        GatewayRpcMessage::notification(
                            GatewayProtocolMethod::ItemAgentMessageDelta,
                            json!({
                                "session_id": request.session_key,
                                "thread_id": request.chat_id,
                                "turn_id": request
                                    .metadata
                                    .get("channel.websocket.v1.turn_id")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("turn_test"),
                                "item_id": "item_agent_test",
                                "delta": "streamed",
                            }),
                        ),
                    ))
                    .map_err(|_| GatewayWebsocketHandlerError::internal("connection closed"))?;
            }
            frame_tx
                .try_send(GatewayWebsocketServerFrame::Protocol(
                    GatewayRpcMessage::notification(
                        GatewayProtocolMethod::TurnCompleted,
                        json!({
                            "session_id": request.session_key,
                            "thread_id": request.chat_id,
                            "turn_id": request
                                .metadata
                                .get("channel.websocket.v1.turn_id")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("turn_test"),
                            "request_id": request.request_id,
                            "status": "completed",
                            "response": {
                                "content": format!("ack: {}", request.input),
                            },
                        }),
                    ),
                ))
                .map_err(|_| GatewayWebsocketHandlerError::internal("connection closed"))?;
            Ok(())
        }
    }

    #[derive(Clone, Default)]
    struct RecordingMcpHandler {
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingMcpHandler {
        fn runtime() -> GatewayMcpRuntimeSnapshot {
            GatewayMcpRuntimeSnapshot {
                statuses: vec![GatewayMcpServerStatusView {
                    id: "local".to_string(),
                    mode: McpServerMode::Stdio,
                    enabled: true,
                    state: "running".to_string(),
                    last_error: None,
                    tool_count: 1,
                }],
                details: vec![GatewayMcpServerDetailView {
                    id: "local".to_string(),
                    tools_list_response: Some(json!({
                        "tools": [{
                            "name": "echo",
                            "description": "Echo input",
                            "inputSchema": {"type": "object"}
                        }]
                    })),
                }],
            }
        }

        fn server() -> GatewayMcpServerConfigView {
            GatewayMcpServerConfigView {
                id: "local".to_string(),
                enabled: true,
                mode: McpServerMode::Stdio,
                tool_timeout_seconds: 60,
                command: Some("npx".to_string()),
                args: vec!["server".to_string()],
                cwd: None,
                url: None,
                env_keys: vec!["API_KEY".to_string()],
                header_keys: vec!["Authorization".to_string()],
            }
        }
    }

    #[async_trait]
    impl GatewayMcpHandler for RecordingMcpHandler {
        async fn status(&self) -> Result<GatewayMcpRuntimeSnapshot, GatewayMcpHandlerError> {
            self.calls
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push("status".to_string());
            Ok(Self::runtime())
        }

        async fn list_servers(
            &self,
        ) -> Result<
            (Vec<GatewayMcpServerConfigView>, GatewayMcpRuntimeSnapshot),
            GatewayMcpHandlerError,
        > {
            self.calls
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push("list".to_string());
            Ok((vec![Self::server()], Self::runtime()))
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
            self.calls
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(format!("get:{id}"));
            Ok((
                Self::server(),
                Self::runtime().statuses.into_iter().next(),
                Self::runtime().details.into_iter().next(),
            ))
        }

        async fn create_server(
            &self,
            _request: GatewayMcpServerUpsertRequest,
        ) -> Result<(GatewayMcpServerConfigView, GatewayMcpRuntimeSnapshot), GatewayMcpHandlerError>
        {
            self.calls
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push("create".to_string());
            Ok((Self::server(), Self::runtime()))
        }

        async fn update_server(
            &self,
            id: String,
            _request: GatewayMcpServerUpsertRequest,
        ) -> Result<(GatewayMcpServerConfigView, GatewayMcpRuntimeSnapshot), GatewayMcpHandlerError>
        {
            self.calls
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(format!("update:{id}"));
            Ok((Self::server(), Self::runtime()))
        }

        async fn delete_server(
            &self,
            id: String,
        ) -> Result<GatewayMcpRuntimeSnapshot, GatewayMcpHandlerError> {
            self.calls
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(format!("delete:{id}"));
            Ok(Self::runtime())
        }

        async fn sync(&self) -> Result<GatewayMcpRuntimeSnapshot, GatewayMcpHandlerError> {
            self.calls
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push("sync".to_string());
            Ok(Self::runtime())
        }

        async fn restart_server(
            &self,
            id: String,
        ) -> Result<GatewayMcpRuntimeSnapshot, GatewayMcpHandlerError> {
            self.calls
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(format!("restart:{id}"));
            Ok(Self::runtime())
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

    fn is_user_message_item(frame: &serde_json::Value, content: &str) -> bool {
        frame.get("method").and_then(|value| value.as_str()) == Some("item/completed")
            && frame
                .pointer("/params/item/type")
                .and_then(|value| value.as_str())
                == Some("userMessage")
            && frame
                .pointer("/params/item/payload/message/content")
                .and_then(|value| value.as_str())
                == Some(content)
    }

    type TestWebSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

    async fn next_json_frame(socket: &mut TestWebSocket, label: &str) -> serde_json::Value {
        let frame = socket
            .next()
            .await
            .unwrap_or_else(|| panic!("{label} frame"))
            .unwrap_or_else(|err| panic!("{label} message: {err}"));
        let Message::Text(text) = frame else {
            panic!("unexpected {label} frame: {frame:?}");
        };
        serde_json::from_str::<serde_json::Value>(&text)
            .unwrap_or_else(|err| panic!("{label} should parse: {err}"))
    }

    async fn next_json_frame_matching(
        socket: &mut TestWebSocket,
        label: &str,
        matches: impl Fn(&serde_json::Value) -> bool,
    ) -> serde_json::Value {
        for _ in 0..8 {
            let frame = next_json_frame(socket, label).await;
            if matches(&frame) {
                return frame;
            }
        }
        panic!("did not receive expected {label} frame");
    }

    #[tokio::test]
    async fn gateway_docs_routes_expose_openapi_and_scalar_without_auth() {
        let config = test_gateway_config();
        let handle = match spawn_gateway_with_options(&config, GatewayOptions::default()).await {
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

        let openapi: serde_json::Value = client
            .get(format!("{base_url}{}", Route::OpenApiJson.as_str()))
            .send()
            .await
            .expect("openapi should respond")
            .json()
            .await
            .expect("openapi json");
        assert_eq!(
            openapi.pointer("/openapi").and_then(|value| value.as_str()),
            Some("3.1.0")
        );
        for path in [
            "/mcp/status",
            "/mcp/servers",
            "/archive/upload",
            "/providers/list",
            "/webhook/events",
            "/health/status",
        ] {
            assert!(
                openapi
                    .pointer(&format!("/paths/{}", path.replace('/', "~1")))
                    .is_some(),
                "OpenAPI document should include {path}"
            );
        }

        let scalar = client
            .get(format!("{base_url}{}", Route::Scalar.as_str()))
            .send()
            .await
            .expect("scalar should respond");
        assert_eq!(scalar.status(), StatusCode::OK);
        let content_type = scalar
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let html = scalar.text().await.expect("scalar html");
        assert!(content_type.starts_with("text/html"));
        assert!(html.contains("Klaw Gateway API"));
        assert!(html.contains("/openapi.json"));

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn gateway_docs_routes_remain_public_when_gateway_auth_enabled() {
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

        let base_url = format!("http://127.0.0.1:{}", handle.info().actual_port);
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("reqwest client");

        for route in [Route::OpenApiJson, Route::Scalar] {
            let public = client
                .get(format!("{base_url}{}", route.as_str()))
                .send()
                .await
                .expect("docs route should respond");
            assert_eq!(public.status(), StatusCode::OK);

            let authorized = client
                .get(format!("{base_url}{}", route.as_str()))
                .bearer_auth("secret-token")
                .send()
                .await
                .expect("docs route should respond with auth");
            assert_eq!(authorized.status(), StatusCode::OK);
        }

        handle.shutdown().await.expect("gateway should stop");
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
    async fn spawn_gateway_rejects_enabled_auth_without_token() {
        let mut config = test_gateway_config();
        config.auth = GatewayAuthConfig {
            enabled: true,
            token: None,
            env_key: None,
        };

        let err = match spawn_gateway(&config).await {
            Ok(handle) => {
                let _ = handle.shutdown().await;
                panic!("gateway should reject missing auth token");
            }
            Err(err) => err,
        };

        assert!(matches!(err, crate::GatewayError::MissingAuthToken));
    }

    #[tokio::test]
    async fn spawn_gateway_accepts_enabled_auth_with_token() {
        let mut config = test_gateway_config();
        config.auth = GatewayAuthConfig {
            enabled: true,
            token: Some("secret-token".to_string()),
            env_key: None,
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

        assert!(handle.info().auth_configured);
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

    #[tokio::test]
    async fn gateway_mcp_routes_require_gateway_auth_when_enabled() {
        let mut config = test_gateway_config();
        config.auth = GatewayAuthConfig {
            enabled: true,
            token: Some("secret-token".to_string()),
            env_key: None,
        };
        let handler = Arc::new(RecordingMcpHandler::default());
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                mcp_handler: Some(handler),
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

        let base_url = format!("http://127.0.0.1:{}", handle.info().actual_port);
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("reqwest client");

        let unauthorized = client
            .get(format!("{base_url}{}", Route::McpStatus.as_str()))
            .send()
            .await
            .expect("mcp status should respond");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let authorized = client
            .get(format!("{base_url}{}", Route::McpStatus.as_str()))
            .bearer_auth("secret-token")
            .send()
            .await
            .expect("mcp status should respond with auth");
        assert_eq!(authorized.status(), StatusCode::OK);
        let body: serde_json::Value = authorized.json().await.expect("json body");
        assert_eq!(
            body.pointer("/success").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            body.pointer("/runtime/statuses/0/id")
                .and_then(|v| v.as_str()),
            Some("local")
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn gateway_mcp_routes_expose_redacted_configs_and_actions() {
        let config = test_gateway_config();
        let handler = Arc::new(RecordingMcpHandler::default());
        let calls = Arc::clone(&handler.calls);
        let handle = match spawn_gateway_with_options(
            &config,
            GatewayOptions {
                mcp_handler: Some(handler),
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

        let base_url = format!("http://127.0.0.1:{}", handle.info().actual_port);
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("reqwest client");

        let list: serde_json::Value = client
            .get(format!("{base_url}{}", Route::McpServers.as_str()))
            .send()
            .await
            .expect("mcp servers should respond")
            .json()
            .await
            .expect("json body");
        assert_eq!(
            list.pointer("/success").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            list.pointer("/servers/0/env_keys/0")
                .and_then(|v| v.as_str()),
            Some("API_KEY")
        );
        assert!(list.pointer("/servers/0/env/API_KEY").is_none());

        let create: serde_json::Value = client
            .post(format!("{base_url}{}", Route::McpServers.as_str()))
            .json(&json!({
                "id": "local",
                "mode": "stdio",
                "command": "npx",
                "env": {"API_KEY": "secret"}
            }))
            .send()
            .await
            .expect("mcp create should respond")
            .json()
            .await
            .expect("json body");
        assert_eq!(
            create.pointer("/success").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert!(create.pointer("/server/env/API_KEY").is_none());

        let invalid_payload: serde_json::Value = client
            .post(format!("{base_url}{}", Route::McpServers.as_str()))
            .json(&json!({"id": "missing-mode"}))
            .send()
            .await
            .expect("mcp invalid create should respond")
            .json()
            .await
            .expect("json body");
        assert_eq!(
            invalid_payload
                .pointer("/success")
                .and_then(|v| v.as_bool()),
            Some(false)
        );
        assert!(invalid_payload.pointer("/error").is_some());

        let restart = client
            .post(format!(
                "{base_url}{}",
                Route::McpServerRestart.as_str().replace("{id}", "local")
            ))
            .send()
            .await
            .expect("mcp restart should respond");
        assert_eq!(restart.status(), StatusCode::OK);

        let calls = calls.lock().unwrap_or_else(|err| err.into_inner()).clone();
        assert!(calls.iter().any(|call| call == "list"));
        assert!(calls.iter().any(|call| call == "create"));
        assert!(calls.iter().any(|call| call == "restart:local"));

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
        assert_eq!(Route::McpStatus.as_str(), "/mcp/status");
        assert_eq!(Route::McpServers.as_str(), "/mcp/servers");
        assert_eq!(Route::McpServer.as_str(), "/mcp/servers/{id}");
        assert_eq!(Route::McpSync.as_str(), "/mcp/sync");
        assert_eq!(
            Route::McpServerRestart.as_str(),
            "/mcp/servers/{id}/restart"
        );
        assert_eq!(Route::OpenApiJson.as_str(), "/openapi.json");
        assert_eq!(Route::Scalar.as_str(), "/scalar");
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

    #[allow(clippy::await_holding_lock)]
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

        let second = next_json_frame_matching(&mut socket, "turn started event", |frame| {
            frame.get("method").and_then(|value| value.as_str()) == Some("turn/started")
        })
        .await;
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
        drop(recorded);

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_v1_turn_frames_reach_all_subscribed_connections() {
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

        let (mut socket_a, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("first websocket should connect");
        let (mut socket_b, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("second websocket should connect");

        for socket in [&mut socket_a, &mut socket_b] {
            socket
                .send(Message::Text(
                    json!({
                        "id": "subscribe-shared",
                        "method": "session/subscribe",
                        "params": { "session_key": "websocket:shared" }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .expect("session/subscribe should send");
            for _ in 0..2 {
                let _ = socket
                    .next()
                    .await
                    .expect("subscribe response frame")
                    .expect("subscribe response message");
            }
        }

        socket_a
            .send(Message::Text(
                json!({
                    "id": "turn-shared-1",
                    "method": "turn/start",
                    "params": {
                        "session_id": "websocket:shared",
                        "thread_id": "thread_shared",
                        "turn_id": "turn_shared",
                        "input": [{ "type": "text", "text": "__stream__" }]
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("turn/start should send");

        let first = socket_a
            .next()
            .await
            .expect("turn/start result")
            .expect("turn/start result message");
        let Message::Text(text) = first else {
            panic!("unexpected turn/start result frame: {first:?}");
        };
        let result =
            serde_json::from_str::<serde_json::Value>(&text).expect("turn/start result parses");
        assert_eq!(
            result.get("id").and_then(|value| value.as_str()),
            Some("turn-shared-1")
        );

        let mut saw_delta_a = false;
        let mut saw_started_b = false;
        let mut saw_delta_b = false;
        for _ in 0..3 {
            let frame_a = timeout(Duration::from_secs(1), socket_a.next())
                .await
                .expect("first connection should receive lifecycle frame")
                .expect("first connection lifecycle frame")
                .expect("first connection lifecycle message");
            if let Message::Text(text) = frame_a {
                let frame = serde_json::from_str::<serde_json::Value>(&text)
                    .expect("first connection frame parses");
                saw_delta_a |= frame.get("method").and_then(|value| value.as_str())
                    == Some("item/agentMessage/delta")
                    && frame
                        .pointer("/params/delta")
                        .and_then(|value| value.as_str())
                        == Some("streamed");
            }

            let frame_b = timeout(Duration::from_secs(1), socket_b.next())
                .await
                .expect("second connection should receive lifecycle frame")
                .expect("second connection lifecycle frame")
                .expect("second connection lifecycle message");
            if let Message::Text(text) = frame_b {
                let frame = serde_json::from_str::<serde_json::Value>(&text)
                    .expect("second connection frame parses");
                saw_started_b |= frame.get("method").and_then(|value| value.as_str())
                    == Some("turn/started")
                    && frame
                        .pointer("/params/turn_id")
                        .and_then(|value| value.as_str())
                        == Some("turn_shared");
                saw_delta_b |= frame.get("method").and_then(|value| value.as_str())
                    == Some("item/agentMessage/delta")
                    && frame
                        .pointer("/params/delta")
                        .and_then(|value| value.as_str())
                        == Some("streamed");
            }
        }

        assert!(
            saw_delta_a,
            "initiating connection should receive stream delta"
        );
        assert!(saw_started_b, "subscribed peer should receive turn/started");
        assert!(saw_delta_b, "subscribed peer should receive stream delta");

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_v1_user_message_reaches_all_subscribed_connections() {
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

        let (mut socket_a, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("first websocket should connect");
        let (mut socket_b, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("second websocket should connect");

        for socket in [&mut socket_a, &mut socket_b] {
            socket
                .send(Message::Text(
                    json!({
                        "id": "subscribe-user-message",
                        "method": "session/subscribe",
                        "params": { "session_key": "websocket:user-message" }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .expect("session/subscribe should send");
            for _ in 0..2 {
                let _ = socket
                    .next()
                    .await
                    .expect("subscribe response frame")
                    .expect("subscribe response message");
            }
        }

        socket_a
            .send(Message::Text(
                json!({
                    "id": "turn-user-message-1",
                    "method": "turn/start",
                    "params": {
                        "session_id": "websocket:user-message",
                        "thread_id": "thread_user_message",
                        "turn_id": "turn_user_message",
                        "input": [{ "type": "text", "text": "hello from browser a" }]
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("turn/start should send");

        let _result = socket_a
            .next()
            .await
            .expect("turn/start result")
            .expect("turn/start result message");

        let mut saw_user_message_a = false;
        let mut saw_user_message_b = false;
        for _ in 0..3 {
            if let Ok(Some(Ok(Message::Text(text)))) =
                timeout(Duration::from_millis(250), socket_a.next()).await
            {
                let frame = serde_json::from_str::<serde_json::Value>(&text)
                    .expect("first connection frame parses");
                saw_user_message_a |= is_user_message_item(&frame, "hello from browser a");
            }

            if let Ok(Some(Ok(Message::Text(text)))) =
                timeout(Duration::from_millis(250), socket_b.next()).await
            {
                let frame = serde_json::from_str::<serde_json::Value>(&text)
                    .expect("second connection frame parses");
                saw_user_message_b |= is_user_message_item(&frame, "hello from browser a");
            }
        }

        assert!(
            saw_user_message_a,
            "initiating connection should receive userMessage item"
        );
        assert!(
            saw_user_message_b,
            "subscribed peer should receive userMessage item"
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_v1_turn_failure_reaches_all_subscribed_connections() {
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

        let (mut socket_a, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("first websocket should connect");
        let (mut socket_b, _) = connect_async(ws_url(handle.info().actual_port, None))
            .await
            .expect("second websocket should connect");

        for socket in [&mut socket_a, &mut socket_b] {
            socket
                .send(Message::Text(
                    json!({
                        "id": "subscribe-failing",
                        "method": "session/subscribe",
                        "params": { "session_key": "websocket:failing" }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .expect("session/subscribe should send");
            for _ in 0..2 {
                let _ = socket
                    .next()
                    .await
                    .expect("subscribe response frame")
                    .expect("subscribe response message");
            }
        }

        socket_a
            .send(Message::Text(
                json!({
                    "id": "turn-failing-1",
                    "method": "turn/start",
                    "params": {
                        "session_id": "websocket:failing",
                        "thread_id": "thread_failing",
                        "turn_id": "turn_failing",
                        "input": [{ "type": "text", "text": "__fail__" }]
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("turn/start should send");

        let _result = socket_a
            .next()
            .await
            .expect("turn/start result")
            .expect("turn/start result message");

        let mut saw_failed_a = false;
        let mut saw_failed_b = false;
        for _ in 0..3 {
            let frame_a = timeout(Duration::from_secs(1), socket_a.next())
                .await
                .expect("first connection should receive lifecycle frame")
                .expect("first connection lifecycle frame")
                .expect("first connection lifecycle message");
            if let Message::Text(text) = frame_a {
                let frame = serde_json::from_str::<serde_json::Value>(&text)
                    .expect("first connection frame parses");
                saw_failed_a |= frame.get("method").and_then(|value| value.as_str())
                    == Some("turn/failed")
                    && frame
                        .pointer("/params/error/code")
                        .and_then(|value| value.as_str())
                        == Some("internal_error");
            }

            let frame_b = timeout(Duration::from_secs(1), socket_b.next())
                .await
                .expect("second connection should receive lifecycle frame")
                .expect("second connection lifecycle frame")
                .expect("second connection lifecycle message");
            if let Message::Text(text) = frame_b {
                let frame = serde_json::from_str::<serde_json::Value>(&text)
                    .expect("second connection frame parses");
                saw_failed_b |= frame.get("method").and_then(|value| value.as_str())
                    == Some("turn/failed")
                    && frame
                        .pointer("/params/error/code")
                        .and_then(|value| value.as_str())
                        == Some("internal_error");
            }
        }

        assert!(
            saw_failed_a,
            "initiating connection should receive turn/failed"
        );
        assert!(saw_failed_b, "subscribed peer should receive turn/failed");

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_v1_server_request_responses_return_not_implemented_error() {
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
            .expect("approval response error")
            .expect("approval response message");
        let Message::Text(text) = result else {
            panic!("unexpected approval error frame: {result:?}");
        };
        let result =
            serde_json::from_str::<serde_json::Value>(&text).expect("approval error should parse");
        assert_eq!(
            result.get("id").and_then(|value| value.as_str()),
            Some("approval-response-1")
        );
        assert_eq!(
            result
                .pointer("/error/code")
                .and_then(|value| value.as_str()),
            Some("method_not_found")
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

        socket
            .send(Message::Text(
                json!({
                    "id": "turn-start-1",
                    "method": "turn/start",
                    "params": {
                        "session_id": "websocket:v1-session",
                        "thread_id": "thr_v1",
                        "turn_id": "turn_v1",
                        "input": [{ "type": "text", "text": "__hold__" }]
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("turn/start should send");
        let _start_result = socket
            .next()
            .await
            .expect("turn/start result")
            .expect("turn/start result message");
        let _started_event = next_json_frame_matching(&mut socket, "turn/started event", |frame| {
            frame.get("method").and_then(|value| value.as_str()) == Some("turn/started")
        })
        .await;

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

        let result = next_json_frame_matching(&mut socket, "cancel result", |frame| {
            frame.get("id").and_then(|value| value.as_str()) == Some("cancel-1")
        })
        .await;
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

        let event = next_json_frame_matching(&mut socket, "interrupted event", |frame| {
            frame.get("method").and_then(|value| value.as_str()) == Some("turn/interrupted")
        })
        .await;
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
        assert!(
            timeout(Duration::from_millis(100), socket.next())
                .await
                .is_err(),
            "cancelled turn should not emit a later completion frame"
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_v1_turn_start_failure_emits_turn_failed_event() {
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

        socket
            .send(Message::Text(
                json!({
                    "id": "turn-fail-1",
                    "method": "turn/start",
                    "params": {
                        "session_id": "websocket:v1-session",
                        "thread_id": "thr_v1",
                        "turn_id": "turn_fail_v1",
                        "input": [{ "type": "text", "text": "__fail__" }]
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("turn/start should send");

        let _start_result = socket
            .next()
            .await
            .expect("turn/start result")
            .expect("turn/start result message");
        let _started_event = next_json_frame_matching(&mut socket, "turn/started event", |frame| {
            frame.get("method").and_then(|value| value.as_str()) == Some("turn/started")
        })
        .await;
        let failed = next_json_frame_matching(&mut socket, "turn/failed event", |frame| {
            frame.get("method").and_then(|value| value.as_str()) == Some("turn/failed")
        })
        .await;
        assert_eq!(
            failed.get("method").and_then(|value| value.as_str()),
            Some("turn/failed")
        );
        assert_eq!(
            failed
                .pointer("/params/error/code")
                .and_then(|value| value.as_str()),
            Some("internal_error")
        );

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_v1_turn_start_enforces_active_turn_limit() {
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

        for index in 0..crate::websocket::GATEWAY_WEBSOCKET_MAX_ACTIVE_TURNS_PER_CONNECTION {
            socket
                .send(Message::Text(
                    json!({
                        "id": format!("turn-start-{index}"),
                        "method": "turn/start",
                        "params": {
                            "session_id": "websocket:v1-session",
                            "thread_id": "thr_v1",
                            "turn_id": format!("turn_v1_{index}"),
                            "input": [{ "type": "text", "text": "__hold__" }]
                        }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .expect("turn/start should send");
            let _start_result = socket
                .next()
                .await
                .expect("turn/start result")
                .expect("turn/start result message");
            let _started_event =
                next_json_frame_matching(&mut socket, "turn/started event", |frame| {
                    frame.get("method").and_then(|value| value.as_str()) == Some("turn/started")
                })
                .await;
        }

        socket
            .send(Message::Text(
                json!({
                    "id": "turn-start-over-limit",
                    "method": "turn/start",
                    "params": {
                        "session_id": "websocket:v1-session",
                        "thread_id": "thr_v1",
                        "turn_id": "turn_v1_over_limit",
                        "input": [{ "type": "text", "text": "__hold__" }]
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("over limit turn/start should send");

        let error = next_json_frame_matching(&mut socket, "over limit error", |frame| {
            frame
                .pointer("/error/code")
                .and_then(|value| value.as_str())
                == Some("too_many_active_turns")
        })
        .await;
        assert_eq!(
            error
                .pointer("/error/code")
                .and_then(|value| value.as_str()),
            Some("too_many_active_turns")
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
    async fn websocket_does_not_emit_legacy_startup_frame_before_initialize() {
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
        assert!(
            timeout(Duration::from_millis(100), socket.next())
                .await
                .is_err(),
            "v1 websocket should not emit a legacy startup frame"
        );

        socket
            .send(Message::Text(
                json!({
                    "id": "init-after-silence",
                    "method": "initialize",
                    "params": {
                        "client_info": { "name": "test-client" },
                        "capabilities": { "protocol_version": "v1" }
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
            Some("init-after-silence")
        );
        assert!(frame.get("type").is_none());

        handle.shutdown().await.expect("gateway should stop");
    }

    #[tokio::test]
    async fn websocket_rejects_legacy_method_frames_as_v1_errors() {
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
        socket
            .send(Message::Text(
                json!({
                    "type": "method",
                    "id": "legacy-submit",
                    "method": "session.submit",
                    "params": { "input": "legacy request" }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("legacy method should send");

        let frame = socket
            .next()
            .await
            .expect("legacy rejection response")
            .expect("legacy rejection message");
        let Message::Text(text) = frame else {
            panic!("unexpected legacy rejection frame: {frame:?}");
        };
        let frame = serde_json::from_str::<serde_json::Value>(&text)
            .expect("legacy rejection should parse");
        assert_eq!(
            frame.get("id").and_then(|value| value.as_str()),
            Some("legacy-submit")
        );
        assert_eq!(
            frame
                .pointer("/error/code")
                .and_then(|value| value.as_str()),
            Some("invalid_request")
        );
        assert!(
            requests
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .is_empty()
        );

        handle.shutdown().await.expect("gateway should stop");
    }
}
