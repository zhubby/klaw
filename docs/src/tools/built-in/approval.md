# Approval Tool 设计与实现

本文档记录 `klaw-tool` 中 `approval` 工具的实现：审批记录管理、状态流转、会话隔离与过期控制。

## 目标

- 为高风险操作提供持久化的审批记录管理
- 支持审批请求、查询、决议（批准/拒绝）的完整生命周期
- 基于会话的隔离机制，确保审批记录归属安全
- 自动过期控制，避免审批记录永久挂起

## 代码位置

- 工具实现：`klaw-tool/src/approval.rs`
- 存储依赖：`klaw-storage`（`DefaultSessionStore`、`ApprovalRecord`）

## 参数模型（强约束）

`approval` 工具使用 `oneOf` 联合结构定义三种操作，均开启 `deny_unknown_fields`。

### `request` 操作 - 创建审批记录

| 字段 | 类型 | 必填 | 描述 | 默认值 |
|------|------|------|------|--------|
| `action` | `"request"` | 是 | 操作类型 | - |
| `tool_name` | `string` | 是 | 需要审批的工具名称，如 `shell` | - |
| `command_text` | `string` | 是 | 需要审批的完整操作文本 | - |
| `command_preview` | `string` | 否 | 短预览文本，显示在审批 UI 中 | 自动截取 `command_text` 前 160 字符 |
| `command_hash` | `string` | 否 | 命令哈希值 | `sha256(command_text)` |
| `risk_level` | `string` | 否 | 风险标签 | `"mutating"` |
| `requested_by` | `string` | 否 | 请求方标识 | `"agent"` |
| `justification` | `string` | 否 | 审批理由说明 | - |
| `expires_in_minutes` | `integer` | 否 | 审批有效期（分钟） | `10` |

约束：
- `expires_in_minutes` 范围：1 ~ 10080（7 天）

### `get` 操作 - 查询审批记录

| 字段 | 类型 | 必填 | 描述 |
|------|------|------|------|
| `action` | `"get"` | 是 | 操作类型 |
| `approval_id` | `string` | 是 | 审批记录 ID |

### `resolve` 操作 - 决议审批

| 字段 | 类型 | 必填 | 描述 | 默认值 |
|------|------|------|------|--------|
| `action` | `"resolve"` | 是 | 操作类型 | - |
| `approval_id` | `string` | 是 | 审批记录 ID | - |
| `decision` | `"approve"` / `"reject"` | 是 | 决议结果 | - |
| `actor` | `string` | 否 | 决议人标识 | `"channel-user"` |

## 配置模型

审批工具本身无需独立配置项，依赖 `klaw-storage` 的存储路径配置：

```toml
# 存储路径配置
[storage]
root = "~/.klaw/data"
```

审批记录的过期时间由请求方在 `request` 时指定，默认 10 分钟，最长 7 天。

## 审批状态机

审批记录状态流转：

```
Pending ──► Approved   (resolve: decision=approve)
        ├─► Rejected   (resolve: decision=reject)
        └─► Expired    (自动过期，超时检测时触发)
```

### 状态定义

| 状态 | 说明 |
|------|------|
| `Pending` | 待审批，初始状态 |
| `Approved` | 已批准 |
| `Rejected` | 已拒绝 |
| `Expired` | 已过期，自动触发 |

### 过期检测

`resolve_for_session` 方法在决议时会先检查过期时间：
- 若 `expires_at_ms < now` 且状态仍为 `Pending`，自动标记为 `Expired`
- 已 `Approved`/`Rejected`/`Expired` 的记录不可再次决议

## 会话隔离

所有审批操作均进行会话归属校验：

1. **创建**：`request` 操作将 `session_key` 写入记录
2. **查询**：`get` 操作校验 `approval.session_key == ctx.session_key`
3. **决议**：`resolve` 操作同样进行归属校验

跨会话访问会被拒绝，错误信息包含 `"does not belong to current session"`。

## 实现细节

### 命令哈希

使用 SHA-256 计算 `command_text` 的哈希值：

```rust
fn command_hash(command: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(command.trim().as_bytes());
    format!("{:x}", hasher.finalize())
}
```

### 命令预览

自动截取策略：

```rust
fn command_preview(command: &str) -> String {
    let trimmed = command.trim();
    let max = 160;
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let mut preview = trimmed.chars().take(max).collect::<String>();
    preview.push_str("...");
    preview
}
```

### 参数归一化

所有字符串参数均经过 `trim()` 处理，空值校验：

```rust
fn normalize_non_empty(value: &str, field: &str) -> Result<String, ToolError> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(ToolError::InvalidArgs(format!("`{field}` cannot be empty")));
    }
    Ok(normalized.to_string())
}
```

### 时间戳

使用 `time::OffsetDateTime` 获取当前时间戳（毫秒）：

```rust
fn now_ms() -> i64 {
    (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}
```

## 输出格式

工具输出为结构化 JSON，包含操作类型、更新标志与审批记录：

```json
{
  "action": "request",
  "updated": true,
  "approval": {
    "id": "uuid-v4-string",
    "session_key": "session-123",
    "tool_name": "shell",
    "command_text": "rm -rf build/",
    "command_preview": "rm -rf build/",
    "command_hash": "abc123...",
    "risk_level": "destructive",
    "requested_by": "agent",
    "justification": "Cleanup build artifacts",
    "status": "pending",
    "resolved_at_ms": null,
    "resolved_by": null,
    "expires_at_ms": 1710604800000,
    "created_at_ms": 1710604200000
  }
}
```

字段说明：
- `action`：当前操作类型（`request` / `get` / `resolve`）
- `updated`：是否发生了状态变更（仅 `resolve` 可能为 `true`）
- `approval`：完整的 `ApprovalRecord` 对象

## 典型用例

### 1. 通用高风险操作审批流程

```json
// 1. 请求审批
{
  "action": "request",
  "tool_name": "apply_patch",
  "command_text": "Rewrite deployment manifests in production/",
  "risk_level": "destructive",
  "justification": "Apply reviewed release patch"
}

// 响应
{
  "action": "request",
  "updated": true,
  "approval": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "status": "pending",
    "expires_at_ms": 1710604800000
  }
}

// 2. 查询审批状态
{
  "action": "get",
  "approval_id": "550e8400-e29b-41d4-a716-446655440000"
}

// 3. 决议审批
{
  "action": "resolve",
  "approval_id": "550e8400-e29b-41d4-a716-446655440000",
  "decision": "approve",
  "actor": "senior-dev"
}
```

### 2. 过期处理

```json
// 超过 expires_at_ms 后尝试决议
{
  "action": "resolve",
  "approval_id": "550e8400-e29b-41d4-a716-446655440000",
  "decision": "approve"
}

// 响应（自动标记过期）
{
  "action": "resolve",
  "updated": true,
  "approval": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "status": "expired"
  }
}
```

### 3. 跨会话访问拦截

```json
// Session A 创建的审批，Session B 尝试访问
{
  "action": "get",
  "approval_id": "550e8400-e29b-41d4-a716-446655440000"
}

// 错误响应
{
  "error": "approval does not belong to current session"
}
```

## 测试覆盖

`klaw-tool/src/approval.rs` 当前覆盖：

- `request_creates_pending_approval` - 创建待审批记录
- `resolve_updates_approval_to_approved` - 决议更新状态为已批准
- `get_rejects_cross_session_access` - 跨会话访问拦截

## 与其他工具的协作

`approval` 工具通常与以下工具配合使用：

1. **文件编辑工具**：批量文件修改前请求审批
2. **网络工具**：外部 API 调用前的授权审批
3. **自定义高风险工具**：在执行前统一走审批记录与决议流

典型集成模式：

```rust
// 在执行高风险操作前
let approval = approval_service
    .create(ApprovalCreateInput {
        session_key: ctx.session_key.clone(),
        tool_name: "shell".to_string(),
        command_text: command.clone(),
        risk_level: Some("destructive".to_string()),
        ..Default::default()
    })
    .await?;

// 等待用户决议（通过外部渠道）
// ...

// 检查审批状态
let approval = approval_service
    .get_for_session(&ctx.session_key, &approval.id)
    .await?;

if approval.status != ApprovalStatus::Approved {
    return Err(ToolError::ExecutionFailed("approval not granted".into()));
}

// 执行实际操作
```

## 安全考虑

1. **会话隔离**：防止跨会话篡改审批记录
2. **过期控制**：避免审批记录永久有效，降低长期风险
3. **哈希校验**：`command_hash` 可用于快速比对相同命令的审批历史
4. **审计追踪**：`resolved_by` / `requested_by` 记录责任人
5. **风险分级**：`risk_level` 支持差异化审批策略（如多级审批）
