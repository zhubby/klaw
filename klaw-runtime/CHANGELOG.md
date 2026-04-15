# CHANGELOG

## 2026-04-15

### Changed

- IM callback commands now opt into isolated execution sessions instead of resuming directly inside the routed chat session, aligning callback turns with cron/webhook-style one-shot execution semantics

### Fixed

- `/approve` follow-up resumes triggered from isolated IM callbacks now replay approval context into a fresh `callback:*` execution session, so approval-triggered turns no longer append into the live IM chat history
- `/card_answer` follow-ups triggered from isolated IM callbacks now execute from a fresh `callback:*` session built from the source conversation history, so callback answers no longer pollute the active chat transcript
- websocket 会话现在和 telegram / dingtalk 一样会自动同步创建 session-bound heartbeat；gateway `session.create` 不再因为绕过常规路由初始化而漏掉 heartbeat 绑定

## 2026-04-14

### Changed

- runtime gateway websocket handler 现在会把 `session.submit.attachments` 转成标准 `media_references` 注入 inbound turn，使浏览器一次提交的多个上传文件都能作为当前消息附件进入 agent loop
- runtime 注册本地工具时，`apply_patch` 现在会像 `shell` 一样复用共享 session store 注入审批管理器，使越权补丁请求能进入统一的审批流并在批准后重试执行

### Fixed

- `/approve` 现在会优先按触发审批的 `tool_audit` 重放原始 tool call，并把已批准工具的实际结果作为结构化 tool 历史交回模型；shell 与其它接入审批的工具都不再依赖 prompt 式 follow-up 或 runtime 侧第二轮强制重试

### Changed

- runtime 持久化 assistant 聊天历史时，现会同时写入 outbound metadata 与 assistant `message_id`，使 websocket/webui 客户端可以在刷新后恢复 `im.card` 等结构化消息状态
- gateway websocket 历史加载现在会解析并回传持久化的聊天 metadata，而不再把历史 assistant 消息降级为纯文本
- runtime gateway websocket handler 现在按页读取会话历史，并把无效历史游标映射为 websocket `invalid_request` 错误而不是全量读取后静默截断

## 2026-04-13

### Fixed

- 后台 `cron` / 其他隔离执行产生的 outbound 消息现在会镜像写回 `channel.delivery_session_key`（或 base session）对应的会话历史，terminal 与 websocket 会话在重新打开时不再丢失这些后台回复
- runtime 后台 outbound dispatcher 现在支持 `websocket` channel，会把隔离执行的 assistant 回复按目标 session 广播给当前订阅该会话的浏览器 websocket 客户端
- cron / heartbeat / webhook 在进入 agent loop 前会先校验目标 channel 是否仍然 enabled；若目标 channel 已 disabled，则仅输出 debug 日志并跳过，不再继续执行后台 agent turn

## 2026-04-12

### Fixed

- gateway websocket 的 `session.subscribe` 读取历史时，现在会先解析订阅 session 的 `active_session_key`；当 base session 已派生到 active child 时，webui 打开窗口会加载 active session 的历史，而不再误读 base session 的旧历史
- runtime 生成和识别浏览器 websocket 会话时，现已统一使用 `websocket:` 前缀与 `websocket` channel 名称，不再混用旧的 `web:` / `web`

## 2026-04-09

### Fixed

- `/approve` 现在会识别挂接到当前会话链路上的 `cron` / `webhook` execution session 审批；当 approval 绑定的是隔离执行 session 时，runtime 会依据持久化的 `channel.base_session_key` / `channel.delivery_session_key` 允许当前 IM 会话认领并继续执行批准后的 shell 命令

## 2026-04-08

### Added

- Introduced the `klaw-runtime` crate as the shared host/runtime composition layer extracted from `klaw-cli`.
