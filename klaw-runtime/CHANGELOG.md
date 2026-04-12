# CHANGELOG

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
