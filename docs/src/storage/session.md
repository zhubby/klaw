# Session 存储语义

首版 `klaw-storage` 只管理 session 索引，聊天内容不入库，正文写入 JSONL。

## Session 索引字段

`sessions` 表核心字段：

- `session_key`：会话唯一键（主键）
- `chat_id`：对话 ID
- `channel`：来源通道（如 `stdio`）
- `created_at_ms`：创建时间
- `updated_at_ms`：最近更新时间
- `last_message_at_ms`：最近消息时间
- `turn_count`：已完成轮次
- `jsonl_path`：对应 JSONL 文件路径

## 轮次规则

`turn_count` 的定义是“完整请求-响应轮次”：

- 请求发起（用户消息写入）时，不增加 `turn_count`；
- 收到并落盘完整 agent 响应后，`turn_count + 1`。

这确保 `turn_count` 表示“已完成轮次”，而不是“消息条数”。

## 统一 trait

`SessionStorage` 对外暴露统一接口（后端无关）：

- `touch_session(session_key, chat_id, channel)`
- `complete_turn(session_key, chat_id, channel)`
- `append_chat_record(session_key, record)`
- `get_session(session_key)`
- `list_sessions(limit, offset)`
- `session_jsonl_path(session_key)`

## Runtime 写入流程

在 `submit_and_get_output(...)` 路径中：

1. 生成并追加用户消息到 JSONL；
2. 调用 `touch_session(...)` 更新时间戳（不加轮次）；
3. 运行 agent 请求；
4. 生成并追加 assistant 响应到 JSONL；
5. 调用 `complete_turn(...)`，执行 `turn_count + 1`。

## CLI 读取流程

`klaw session` 子命令直接通过 `SessionStorage` 读取索引：

- `session list`：按 `updated_at_ms DESC` 分页列出；
- `session get --session-key`：读取单条 session 索引详情。
