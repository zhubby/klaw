# CHANGELOG

## 2026-04-14

### Changed

- `ApprovalManager` 的消费接口已从 shell 专用流程扩展为按 `tool_name + session_key + command_hash` 的通用审批消费模型，`consume_approval` 也不再限制为只处理 shell 记录

## 2026-03-16

### Added

- initial approval manager crate with `ApprovalManager` trait, list/query support, lifecycle resolution, and shell approval consume operations
