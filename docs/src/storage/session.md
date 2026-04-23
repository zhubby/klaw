# Session 存储语义

`klaw-storage` 管理 session 索引与聊天历史，索引存储在 SQLite（`klaw.db`），聊天正文写入 JSONL 文件。`klaw-session` 在此基础上提供 `SessionManager` 业务层，供 CLI 和 GUI 使用。

## 数据目录布局

`StoragePaths` 定义 `~/.klaw/` 下的目录结构：

```text
~/.klaw/
├── klaw.db            # session 索引 + cron/heartbeat/audit/approval/webhook/pending_question
├── memory.db          # long-term memory（FTS5 + vector）
├── archive.db         # archive 归档
├── config.toml
├── sessions/          # JSONL 聊天历史文件
├── archives/
├── skills/
├── skills-registry/
├── workspace/
├── logs/
└── tmp/
```

`sessions/` 目录存放每个 session 的 JSONL 文件，文件名从 `session_key` 推导：取 `:` 后半段作为 `session_id`（如 `terminal:local-chat` → `local-chat.jsonl`）。若 `session_key` 不含 `:`，则整体作为文件名。

## Session 索引字段

`sessions` 表完整字段：

| 字段 | 类型 | 说明 |
|------|------|------|
| `session_key` | TEXT PK | 会话唯一键（主键），格式 `channel:session_id` |
| `chat_id` | TEXT NOT NULL | 对话 ID |
| `channel` | TEXT NOT NULL | 来源通道（如 `terminal`、`websocket`、`webhook`） |
| `title` | TEXT | 会话标题（可选，通过 `set_session_title` 设置） |
| `active_session_key` | TEXT | 活跃子会话键（见路由状态章节） |
| `model_provider` | TEXT | 会话级模型提供商（可选） |
| `model_provider_explicit` | INTEGER NOT NULL DEFAULT 0 | `model_provider` 是否为用户显式选择 |
| `model` | TEXT | 会话级模型名称（可选） |
| `model_explicit` | INTEGER NOT NULL DEFAULT 0 | `model` 是否为用户显式选择 |
| `delivery_metadata_json` | TEXT | 投递元数据 JSON（可选） |
| `created_at_ms` | INTEGER NOT NULL | 创建时间（毫秒 epoch） |
| `updated_at_ms` | INTEGER NOT NULL | 最近更新时间（毫秒 epoch） |
| `last_message_at_ms` | INTEGER NOT NULL | 最近消息时间（毫秒 epoch） |
| `turn_count` | INTEGER NOT NULL DEFAULT 0 | 已完成轮次 |
| `jsonl_path` | TEXT NOT NULL | 对应 JSONL 文件路径（相对或绝对） |
| `compression_last_len` | INTEGER | 压缩状态：上次压缩时的历史长度 |
| `compression_summary_json` | TEXT | 压缩状态：摘要 JSON（可选） |

索引表中与压缩状态相关的两个列（`compression_last_len`、`compression_summary_json`）通过 `SessionCompressionState` 结构体读写，不直接暴露在 `SessionIndex` 中。

## 轮次规则

`turn_count` 的定义是"完整请求-响应轮次"：

- 请求发起（用户消息写入）时，不增加 `turn_count`；
- 收到并落盘完整 agent 响应后，`turn_count + 1`。

这确保 `turn_count` 表示"已完成轮次"，而不是"消息条数"。

## ChatRecord 结构

JSONL 中每行是一条 `ChatRecord`：

| 字段 | 类型 | 说明 |
|------|------|------|
| `ts_ms` | i64 | 消息时间戳 |
| `role` | String | 角色（`user` / `assistant` / `system` 等） |
| `content` | String | 消息正文 |
| `metadata_json` | String? | 附加元数据（可选，序列化时跳过空值） |
| `message_id` | String? | 消息唯一 ID（可选，用于游标分页） |

`ChatRecord::new(role, content, message_id)` 自动填充 `ts_ms`。`with_metadata_json()` 可追加元数据。

## 会话路由状态

Session 索引承载了会话级路由状态，让 IM 命令（如 `/model`、`/switch`）可以改变当前会话的行为而不影响全局配置。

### active_session_key

`active_session_key` 实现了"基础会话 → 活跃会话"的切换机制：

- 基础会话（`session_key`）是长期存在的会话容器；
- 活跃会话（`active_session_key`）指向当前正在使用的子会话；
- `get_session_by_active_session_key` 可通过活跃键反查基础会话；
- `set_active_session` 更新基础会话的活跃子会话指向；
- `get_or_create_session_state` 在首次访问时将 `active_session_key` 初始化为 `session_key` 自身。

### 模型路由

`model_provider` / `model` 字段存储会话级模型选择：

- `set_model_provider` 同时设置 provider 和 model，标记两者为显式选择（`explicit = 1`）；
- `set_model` 只改 model，标记为非显式（`explicit = 0`），表示沿用 provider 默认 model 映射；
- `clear_model_routing_override` 将两者清空并重置显式标记为 0，回到全局默认路由；
- runtime 在构建请求时通过显式标记区分用户主动选择与历史默认残留。

### delivery_metadata_json

`delivery_metadata_json` 存储投递相关的结构化元数据（如 webhook 回调地址、消息格式偏好等），通过 `set_delivery_metadata` 写入。

## 会话压缩

当聊天历史超过 `conversation_history_limit` 时，runtime 触发增量压缩：

1. `maybe_refresh_summary` 检查是否需要压缩；
2. 读取 `SessionCompressionState`（`compression_last_len` + `compression_summary_json`）；
3. 对新增消息调用 LLM 生成增量摘要，与已有摘要合并；
4. 通过 `set_session_compression_state` 持久化新的压缩状态；
5. 构建 model 输入时，用摘要替代被压缩的历史段。

`SessionCompressionState` 结构：

```text
SessionCompressionState {
    last_compressed_len: i64,   // 上次压缩时的历史长度
    summary_json: Option<String>, // 摘要 JSON（可选）
}
```

## 统一 trait

### SessionStorage（klaw-storage）

底层存储 trait，后端无关，供 runtime 和内部模块直接使用：

**会话索引与生命周期**

| 方法 | 说明 |
|------|------|
| `touch_session(session_key, chat_id, channel)` | 更新时间戳，不增加轮次；不存在则插入 |
| `complete_turn(session_key, chat_id, channel)` | `turn_count + 1`，更新时间戳 |
| `get_session(session_key)` | 读取单条 session 索引 |
| `set_session_title(session_key, title)` | 设置会话标题 |
| `delete_session(session_key)` | 删除索引行 + 对应 JSONL 文件 |
| `get_session_by_active_session_key(active_session_key)` | 通过活跃键反查基础会话 |
| `get_or_create_session_state(session_key, chat_id, channel, default_provider, default_model)` | 创建或更新会话，初始化 `active_session_key` |

**路由状态**

| 方法 | 说明 |
|------|------|
| `set_active_session(session_key, chat_id, channel, active_session_key)` | 设置活跃子会话指向 |
| `set_model_provider(session_key, chat_id, channel, model_provider, model)` | 设置会话级 provider + model（显式） |
| `set_model(session_key, chat_id, channel, model)` | 设置会话级 model（非显式） |
| `set_delivery_metadata(session_key, chat_id, channel, delivery_metadata_json)` | 设置投递元数据 |
| `clear_model_routing_override(session_key, chat_id, channel)` | 清空模型路由覆盖 |

**压缩状态**

| 方法 | 说明 |
|------|------|
| `get_session_compression_state(session_key)` | 读取压缩状态 |
| `set_session_compression_state(session_key, state)` | 写入压缩状态 |

**聊天记录**

| 方法 | 说明 |
|------|------|
| `append_chat_record(session_key, record)` | 追加一条记录到 JSONL |
| `read_chat_records(session_key)` | 读取全部聊天记录 |
| `read_chat_records_page(session_key, before_message_id, limit)` | 游标分页读取（见下文） |
| `session_jsonl_path(session_key)` | 返回 JSONL 文件路径 |

**会话列表**

| 方法 | 说明 |
|------|------|
| `list_sessions(limit, offset, updated_from_ms, updated_to_ms, channel, session_key_prefix, sort_order)` | 多条件过滤 + 排序分页 |
| `list_session_channels()` | 返回所有已有通道的 distinct 列表 |

**LLM 用量与审计**

| 方法 | 说明 |
|------|------|
| `append_llm_usage(input)` | 记录 LLM token 用量 |
| `list_llm_usage(session_key, limit, offset)` | 查询用量明细 |
| `sum_llm_usage_by_session(session_key)` | 按会话汇总用量 |
| `sum_llm_usage_by_turn(session_key, turn_index)` | 按轮次汇总用量 |
| `append_llm_audit(input)` | 记录 LLM 请求/响应审计 |
| `list_llm_audit(query)` | 查询审计记录（支持过滤 + 排序） |
| `get_llm_audit(audit_id)` | 读取单条审计 |
| `list_llm_audit_summaries(query)` | 查询审计摘要（不含大 JSON payload） |
| `list_llm_audit_filter_options(query)` | 查询审计过滤选项（session_keys + providers） |

**Tool 审计**

| 方法 | 说明 |
|------|------|
| `append_tool_audit(input)` | 记录 tool 调用审计 |
| `list_tool_audit(query)` | 查询 tool 审计（支持过滤 + 排序） |
| `list_tool_audit_filter_options(query)` | 查询 tool 审计过滤选项 |

**Webhook 事件与 Agent**

| 方法 | 说明 |
|------|------|
| `append_webhook_event(input)` | 记录 webhook 入站事件 |
| `update_webhook_event_status(event_id, update)` | 更新事件处理结果 |
| `list_webhook_events(query)` | 查询 webhook 事件 |
| `append_webhook_agent(input)` | 记录 webhook agent 触发 |
| `update_webhook_agent_status(event_id, update)` | 更新 agent 处理结果 |
| `list_webhook_agents(query)` | 查询 webhook agent |

**审批与待答问题**

| 方法 | 说明 |
|------|------|
| `create_approval(input)` | 创建审批请求 |
| `get_approval(approval_id)` | 读取审批 |
| `update_approval_status(approval_id, status, approved_by)` | 更新审批状态 |
| `consume_approved_tool_command(...)` | 消费已审批的 tool 命令 |
| `consume_latest_approved_tool_command(...)` | 消费最新已审批命令 |
| `consume_approved_shell_command(...)` | 消费已审批 shell 命令（默认 tool_name = "shell"） |
| `consume_latest_approved_shell_command(...)` | 消费最新已审批 shell 命令 |
| `create_pending_question(input)` | 创建待答问题卡片 |
| `get_pending_question(question_id)` | 读取待答问题 |
| `update_pending_question_answer(...)` | 更新待答问题答案 |

### SessionManager（klaw-session）

`SessionManager` 是业务层 trait，封装 `SessionStorage` 并对外提供更友好的接口。CLI 和 GUI 通过 `SqliteSessionManager` 使用：

- `SqliteSessionManager::open_default()` 打开默认 store；
- `SqliteSessionManager::from_store(store)` 从已有 store 构建；
- `list_sessions` 接收 `SessionListQuery` 结构体（含 limit、offset、时间范围、channel 过滤、prefix 过滤、排序方式）；
- `read_chat_records_page` 返回 `SessionHistoryPage`（含 records、has_more、oldest_message_id）。

`SessionListQuery` 字段：

| 字段 | 说明 |
|------|------|
| `limit` | 最大返回条数（可选，None 表示不限制） |
| `offset` | 分页偏移 |
| `updated_from_ms` | 时间范围起始（可选） |
| `updated_to_ms` | 时间范围截止（可选） |
| `channel` | 通道过滤（可选） |
| `session_key_prefix` | session_key 前缀过滤（可选） |
| `sort_order` | 排序方式（`UpdatedAtDesc` / `UpdatedAtAsc` / `CreatedAtDesc`） |

## 游标分页

`read_chat_records_page` 支持基于 `message_id` 的游标分页，适用于 GUI 历史面板的无限滚动：

- `before_message_id = None`：返回最新的 `limit` 条记录；
- `before_message_id = Some(id)`：返回该 ID 之前的 `limit` 条记录；
- 返回 `ChatRecordPage` / `SessionHistoryPage`，含 `has_more` 标志和 `oldest_message_id`（可用于下一页游标）；
- 内部从 JSONL 文件末尾向前倒序读取，避免全文件扫描；
- 若游标 ID 不存在，返回 `InvalidHistoryCursor` 错误。

## Runtime 写入流程

runtime 中有多条写入路径，都遵循相同的轮次语义：

### 标准交互轮次（`submit_and_get_turn_outcome`）

1. 生成 `ChatRecord`（user），追加到 JSONL；
2. 调用 `touch_session(...)` 更新时间戳（不加轮次）；
3. 读取完整历史，检查并执行压缩（`maybe_refresh_summary`）；
4. 构建对话上下文，运行 agent 请求；
5. 生成 `ChatRecord`（assistant），追加到 JSONL；
6. 调用 `complete_turn(...)`，`turn_count + 1`。

### 流式输出（`submit_and_stream_output` / `submit_and_stream_output_with_callback`）

流程同标准轮次，但 agent 响应以流式方式返回。完成后的持久化（assistant 记录 + `complete_turn`）在 `persist_assistant_response_state` 中执行。

### 历史追加轮次（`submit_history_only_turn_outcome`）

仅追加历史记录并 `touch_session`，不触发完整 agent 请求，也不增加 `turn_count`。

### 隔离轮次（`submit_isolated_turn`）

用于 sub-agent 等隔离执行场景：追加 user 记录 → `touch_session` → 执行 → 返回结果。轮次计数在隔离会话上独立维护。

### Webhook 隔离轮次（`submit_webhook_isolated_turn`）

与隔离轮次类似，channel 标记为 `webhook`。

### 投递镜像（`mirror_outbound_to_delivery_session`）

当消息需要投递到另一 session 时（如跨通道转发），在目标 session 上执行 `touch_session` + `append_chat_record`，不增加目标 session 的轮次。

### Webhook / WebSocket 入口

- `handle_event` / `handle_agent`：在 webhook 请求进入时调用 `touch_session` 初始化会话索引；
- `create_session`：WebSocket 连接建立时调用 `touch_session` 创建 session。

## 会话删除

`delete_session` 执行两步操作：

1. 从 `sessions` 表删除索引行；
2. 删除对应的 JSONL 文件（文件不存在时静默忽略）。

通过 `GatewayWebsocketHandler` 暴露给 WebSocket 客户端。

## Session 记忆复用

当前架构中，session 记忆检索分两层：

### 长期记忆（memory.db）

`klaw-memory` 使用独立的 `memory.db`（`DefaultMemoryDb`）持久化长期记忆：

- `memories` 表存储 scope、content、metadata、pinned 状态和 embedding 向量；
- `memories_fts`（FTS5）提供全文检索；
- 向量索引提供语义检索（需 Turso/libSQL 后端 + embedding provider）；
- scope 字段支持 `session:xxx` 格式，将长期记忆与 session 关联；
- 检索使用 RRF（Reciprocal Rank Fusion）融合 BM25 和向量排名。

### Session 历史检索（JSONL）

session 记忆的 **source of truth** 仍然是 chat JSONL 文件，长期记忆不复制聊天内容：

- `memory search(scope=session)` 可在 `memories` 表中检索用户主动保存的长期笔记；
- GUI 的 `search_session_history` 直接在 chat JSONL 上做文本检索，按 `base_session_key` 解析会话范围，合并 base 与 active 的聊天记录作为候选集合；
- session 历史是检索视图，不是第二套写入实体。

主要收益：

- 避免双写；
- 避免会话内容一致性问题；
- 避免为 session 记忆引入额外存储开销。

## 后端实现

`SessionStorage` 有两个后端实现，功能对等：

| 后端 | feature | 特性 | 说明 |
|------|---------|------|------|
| `SqlxSessionStore` | `sqlx` | 标准 SQLite | 通过 sqlx 异步驱动访问 `klaw.db`，适合本地单进程 |
| `TursoSessionStore` | `turso` | libSQL / Turso | 支持向量搜索和远程 Turso 连接，适合需要 embedding 的场景 |

两者共享 `jsonl` 模块的 JSONL 读写逻辑，索引操作通过各自的 SQL 驱动执行。

`DefaultSessionStore` 根据编译 feature 选择后端类型。

## CLI 读取流程

`klaw session` 子命令通过 `SqliteSessionManager`（`SessionManager` trait）读取：

- `session list`：按 `updated_at_ms DESC` 分页列出，输出包含 token 用量汇总；
- `session get --session-key`：读取单条 session 索引详情 + 用量统计。

输出字段包含 `session_key`、`chat_id`、`channel`、`turn_count`、token 用量（`total_tokens`、`input_tokens`、`output_tokens`）、时间戳和 `jsonl_path`。