# Cron 存储语义

`klaw-cron` 与 `klaw-storage` 配合实现定时任务的持久化调度。触发目标是发布到 `agent.inbound`，执行链路复用现有 runtime。

## 表结构

## `cron`（任务定义）

- `id`：任务主键
- `name`：任务名称
- `schedule_kind`：调度类型（`cron` 或 `every`）
- `schedule_expr`：表达式原文
- `payload_json`：入站消息 JSON 模板（可反序列化为 `InboundMessage`）
- `enabled`：是否启用
- `timezone`：时区（当前默认 `UTC`）
- `next_run_at_ms`：下一次触发时间
- `last_run_at_ms`：最近一次已被 claim 的计划时间
- `created_at_ms` / `updated_at_ms`：审计字段

## `cron_task`（运行记录）

- `id`：运行记录主键
- `cron_id`：关联任务 ID
- `scheduled_at_ms`：该次计划触发时间
- `started_at_ms` / `finished_at_ms`：执行起止时间
- `status`：`pending` / `running` / `success` / `failed`
- `attempt`：重试计数（当前首版默认 0）
- `error_message`：失败原因
- `published_message_id`：成功发布后的消息 ID
- `created_at_ms`：记录创建时间

## 索引

- `cron(enabled, next_run_at_ms)`：加速扫描到期任务
- `cron_task(cron_id, created_at_ms DESC)`：按任务查询历史
- `cron_task(status, scheduled_at_ms)`：按状态和计划时间过滤

## 并发防重（CAS）

worker 处理到期任务时不会直接执行，而是先调用：

- `claim_next_run(cron_id, expected_next_run_at_ms, new_next_run_at_ms, now_ms)`

该操作在数据库侧执行条件更新（`WHERE next_run_at_ms = expected...`），只有一个 worker 能成功 claim，避免同一触发时刻重复执行。

## 状态流转

单次运行典型过程：

1. `append_task_run(..., status=pending)`
2. `mark_task_running(...)`
3. 发布到 `agent.inbound`
4. 成功：`mark_task_result(..., status=success, published_message_id=...)`
5. 失败：`mark_task_result(..., status=failed, error_message=...)`

## 后端一致性

`turso` 与 `sqlx` 两个后端都实现了相同的 `CronStorage` trait，并在初始化时自动建表与建索引，保持上层行为一致。
