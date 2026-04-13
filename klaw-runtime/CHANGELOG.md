# CHANGELOG

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
