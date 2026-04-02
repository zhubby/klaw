# Changelog

## 2026-04-02

### Added

- ACP `session/update` 现在会保留结构化事件快照，覆盖消息、thought、tool、plan、mode、config、available commands 与 session info 等标准更新类型
- ACP client 现在支持结构化权限请求模型和外部异步 permission handler，为 GUI 往返审批链路提供协议层基础

### Changed

- `ContentBlock` 渲染已从仅支持 `Text` 扩展为可读摘要输出，`ResourceLink` 与嵌入式 `Resource` 不再静默丢失
- ACP prompt 流式 update 现在统一透传结构化 session 事件，而不是只产出回答/thought/tool 的字符串片段

## 2026-03-30

### Added

- 新增 `klaw-acp` crate，作为 ACP 客户端集成骨架。
- 提供 ACP 配置快照、生命周期管理、tool 代理和运行时注册入口。

### Changed

- ACP agent 执行路径已接入真实 stdio `ClientSideConnection`，支持 `initialize -> new_session -> prompt` 单轮会话。
- ACP `session/update` 现在会聚合 agent 消息、thought 和 tool update，并回传为 klaw tool 输出。
- ACP client 现在支持权限请求、文本文件读写和终端创建/读取/等待/释放/终止等基础反向能力。
- ACP prompt 执行目录现在统一来自调用方传入的 `working_directory`，默认 adapter 模板对齐为 Zed 的 Claude/Codex ACP 包。
- ACP prompt 执行现在支持可选的增量 update sink，可把 `session/update` 中的回答、思考和工具状态实时转发给 GUI。
- ACP prompt 执行现在支持取消信号，GUI 可主动停止正在运行的测试 prompt。
