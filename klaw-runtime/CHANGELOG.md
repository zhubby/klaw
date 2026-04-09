# CHANGELOG

## 2026-04-09

### Fixed

- `/approve` 现在会识别挂接到当前会话链路上的 `cron` / `webhook` execution session 审批；当 approval 绑定的是隔离执行 session 时，runtime 会依据持久化的 `channel.base_session_key` / `channel.delivery_session_key` 允许当前 IM 会话认领并继续执行批准后的 shell 命令

## 2026-04-08

### Added

- Introduced the `klaw-runtime` crate as the shared host/runtime composition layer extracted from `klaw-cli`.