# 存储概述

`klaw-storage` 提供 session、cron 与 archive 相关持久化所需的路径和底层 SQLite 访问能力。

## Session 索引

核心字段：

| 字段 | 说明 |
|------|------|
| `session_key` | 会话唯一键（主键） |
| `chat_id` | 对话 ID |
| `channel` | 来源通道 |
| `created_at_ms` | 创建时间 |
| `updated_at_ms` | 最近更新时间 |
| `last_message_at_ms` | 最近消息时间 |
| `turn_count` | 已完成轮次 |
| `jsonl_path` | JSONL 文件路径 |

## Cron 任务

### cron 表（任务定义）

| 字段 | 说明 |
|------|------|
| `id` | 任务主键 |
| `name` | 任务名称 |
| `schedule_kind` | `cron` 或 `every` |
| `schedule_expr` | 表达式原文 |
| `payload_json` | 入站消息 JSON 模板 |
| `enabled` | 是否启用 |
| `next_run_at_ms` | 下一次触发时间 |
| `last_run_at_ms` | 最近一次 claim 时间 |

### cron_task 表（运行记录）

| 字段 | 说明 |
|------|------|
| `id` | 运行记录主键 |
| `cron_id` | 关联任务 ID |
| `scheduled_at_ms` | 计划触发时间 |
| `started_at_ms` | 开始执行时间 |
| `finished_at_ms` | 完成时间 |
| `status` | `pending/running/success/failed` |
| `error_message` | 失败原因 |
| `published_message_id` | 成功发布的消息 ID |

## Archive 归档

### archives 表

| 字段 | 说明 |
|------|------|
| `id` | 归档记录主键 |
| `source_kind` | 来源类型 |
| `media_kind` | 识别后的媒体类别 |
| `content_sha256` | 内容哈希 |
| `size_bytes` | 文件大小 |
| `storage_rel_path` | 相对存储路径 |
| `session_key` | 关联会话键 |
| `chat_id` | 关联对话 ID |
| `metadata_json` | 扩展元数据 |
| `created_at_ms` | 写入时间 |

## 索引优化

- `cron(enabled, next_run_at_ms)` - 加速到期任务扫描
- `cron_task(cron_id, created_at_ms DESC)` - 按任务查历史
- `cron_task(status, scheduled_at_ms)` - 按状态和时间过滤
- `archives(content_sha256)` - 加速按内容哈希查重
- `archives(session_key)` / `archives(chat_id)` - 加速按上下文检索归档

## 后端实现

- `turso` 后端：使用 libSQL，支持向量搜索（FTS5 + vector）
- `sqlx` 后端：使用标准 SQLite，功能对等

详细文档：
- [Session 存储语义](./session.md)
- [Cron 存储语义](./cron.md)
- [Archive 存储语义](./archive.md)
