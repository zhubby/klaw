# 存储概述

`klaw-storage` 提供本地数据目录布局管理、SQLite 访问能力和多个持久化 trait；`klaw-archive` 和 `klaw-memory` 分别在独立的数据库文件上管理归档索引和长期记忆。

## 数据目录布局

`StoragePaths` 定义 `~/.klaw/` 下的完整目录结构：

```text
~/.klaw/
├── klaw.db                # 主数据库（session + 辅助表）
├── memory.db              # 长期记忆（FTS5 + vector）
├── knowledge.db           # 知识索引（FTS5 + vector + 图链接）
├── archive.db             # 归档索引
├── config.toml            # 全局配置
├── settings.json          # GUI 设置
├── gui_state.json         # GUI 状态
├── tmp/                   # 临时数据
├── sessions/              # JSONL 聊天历史文件
│   └── <session_id>.jsonl
├── archives/              # 归档物理文件
│   └── YYYY-MM-DD/
│       └── <uuid>.<ext>
├── models/                # 本地模型存储
│   ├── manifest.json      # 全局模型索引
│   ├── snapshots/         # 模型文件实体（GGUF 等）
│   └── cache/downloads/   # 下载临时文件
├── skills/                # 技能定义
├── skills-registry/       # 技能注册表
├── workspace/             # 工作区（copy_to_workspace 目标）
└── logs/                  # 日志文件
```

四个 SQLite 数据库和一个 JSON 索引各司其职：

| 数据库 / 索引 | 责任模块 | 主要内容 |
|---------------|---------|---------|
| `klaw.db` | `klaw-storage` | session 索引 + 辅助表（audit、cron、heartbeat、approval 等） |
| `memory.db` | `klaw-memory` | 长期记忆记录 + FTS5 全文检索 + 向量索引 |
| `knowledge.db` | `klaw-knowledge` | 知识条目 + 分块 + FTS5 + 向量索引 + 链接图谱 |
| `archive.db` | `klaw-archive` | 归档文件索引 |
| `models/manifest.json` | `klaw-model` | 已安装模型索引 + 文件列表 + 能力标注 + 绑定信息 |

## klaw.db 表结构

主数据库包含 `sessions` 表和 9 个辅助表：

### sessions（会话索引）

会话索引是 `SessionStorage` trait 的核心数据，聊天正文存储在 JSONL 文件中：

| 字段 | 类型 | 说明 |
|------|------|------|
| `session_key` | TEXT PK | 会话唯一键（主键），格式 `channel:session_id` |
| `chat_id` | TEXT NOT NULL | 对话 ID |
| `channel` | TEXT NOT NULL | 来源通道（`terminal`、`websocket`、`webhook`） |
| `title` | TEXT | 会话标题（可选） |
| `active_session_key` | TEXT | 活跃子会话键（路由状态） |
| `model_provider` | TEXT | 会话级模型提供商（可选） |
| `model_provider_explicit` | INTEGER NOT NULL DEFAULT 0 | provider 是否为用户显式选择 |
| `model` | TEXT | 会话级模型名称（可选） |
| `model_explicit` | INTEGER NOT NULL DEFAULT 0 | model 是否为用户显式选择 |
| `delivery_metadata_json` | TEXT | 投递元数据（可选） |
| `created_at_ms` | INTEGER NOT NULL | 创建时间 |
| `updated_at_ms` | INTEGER NOT NULL | 最近更新时间 |
| `last_message_at_ms` | INTEGER NOT NULL | 最近消息时间 |
| `turn_count` | INTEGER NOT NULL DEFAULT 0 | 已完成轮次 |
| `jsonl_path` | TEXT NOT NULL | JSONL 文件路径 |
| `compression_last_len` | INTEGER | 压缩状态：上次压缩时历史长度 |
| `compression_summary_json` | TEXT | 压缩状态：摘要 JSON |

### llm_usage（LLM token 用量）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | 用量记录 ID |
| `session_key` | TEXT NOT NULL | 关联会话 |
| `chat_id` | TEXT NOT NULL | 关联对话 |
| `turn_index` | INTEGER NOT NULL | 轮次索引 |
| `request_seq` | INTEGER NOT NULL | 请求序号 |
| `provider` | TEXT NOT NULL | LLM 提供商 |
| `model` | TEXT NOT NULL | 模型名称 |
| `wire_api` | TEXT NOT NULL | API 类型 |
| `input_tokens` | INTEGER NOT NULL | 输入 token 数 |
| `output_tokens` | INTEGER NOT NULL | 输出 token 数 |
| `total_tokens` | INTEGER NOT NULL | 总 token 数 |
| `cached_input_tokens` | INTEGER | 缓存输入 token |
| `reasoning_tokens` | INTEGER | 推理 token |
| `source` | TEXT NOT NULL | 来源（`provider_reported` / `estimated_local`） |
| `provider_request_id` | TEXT | 提供商请求 ID |
| `provider_response_id` | TEXT | 提供商响应 ID |
| `created_at_ms` | INTEGER NOT NULL | 创建时间 |

### llm_audit（LLM 请求/响应审计）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | 审计记录 ID |
| `session_key` | TEXT NOT NULL | 关联会话 |
| `chat_id` | TEXT NOT NULL | 关联对话 |
| `turn_index` / `request_seq` | INTEGER NOT NULL | 轮次 + 序号 |
| `provider` / `model` / `wire_api` | TEXT NOT NULL | 模型路由信息 |
| `status` | TEXT NOT NULL | `success` / `failed` |
| `error_code` / `error_message` | TEXT | 错误信息 |
| `provider_request_id` / `provider_response_id` | TEXT | 提供商追踪 ID |
| `request_body_json` | TEXT NOT NULL | 完整请求体 |
| `response_body_json` | TEXT | 完整响应体 |
| `metadata_json` | TEXT | 执行上下文元数据（如子 agent lineage） |
| `requested_at_ms` / `responded_at_ms` | INTEGER | 请求/响应时间 |
| `created_at_ms` | INTEGER NOT NULL | 创建时间 |

### tool_audit（Tool 调用审计）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | 审计记录 ID |
| `session_key` / `chat_id` | TEXT NOT NULL | 关联会话/对话 |
| `turn_index` / `request_seq` / `tool_call_seq` | INTEGER NOT NULL | 定位序号 |
| `tool_name` | TEXT NOT NULL | 工具名称 |
| `status` | TEXT NOT NULL | `success` / `failed` |
| `error_code` / `error_message` | TEXT | 错误信息 |
| `retryable` / `approval_required` | INTEGER | 标记 |
| `arguments_json` | TEXT NOT NULL | 工具调用参数 |
| `result_content` | TEXT | 工具返回内容 |
| `error_details_json` / `signals_json` / `metadata_json` | TEXT | 详细信息 |
| `started_at_ms` / `finished_at_ms` / `created_at_ms` | INTEGER | 时间戳 |

### webhook_events（Webhook 入站事件）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | 事件 ID |
| `source` / `event_type` | TEXT NOT NULL | 来源与事件类型 |
| `session_key` / `chat_id` / `sender_id` | TEXT NOT NULL | 会话上下文 |
| `content` | TEXT NOT NULL | 事件内容 |
| `payload_json` / `metadata_json` | TEXT | 原始数据 |
| `status` | TEXT NOT NULL | `accepted` / `processed` / `failed` |
| `error_message` / `response_summary` | TEXT | 处理结果 |
| `received_at_ms` / `processed_at_ms` | INTEGER | 时间戳 |
| `remote_addr` | TEXT | 来源 IP |
| `created_at_ms` | INTEGER NOT NULL | 创建时间 |

### webhook_agents（Webhook Agent 触发）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | 触发 ID |
| `hook_id` | TEXT NOT NULL | Hook 配置 ID |
| `session_key` / `chat_id` / `sender_id` | TEXT NOT NULL | 会话上下文 |
| `content` | TEXT NOT NULL | 触发内容 |
| `payload_json` / `metadata_json` | TEXT | 原始数据 |
| `status` | TEXT NOT NULL | 处理状态 |
| `error_message` / `response_summary` | TEXT | 处理结果 |
| `received_at_ms` / `processed_at_ms` / `remote_addr` | TEXT/INTEGER | 来源信息 |
| `created_at_ms` | INTEGER NOT NULL | 创建时间 |

### pending_questions（待答问题卡片）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | 问题 ID |
| `session_key` / `channel` / `chat_id` | TEXT NOT NULL | 会话上下文 |
| `title` | TEXT | 问题标题 |
| `question_text` | TEXT NOT NULL | 问题正文 |
| `options_json` | TEXT NOT NULL | 选项列表 JSON |
| `status` | TEXT NOT NULL | `pending` / `answered` / `expired` |
| `selected_option_id` / `answered_by` / `answered_at_ms` | TEXT/INTEGER | 答案信息 |
| `expires_at_ms` | INTEGER | 过期时间 |
| `created_at_ms` / `updated_at_ms` | INTEGER NOT NULL | 审计字段 |

### cron（定时任务定义）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | 任务 ID |
| `name` | TEXT NOT NULL | 任务名称 |
| `schedule_kind` | TEXT NOT NULL | `cron` 或 `every` |
| `schedule_expr` | TEXT NOT NULL | 表达式原文 |
| `payload_json` | TEXT NOT NULL | 入站消息模板 |
| `enabled` | INTEGER NOT NULL DEFAULT 1 | 是否启用 |
| `timezone` | TEXT NOT NULL DEFAULT 'UTC' | 时区 |
| `next_run_at_ms` | INTEGER NOT NULL | 下次触发时间 |
| `last_run_at_ms` | INTEGER | 最近 claim 时间 |
| `created_at_ms` / `updated_at_ms` | INTEGER NOT NULL | 审计字段 |

### cron_task（Cron 运行记录）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | 运行记录 ID |
| `cron_id` | TEXT NOT NULL | 关联任务 |
| `scheduled_at_ms` | INTEGER NOT NULL | 计划触发时间 |
| `started_at_ms` / `finished_at_ms` | INTEGER | 执行起止 |
| `status` | TEXT NOT NULL | `pending` / `running` / `success` / `failed` |
| `attempt` | INTEGER NOT NULL DEFAULT 0 | 重试计数 |
| `error_message` | TEXT | 失败原因 |
| `published_message_id` | TEXT | 成功发布的消息 ID |
| `created_at_ms` | INTEGER NOT NULL | 创建时间 |

### heartbeat（心跳任务定义）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | 心跳 ID |
| `session_key` | TEXT NOT NULL UNIQUE | 关联会话 |
| `channel` / `chat_id` | TEXT NOT NULL | 通道与对话 |
| `enabled` | INTEGER NOT NULL DEFAULT 1 | 是否启用 |
| `every` | TEXT NOT NULL | 间隔表达式 |
| `prompt` | TEXT NOT NULL | 周期提示词 |
| `silent_ack_token` | TEXT NOT NULL | 静默确认令牌 |
| `recent_messages_limit` | INTEGER NOT NULL DEFAULT 12 | 继承上下文窗口 |
| `timezone` | TEXT NOT NULL DEFAULT 'UTC' | 时区 |
| `next_run_at_ms` / `last_run_at_ms` | INTEGER | 调度时间 |
| `created_at_ms` / `updated_at_ms` | INTEGER NOT NULL | 审计字段 |

### heartbeat_task（心跳运行记录）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | 运行记录 ID |
| `heartbeat_id` | TEXT NOT NULL | 关联心跳任务 |
| `scheduled_at_ms` / `started_at_ms` / `finished_at_ms` | INTEGER | 时间戳 |
| `status` | TEXT NOT NULL | `pending` / `running` / `success` / `failed` |
| `attempt` | INTEGER NOT NULL DEFAULT 0 | 重试计数 |
| `error_message` / `published_message_id` | TEXT | 结果信息 |
| `created_at_ms` | INTEGER NOT NULL | 创建时间 |

### approvals（审批请求）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | 审批 ID |
| `session_key` / `tool_name` | TEXT NOT NULL | 会话与工具 |
| `command_hash` / `command_preview` / `command_text` | TEXT NOT NULL | 命令信息 |
| `risk_level` | TEXT NOT NULL | 风险等级 |
| `status` | TEXT NOT NULL | `pending` / `approved` / `rejected` / `expired` / `consumed` |
| `requested_by` / `approved_by` / `justification` | TEXT | 人员信息 |
| `expires_at_ms` | INTEGER | 过期时间 |
| `created_at_ms` / `updated_at_ms` / `consumed_at_ms` | INTEGER | 时间戳 |

## klaw.db 索引

### sessions 索引

| 索引 | 用途 |
|------|------|
| `idx_sessions_updated_at_ms` (DESC) | 按更新时间排序/分页 |

### llm_usage 索引

| 索引 | 用途 |
|------|------|
| `idx_llm_usage_session_created` | 按会话 + 时间查询用量 |
| `idx_llm_usage_chat_created` | 按对话 + 时间查询用量 |
| `idx_llm_usage_session_turn` | 按会话 + 轮次汇总用量 |

### llm_audit 索引

| 索引 | 用途 |
|------|------|
| `idx_llm_audit_session_requested` | 按会话查询审计 |
| `idx_llm_audit_provider_requested` | 按提供商查询审计 |
| `idx_llm_audit_requested` | 全局按时间排序 |
| `idx_llm_audit_session_turn` | 按会话 + 轮次定位 |

### tool_audit 索引

| 索引 | 用途 |
|------|------|
| `idx_tool_audit_tool_started` | 按工具名 + 时间查询 |
| `idx_tool_audit_session_started` | 按会话 + 时间查询 |
| `idx_tool_audit_session_turn` | 按会话 + 轮次 + 调用序号定位 |

### webhook_events 索引

| 索引 | 用途 |
|------|------|
| `idx_webhook_events_received` | 按接收时间排序 |
| `idx_webhook_events_source_received` | 按来源 + 时间过滤 |
| `idx_webhook_events_status_received` | 按状态 + 时间过滤 |
| `idx_webhook_events_session_received` | 按会话 + 时间查询 |

### webhook_agents 索引

| 索引 | 用途 |
|------|------|
| `idx_webhook_agents_received` | 按接收时间排序 |
| `idx_webhook_agents_hook_received` | 按 hook ID + 时间过滤 |
| `idx_webhook_agents_status_received` | 按状态 + 时间过滤 |
| `idx_webhook_agents_session_received` | 按会话 + 时间查询 |

### pending_questions 索引

| 索引 | 用途 |
|------|------|
| `idx_pending_questions_session_status` | 按会话 + 状态查询 |
| `idx_pending_questions_expiry` | 按状态 + 过期时间扫描 |

### cron 索引

| 索引 | 用途 |
|------|------|
| `idx_cron_enabled_next_run` | 加速到期任务扫描 |
| `idx_cron_task_cron_created` | 按任务查历史 |
| `idx_cron_task_status_scheduled` | 按状态和时间过滤 |

### heartbeat 索引

| 索引 | 用途 |
|------|------|
| `idx_heartbeat_enabled_next_run` | 加速到期心跳扫描 |
| `idx_heartbeat_task_heartbeat_created` | 按心跳任务查历史 |
| `idx_heartbeat_task_status_scheduled` | 按状态和时间过滤 |

### approvals 索引

| 索引 | 用途 |
|------|------|
| `idx_approvals_session_status` | 按会话 + 状态查询审批 |

## memory.db 表结构

`klaw-memory` 的 `SqliteMemoryService` 使用独立的 `memory.db`（通过 `DefaultMemoryDb` 提供 SQL 接口）：

### memories（长期记忆记录）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | 记录 ID |
| `scope` | TEXT NOT NULL | 作用域（如 `session:xxx`、`global`） |
| `content` | TEXT NOT NULL | 记忆内容 |
| `metadata_json` | TEXT NOT NULL | 结构化元数据 |
| `pinned` | INTEGER NOT NULL DEFAULT 0 | 是否置顶 |
| `embedding` | BLOB | 向量嵌入（可选） |
| `created_at_ms` | INTEGER NOT NULL | 创建时间 |
| `updated_at_ms` | INTEGER NOT NULL | 更新时间 |

### memories_fts（FTS5 全文检索虚拟表）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT UNINDEXED | 关联 memories.id |
| `content` | TEXT | 全文检索内容 |

### 向量索引

```text
idx_memories_embedding  — libsql_vector_idx(embedding)
```

检索使用 RRF（Reciprocal Rank Fusion）融合 BM25 全文排名和向量语义排名，需要 Turso/libSQL 后端 + embedding provider。

## archive.db 表结构

`klaw-archive` 的 `SqliteArchiveService` 使用独立的 `archive.db`（通过 `DefaultArchiveDb` 提供 SQL 接口）：

### archives（归档文件索引）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | 归档记录 ID（UUID v4） |
| `source_kind` | TEXT NOT NULL | 来源（`user_upload` / `channel_inbound` / `model_generated`） |
| `media_kind` | TEXT NOT NULL | 媒体类别（`pdf` / `image` / `video` / `audio` / `other`） |

| `mime_type` | TEXT | 魔数推断的 MIME 类型 |
| `extension` | TEXT | 落盘扩展名 |
| `original_filename` | TEXT | 原始文件名 |
| `content_sha256` | TEXT NOT NULL | 内容哈希（去重依据） |
| `size_bytes` | INTEGER NOT NULL | 文件大小 |
| `storage_rel_path` | TEXT NOT NULL | 相对存储路径 |
| `session_key` | TEXT | 关联会话 |
| `channel` | TEXT | 来源通道 |
| `chat_id` | TEXT | 关联对话 |
| `message_id` | TEXT | 来源消息 ID |
| `metadata_json` | TEXT NOT NULL | 扩展元数据 |
| `created_at_ms` | INTEGER NOT NULL | 归档时间 |

### archives 索引

| 索引 | 用途 |
|------|------|
| `idx_archives_created_at_ms` (DESC) | 按时间排序/分页 |
| `idx_archives_content_sha256` | 去重查找 |
| `idx_archives_session_key` | 按会话检索 |
| `idx_archives_chat_id` | 按对话检索 |
| `idx_archives_source_kind` | 按来源过滤 |
| `idx_archives_media_kind` | 按媒体类别过滤 |

## 存储 trait 关系

各 crate 定义了后端无关的存储 trait，`klaw-storage` 提供底层 SQL 驱动：

| trait | 所属 crate | 后端实现 | 数据库 |
|-------|-----------|---------|--------|
| `SessionStorage` | `klaw-storage` | `SqlxSessionStore` / `TursoSessionStore` | `klaw.db` |
| `CronStorage` | `klaw-storage` | `SqlxSessionStore` / `TursoSessionStore` | `klaw.db` |
| `HeartbeatStorage` | `klaw-storage` | `SqlxSessionStore` / `TursoSessionStore` | `klaw.db` |
| `ArchiveService` | `klaw-archive` | `SqliteArchiveService` | `archive.db` |
| `MemoryService` | `klaw-memory` | `SqliteMemoryService` | `memory.db` |
| `KnowledgeProvider` | `klaw-knowledge` | `ObsidianKnowledgeProvider` | `knowledge.db` |
| `ModelService` | `klaw-model` | `ModelStorage`（manifest.json）+ `ModelLlamaRuntime` | `~/.klaw/models/` |
| `DatabaseExecutor` | `klaw-storage` | `DefaultMemoryDb` / `DefaultKnowledgeDb` / `DefaultArchiveDb` | `memory.db` / `knowledge.db` / `archive.db` |

`SessionStorage` 是最大的 trait，涵盖会话索引、聊天记录、LLM 用量/审计、Tool 审计、Webhook 事件/Agent、审批和待答问题——这些辅助数据表都存储在同一个 `klaw.db` 中，与 `sessions` 表共享连接池。

`klaw-session` 的 `SessionManager` 和 `SqliteSessionManager` 是业务层封装，内部委托 `SessionStorage`。

`klaw-cron` 的 `CronWorker` 和 `klaw-heartbeat` 的 `HeartbeatWorker` 分别需要 `CronStorage + SessionStorage` 和 `HeartbeatStorage + SessionStorage` 的组合约束，因为调度执行需要读取会话路由状态。

## 后端实现

`klaw-storage` 的 `SqlxSessionStore` / `TursoSessionStore` 通过编译 feature 切换：

| 后端 | feature | 特性 | 适用场景 |
|------|---------|------|---------|
| `SqlxSessionStore` | `sqlx` | 标准 SQLite（sqlx 异步驱动） | 本地单进程 |
| `TursoSessionStore` | `turso` | libSQL / Turso（支持 FTS5 + vector） | 需要 embedding 或远程 Turso |

两者功能对等，共享 `jsonl` 模块的 JSONL 读写逻辑。`DefaultSessionStore` 根据编译 feature 选择后端类型。

`DefaultMemoryDb` 和 `DefaultArchiveDb` 同样通过 feature 选择底层驱动，但 `memory.db` 的向量索引只在 Turso/libSQL 后端下可用。

## 备份服务

`BackupService`（`klaw-storage`）提供 `klaw.db` + `memory.db` + `archive.db` + 文件系统的备份/恢复：

- 版本化 manifest + 内容寻址 blob 上传到 S3 兼容存储；
- `latest.json` 作为当前 manifest 引用，`manifests/<id>.json` 保留历史；
- 支持进度事件（reconciliation、manifest preparation、blob upload、publish、retention cleanup）；
- 保留策略清理旧 manifest 和不再引用的 blob；
- 支持直接凭证或环境变量间接引用的 S3 配置（兼容 R2 等自定义端点）。

## 详细文档

- [Session 存储语义](./session.md) — 会话索引、路由状态、压缩、ChatRecord、写入流程
- [Cron 存储语义](./cron.md) — 调度类型、CAS 防重、CronWorker、GUI 面板
- [Archive 存储语义](./archive.md) — 归档索引、去重、文件识别、ArchiveTool/VoiceTool 集成
- [Memory 存储语义](./memory.md) — 长期记忆 CRUD、混合检索、写入治理、prompt 渲染、统计聚合
- [Knowledge 存储语义](./knowledge.md) — Obsidian vault 索引、五通道融合检索、Smart Chunking、链接图谱、上下文组装
