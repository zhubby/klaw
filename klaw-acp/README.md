# klaw-acp

`klaw-acp` 为 klaw 提供 Agent Client Protocol (ACP) 客户端侧集成能力。

当前已实现的能力：

- ACP agent 配置模型快照
- 外部 ACP agent 生命周期管理与 tool 注册
- 基于 stdio 的 `ClientSideConnection` 一次性会话执行
- `initialize -> new_session -> prompt` 单轮调用链路
- `session/update` 聚合为最终 tool 输出
- ACP 反向能力中的权限确认、文本文件读写和终端代理
- ACP agent 到 `klaw_tool::Tool` 的代理注册

实现继续沿用 `klaw-mcp` 的 `manager + hub + proxy tool` 结构，但执行模型采用“每次 tool 调用启动一个短生命周期 ACP 会话”，优先保证 Claude Code、Codex CLI 这类外部 ACP agent 能被 klaw 真实调起和代理。

默认推荐通过 Zed 的 ACP adapter 包启动外部 agent，例如 `npx -y @zed-industries/claude-agent-acp` 与 `npx -y @zed-industries/codex-acp`。运行目录不再由 agent 配置固定，而是由每次调用传入的 `working_directory` 决定。
