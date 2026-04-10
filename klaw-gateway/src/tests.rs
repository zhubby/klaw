#[cfg(test)]
mod tests {
    use crate::{
        CHAT_DIST_JS_PATH, CHAT_DIST_WASM_PATH, CHAT_PATH, GatewayOptions, GatewayWebsocketHandler,
        GatewayWebsocketHandlerError, GatewayWebsocketServerFrame, GatewayWebsocketSubmitRequest,
        HOME_LOGO_PATH, HOME_PATH, WEBHOOK_AGENTS_PATH, WEBHOOK_EVENTS_PATH, WS_CHAT_PATH,
        spawn_gateway, spawn_gateway_with_options,
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
    use tokio_tungstenite::{connect_async, tungstenite::Message};

    #[derive(Clone, Default)]
    struct RecordingWebsocketHandler {
        requests: Arc<Mutex<Vec<GatewayWebsocketSubmitRequest>>>,
    }

    #[async_trait]
    impl GatewayWebsocketHandler for RecordingWebsocketHandler {
        async fn submit(
            &self,
            request: GatewayWebsocketSubmitRequest,
        ) -> Result<Vec<GatewayWebsocketServerFrame>, GatewayWebsocketHandlerError> {
            self.requests
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(request.clone());
            Ok(vec![GatewayWebsocketServerFrame::Result {
                id: request.request_id,
                result: json!({
                    "response": {
                        "content": format!("ack: {}", request.input),
                    },
                    "session_key": request.session_key,
                    "stream": false,
                }),
            }])
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
        let mut url = format!("ws://127.0.0.1:{port}{WS_CHAT_PATH}");
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
            .get(format!("{base_url}{HOME_PATH}"))
            .send()
            .await
            .expect("home page should respond");
        assert_eq!(home_response.status(), StatusCode::OK);
        let home_html = home_response
            .text()
            .await
            .expect("home page body should load");
        assert!(home_html.contains("Little Claws, Big Conversations."));
        assert!(home_html.contains(HOME_LOGO_PATH));
        assert!(home_html.contains("href=\"/chat\""));
        assert!(home_html.contains("Open web chat"));

        let logo_response = client
            .get(format!("{base_url}{HOME_LOGO_PATH}"))
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
            .get(format!("{base_url}{CHAT_PATH}"))
            .send()
            .await
            .expect("chat page should respond");
        assert_eq!(chat_html.status(), StatusCode::OK);
        let body = chat_html.text().await.expect("chat body");
        assert!(body.contains("klaw_chat_canvas"));
        assert!(body.contains(CHAT_DIST_JS_PATH));

        let js = client
            .get(format!("{base_url}{CHAT_DIST_JS_PATH}"))
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
            .get(format!("{base_url}{CHAT_DIST_WASM_PATH}"))
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
        assert_eq!(HOME_PATH, "/");
        assert_eq!(HOME_LOGO_PATH, "/assets/logo.webp");
        assert_eq!(CHAT_PATH, "/chat");
        assert_eq!(CHAT_DIST_JS_PATH, "/chat/dist/klaw_webui.js");
        assert_eq!(CHAT_DIST_WASM_PATH, "/chat/dist/klaw_webui_bg.wasm");
        assert_eq!(WS_CHAT_PATH, "/ws/chat");
        assert_eq!(WEBHOOK_EVENTS_PATH, "/webhook/events");
        assert_eq!(WEBHOOK_AGENTS_PATH, "/webhook/agents");
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
                assert_eq!(event, "session.connected");
            }
            other => panic!("unexpected connected frame: {other:?}"),
        }

        socket
            .send(Message::Text(
                json!({
                    "type": "method",
                    "id": "sub-1",
                    "method": "session.subscribe",
                    "params": { "session_key": "web:test-session" }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("subscribe should send");

        for _ in 0..2 {
            let _ = socket.next().await;
        }

        socket
            .send(Message::Text(
                json!({
                    "type": "method",
                    "id": "req-1",
                    "method": "session.submit",
                    "params": {
                        "input": "hello gateway",
                        "channel_id": "default"
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
        assert_eq!(recorded[0].session_key, "web:test-session");
        assert_eq!(recorded[0].chat_id, "web:test-session");
        assert_eq!(recorded[0].channel_id, "default");
        assert_eq!(recorded[0].input, "hello gateway");

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
