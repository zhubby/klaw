#[cfg(test)]
mod tests {
    use crate::{
        spawn_gateway,
        webhook::{is_authorized, normalize_webhook_request, GatewayWebhookPayload},
    };
    use axum::http::{HeaderMap, HeaderValue};
    use klaw_config::GatewayConfig;
    use serde_json::json;

    #[tokio::test]
    async fn spawn_gateway_uses_actual_random_port() {
        let config = GatewayConfig {
            enabled: true,
            listen_ip: "127.0.0.1".to_string(),
            listen_port: 0,
            tls: Default::default(),
            webhook: Default::default(),
        };

        let handle = spawn_gateway(&config).await.expect("gateway should start");
        assert!(handle.info().actual_port > 0);
        assert!(handle
            .info()
            .ws_url
            .contains(&handle.info().actual_port.to_string()));

        handle.shutdown().await.expect("gateway should stop");
    }

    #[test]
    fn webhook_authorization_accepts_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret-token"),
        );
        assert!(is_authorized(&headers, "secret-token"));
        assert!(!is_authorized(&headers, "wrong-token"));
    }

    #[test]
    fn normalize_webhook_request_applies_defaults() {
        let request = normalize_webhook_request(
            GatewayWebhookPayload {
                source: "github".to_string(),
                event_type: "issue_comment.created".to_string(),
                content: "New comment".to_string(),
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
        assert_eq!(request.sender_id, "github:webhook");
        assert_eq!(
            request.metadata.get("trigger.kind"),
            Some(&json!("webhook"))
        );
    }
}
