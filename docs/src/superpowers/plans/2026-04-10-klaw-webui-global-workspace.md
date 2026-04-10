# klaw-webui Global Workspace Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `klaw-webui` 改为全局 websocket 驱动的 agent 工作区，未连接时不显示 agent 内容，也不允许新建 agent。

**Architecture:** 先在网关 websocket 协议中补 `workspace.bootstrap` 与 `session.create`，并复用现有 session 存储中的 `created_at_ms` 与聊天记录读取能力，给前端提供稳定的会话元数据和会话内容来源。随后把 `klaw-webui` 从“每个 agent 持有 websocket”改成“`ChatApp` 持有全局 websocket + 按 `session_key` 路由消息”，最后收紧本地持久化，只保留 token、主题和可选布局。

**Tech Stack:** Rust 2024, `axum` websocket, `tokio`, `serde_json`, `eframe`/`egui` web, `web_sys::WebSocket`

---

### Task 1: 为网关 websocket 增加工作区初始化方法

**Files:**
- Modify: `klaw-gateway/src/websocket.rs`
- Test: `klaw-gateway/src/tests.rs`
- Use: `klaw-session/src/manager.rs`
- Use: `klaw-storage/src/types.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[tokio::test]
async fn websocket_workspace_bootstrap_returns_sessions_sorted_by_created_at_desc() {
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
            assert!(result.get("sessions").is_some());
        }
        other => panic!("unexpected bootstrap frame: {other:?}"),
    }

    handle.shutdown().await.expect("gateway should stop");
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p klaw-gateway websocket_workspace_bootstrap_returns_sessions_sorted_by_created_at_desc -- --exact`

Expected: FAIL with `unknown_method` or missing `workspace.bootstrap`

- [ ] **Step 3: 实现最小协议支持**

```rust
const METHOD_WORKSPACE_BOOTSTRAP: &str = "workspace.bootstrap";

#[derive(Debug, Serialize)]
struct WorkspaceBootstrapSession {
    session_key: String,
    title: String,
    created_at_ms: i64,
}

// handle_text_message match arm
METHOD_WORKSPACE_BOOTSTRAP => {
    let sessions = list_web_sessions(state).await?;
    vec![GatewayWebsocketServerFrame::Result {
        id,
        result: json!({ "sessions": sessions, "active_session_key": sessions.first().map(|s| s.session_key.clone()) }),
    }]
}
```

- [ ] **Step 4: 再跑测试确认通过**

Run: `cargo test -p klaw-gateway websocket_workspace_bootstrap_returns_sessions_sorted_by_created_at_desc -- --exact`

Expected: PASS

### Task 2: 为网关 websocket 增加 `session.create`

**Files:**
- Modify: `klaw-gateway/src/websocket.rs`
- Test: `klaw-gateway/src/tests.rs`
- Use: `klaw-storage/src/types.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[tokio::test]
async fn websocket_session_create_returns_created_session_metadata() {
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
            assert!(result.get("session_key").and_then(|v| v.as_str()).is_some());
            assert!(result.get("title").and_then(|v| v.as_str()).is_some());
            assert!(result.get("created_at_ms").and_then(|v| v.as_i64()).is_some());
        }
        other => panic!("unexpected create frame: {other:?}"),
    }

    handle.shutdown().await.expect("gateway should stop");
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p klaw-gateway websocket_session_create_returns_created_session_metadata -- --exact`

Expected: FAIL with `unknown_method` or missing `session.create`

- [ ] **Step 3: 实现最小创建逻辑**

```rust
const METHOD_SESSION_CREATE: &str = "session.create";

fn next_web_session_key() -> String {
    format!("web:{}", Uuid::new_v4())
}

fn next_web_session_title(existing_count: usize) -> String {
    format!("Agent {}", existing_count + 1)
}

METHOD_SESSION_CREATE => {
    let created = create_web_session(state).await?;
    *current_session_key = Some(created.session_key.clone());
    update_connection_session_key(state, connection_id, Some(created.session_key.clone())).await;
    vec![GatewayWebsocketServerFrame::Result {
        id,
        result: serde_json::to_value(&created).unwrap_or_else(|_| json!({})),
    }]
}
```

- [ ] **Step 4: 再跑测试确认通过**

Run: `cargo test -p klaw-gateway websocket_session_create_returns_created_session_metadata -- --exact`

Expected: PASS

### Task 3: 让订阅返回或触发历史内容

**Files:**
- Modify: `klaw-gateway/src/websocket.rs`
- Test: `klaw-gateway/src/tests.rs`
- Use: `klaw-session/src/manager.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[tokio::test]
async fn websocket_subscribe_emits_existing_chat_history_for_session() {
    // test setup creates a session and appends at least one ChatRecord
    // then subscribes and asserts a session.message event is emitted
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p klaw-gateway websocket_subscribe_emits_existing_chat_history_for_session -- --exact`

Expected: FAIL because subscribe currently only returns subscribed metadata

- [ ] **Step 3: 实现最小历史回放**

```rust
let history = session_manager.read_chat_records(&session_key).await?;
let mut frames = vec![
    GatewayWebsocketServerFrame::Result { ... },
    GatewayWebsocketServerFrame::Event { event: EVENT_SESSION_SUBSCRIBED.to_string(), payload: ... },
];
for record in history {
    frames.push(GatewayWebsocketServerFrame::Event {
        event: "session.message".to_string(),
        payload: json!({
            "session_key": session_key,
            "response": { "content": record.content },
            "history": true,
            "role": record.role,
            "timestamp_ms": record.ts_ms,
        }),
    });
}
```

- [ ] **Step 4: 再跑测试确认通过**

Run: `cargo test -p klaw-gateway websocket_subscribe_emits_existing_chat_history_for_session -- --exact`

Expected: PASS

### Task 4: 将 `klaw-webui` 改成全局 websocket 驱动

**Files:**
- Modify: `klaw-webui/src/web_chat/app.rs`
- Modify: `klaw-webui/src/web_chat/session.rs`
- Modify: `klaw-webui/src/web_chat/transport.rs`
- Modify: `klaw-webui/src/web_chat/protocol.rs`
- Test: `klaw-webui/src/web_chat/*.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn sort_sessions_by_created_at_desc_keeps_newest_first() {
    let sessions = vec![
        RemoteSessionMeta { session_key: "web:1".into(), title: "Agent 1".into(), created_at_ms: 10 },
        RemoteSessionMeta { session_key: "web:2".into(), title: "Agent 2".into(), created_at_ms: 20 },
    ];
    let ordered = sort_sessions_by_created_at_desc(sessions);
    assert_eq!(ordered[0].session_key, "web:2");
    assert_eq!(ordered[1].session_key, "web:1");
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p klaw-webui sort_sessions_by_created_at_desc_keeps_newest_first -- --exact`

Expected: FAIL because helper and metadata type do not exist yet

- [ ] **Step 3: 实现最小全局状态重构**

```rust
pub(super) struct ChatApp {
    ctx: Context,
    gateway_token: Option<String>,
    gateway_token_input: String,
    sessions: Vec<SessionWindow>,
    active_session_key: Option<String>,
    ws: Rc<RefCell<Option<WebSocket>>>,
    connection_state: Rc<RefCell<ConnectionState>>,
    workspace_loaded: Rc<RefCell<bool>>,
    // ...
}
```

并完成：

- 移除每个 `SessionWindow` 自己的 websocket
- 新增 `workspace.bootstrap` 和 `session.create` 的客户端发送
- 新增按 `session_key` 路由消息与历史的逻辑
- 保留流式 assistant 输出，但流状态改为全局按会话映射

- [ ] **Step 4: 再跑测试确认通过**

Run: `cargo test -p klaw-webui sort_sessions_by_created_at_desc_keeps_newest_first -- --exact`

Expected: PASS

### Task 5: 用页面模式切断未连接时的 agent 工作区

**Files:**
- Modify: `klaw-webui/src/web_chat/ui.rs`
- Modify: `klaw-webui/src/web_chat/app.rs`
- Test: `klaw-webui/src/web_chat/ui.rs` or extracted helpers

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn derive_page_mode_hides_workspace_until_bootstrap_is_ready() {
    assert_eq!(
        derive_page_mode(ConnectionState::Disconnected, false),
        PageMode::ConnectionGuide
    );
    assert_eq!(
        derive_page_mode(ConnectionState::Connected, false),
        PageMode::LoadingWorkspace
    );
    assert_eq!(
        derive_page_mode(ConnectionState::Connected, true),
        PageMode::Workspace
    );
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p klaw-webui derive_page_mode_hides_workspace_until_bootstrap_is_ready -- --exact`

Expected: FAIL because page mode helper does not exist yet

- [ ] **Step 3: 实现最小页面模式切换**

```rust
enum PageMode {
    ConnectionGuide,
    LoadingWorkspace,
    Workspace,
}
```

并完成：

- 未连接时不渲染左侧 agent 列表
- 未连接时中央显示连接引导页
- `New Agent` 仅在 `Workspace` 模式可用
- 移除每个 agent 窗口的 `Connect` / `Disconnect`

- [ ] **Step 4: 再跑测试确认通过**

Run: `cargo test -p klaw-webui derive_page_mode_hides_workspace_until_bootstrap_is_ready -- --exact`

Expected: PASS

### Task 6: 收紧本地持久化边界

**Files:**
- Modify: `klaw-webui/src/web_chat/storage.rs`
- Modify: `klaw-webui/src/web_chat/app.rs`
- Test: `klaw-webui/src/web_chat/storage.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn persisted_workspace_state_defaults_without_local_sessions() {
    let state = default_workspace_state();
    assert!(state.active_session_key.is_none());
    assert!(state.gateway_token.is_none());
    assert!(state.sessions.is_empty());
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p klaw-webui persisted_workspace_state_defaults_without_local_sessions -- --exact`

Expected: FAIL because default state still seeds `Agent 1`

- [ ] **Step 3: 实现最小持久化收缩**

```rust
fn default_workspace_state() -> PersistedWorkspaceState {
    PersistedWorkspaceState {
        legacy_theme_mode: None,
        sessions: Vec::new(),
        active_session_key: None,
        next_session_number: 1,
        gateway_token: None,
    }
}
```

并继续收缩：

- 删除 `next_session_number` 的真实业务用途
- 页面刷新后不再从 local storage 恢复 agent 列表
- 仅保留 token、主题、可选布局

- [ ] **Step 4: 再跑测试确认通过**

Run: `cargo test -p klaw-webui persisted_workspace_state_defaults_without_local_sessions -- --exact`

Expected: PASS

### Task 7: 端到端验证与整理

**Files:**
- Modify if needed: `klaw-gateway/src/websocket.rs`
- Modify if needed: `klaw-webui/src/web_chat/*.rs`
- Doc already created: `docs/superpowers/specs/2026-04-10-klaw-webui-global-workspace-design.md`

- [ ] **Step 1: 运行格式化**

Run: `cargo fmt --all`
Expected: exit 0

- [ ] **Step 2: 运行网关相关测试**

Run: `cargo test -p klaw-gateway websocket_ -- --nocapture`
Expected: new websocket tests pass

- [ ] **Step 3: 运行 webui crate 测试**

Run: `cargo test -p klaw-webui`
Expected: PASS

- [ ] **Step 4: 运行工作区级最小编译验证**

Run: `cargo check --workspace`
Expected: exit 0
