# Changelog

## 2026-03-30

### Added

- 新增 `klaw-acp` crate，作为 ACP 客户端集成骨架。
- 提供 ACP 配置快照、生命周期管理、tool 代理和运行时注册入口。

### Changed

- ACP agent 执行路径已接入真实 stdio `ClientSideConnection`，支持 `initialize -> new_session -> prompt` 单轮会话。
- ACP `session/update` 现在会聚合 agent 消息、thought 和 tool update，并回传为 klaw tool 输出。
- ACP client 现在支持权限请求、文本文件读写和终端创建/读取/等待/释放/终止等基础反向能力。
- ACP prompt 执行目录现在统一来自调用方传入的 `working_directory`，默认 adapter 模板对齐为 Zed 的 Claude/Codex ACP 包。
