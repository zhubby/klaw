use klaw_gateway::{
    GatewayApprovalDecision, GatewayApprovalRequest, GatewayApprovalScope, GatewayContentBlock,
    GatewayProtocolCapabilities, GatewayProtocolClientInfo, GatewayProtocolErrorCode,
    GatewayProtocolMethod, GatewayProtocolSchemaBundle, GatewayRpcMessage, GatewayThreadItem,
    GatewayThreadItemStatus, GatewayThreadItemType, GatewayToolCall, GatewayToolCallStatus,
    GatewayTurnStatus, GatewayWebsocketProtocolInitializeParams,
    GatewayWebsocketProtocolInitializeResult, GatewayWebsocketProtocolVersion,
    GatewayWebsocketTurnStarted,
};
use serde_json::json;

#[test]
fn v1_request_response_and_notification_envelopes_use_json_rpc_shape_without_type_tags() {
    let request = GatewayRpcMessage::request(
        "req-1",
        GatewayProtocolMethod::TurnStart,
        json!({ "thread_id": "thr_1" }),
    );
    let notification =
        GatewayRpcMessage::notification(GatewayProtocolMethod::ItemStarted, json!({}));
    let response = GatewayRpcMessage::success("req-1", json!({ "ok": true }));

    assert_eq!(
        serde_json::to_value(request).expect("request should serialize"),
        json!({
            "id": "req-1",
            "method": "turn/start",
            "params": { "thread_id": "thr_1" }
        })
    );
    assert_eq!(
        serde_json::to_value(notification).expect("notification should serialize"),
        json!({
            "method": "item/started",
            "params": {}
        })
    );
    assert_eq!(
        serde_json::to_value(response).expect("response should serialize"),
        json!({
            "id": "req-1",
            "result": { "ok": true }
        })
    );
}

#[test]
fn initialize_result_declares_stable_version_and_negotiated_capabilities() {
    let params = GatewayWebsocketProtocolInitializeParams {
        client_info: GatewayProtocolClientInfo {
            name: "test-client".to_string(),
            title: Some("Test Client".to_string()),
            version: Some("0.1.0".to_string()),
        },
        capabilities: GatewayProtocolCapabilities {
            protocol_version: Some(GatewayWebsocketProtocolVersion::V1),
            experimental: false,
            ..GatewayProtocolCapabilities::default()
        },
    };

    let result = GatewayWebsocketProtocolInitializeResult::negotiate("conn-1".to_string(), params);

    assert_eq!(result.protocol_version, GatewayWebsocketProtocolVersion::V1);
    assert_eq!(result.connection_id, "conn-1");
    assert!(result.capabilities.turns);
    assert!(result.capabilities.items);
    assert!(result.capabilities.schema);
    assert!(!result.capabilities.experimental);
}

#[test]
fn turn_started_requires_separate_request_thread_turn_and_session_identity() {
    let event = GatewayWebsocketTurnStarted {
        session_id: "websocket:session".to_string(),
        thread_id: "thr_session".to_string(),
        turn_id: "turn_req_1".to_string(),
        request_id: "req-1".to_string(),
        status: GatewayTurnStatus::InProgress,
    };

    assert_eq!(
        serde_json::to_value(event).expect("turn event should serialize"),
        json!({
            "session_id": "websocket:session",
            "thread_id": "thr_session",
            "turn_id": "turn_req_1",
            "request_id": "req-1",
            "status": "in_progress"
        })
    );
}

#[test]
fn thread_items_cover_agent_message_tool_call_file_change_and_approval_states() {
    let item = GatewayThreadItem {
        item_id: "item_tool_1".to_string(),
        turn_id: "turn_1".to_string(),
        item_type: GatewayThreadItemType::ToolCall,
        status: GatewayThreadItemStatus::InProgress,
        payload: json!({
            "tool_call_id": "tool_1",
            "name": "shell",
            "kind": "command",
            "arguments": { "command": "cargo test" },
            "approval": {
                "scope": "turn",
                "status": "pending"
            }
        }),
    };

    assert_eq!(item.item_type, GatewayThreadItemType::ToolCall);
    assert_eq!(item.status, GatewayThreadItemStatus::InProgress);
    assert_eq!(
        item.payload
            .pointer("/approval/scope")
            .and_then(serde_json::Value::as_str),
        Some("turn")
    );

    let supported = GatewayThreadItemType::stable_v1();
    assert!(supported.contains(&GatewayThreadItemType::AgentMessage));
    assert!(supported.contains(&GatewayThreadItemType::Reasoning));
    assert!(supported.contains(&GatewayThreadItemType::Plan));
    assert!(supported.contains(&GatewayThreadItemType::ToolCall));
    assert!(supported.contains(&GatewayThreadItemType::CommandExecution));
    assert!(supported.contains(&GatewayThreadItemType::FileChange));
    assert!(supported.contains(&GatewayThreadItemType::McpToolCall));
}

#[test]
fn content_blocks_and_tool_approval_payloads_have_structured_v1_shapes() {
    let content = vec![
        GatewayContentBlock::Text {
            text: "hello".to_string(),
        },
        GatewayContentBlock::Attachment {
            archive_id: "arch_1".to_string(),
            filename: Some("notes.md".to_string()),
            mime_type: Some("text/markdown".to_string()),
            size_bytes: 42,
        },
    ];
    assert_eq!(
        serde_json::to_value(content).expect("content blocks should serialize"),
        json!([
            { "type": "text", "text": "hello" },
            {
                "type": "attachment",
                "archive_id": "arch_1",
                "filename": "notes.md",
                "mime_type": "text/markdown",
                "size_bytes": 42
            }
        ])
    );

    let tool_call = GatewayToolCall {
        tool_call_id: "tool_1".to_string(),
        name: "shell".to_string(),
        kind: "command".to_string(),
        status: GatewayToolCallStatus::InProgress,
        arguments: json!({ "command": "cargo test" }),
        result: None,
        error: None,
        duration_ms: None,
    };
    let approval = GatewayApprovalRequest {
        request_id: "srv_req_1".to_string(),
        thread_id: "thr_1".to_string(),
        turn_id: "turn_1".to_string(),
        item_id: "item_tool_1".to_string(),
        scope: GatewayApprovalScope::Turn,
        message: Some("Allow command?".to_string()),
        options: vec![
            GatewayApprovalDecision::Accept,
            GatewayApprovalDecision::Decline,
        ],
        payload: json!({ "tool_call_id": "tool_1" }),
    };

    assert_eq!(
        serde_json::to_value(tool_call)
            .expect("tool call should serialize")
            .pointer("/status")
            .and_then(serde_json::Value::as_str),
        Some("in_progress")
    );
    assert_eq!(
        serde_json::to_value(approval)
            .expect("approval request should serialize")
            .pointer("/scope")
            .and_then(serde_json::Value::as_str),
        Some("turn")
    );
}

#[test]
fn schema_bundle_exposes_core_v1_definitions_and_error_codes() {
    let bundle = GatewayProtocolSchemaBundle::v1();

    assert_eq!(bundle.protocol_version, GatewayWebsocketProtocolVersion::V1);
    assert!(bundle.definitions.contains_key("GatewayRpcMessage"));
    assert!(bundle.definitions.contains_key("GatewayThreadItem"));
    assert!(
        bundle
            .definitions
            .contains_key("GatewayWebsocketTurnStarted")
    );
    assert!(
        bundle
            .error_codes
            .contains(&GatewayProtocolErrorCode::Overloaded)
    );
    assert!(
        bundle
            .error_codes
            .contains(&GatewayProtocolErrorCode::NotInitialized)
    );
}
