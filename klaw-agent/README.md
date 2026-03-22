# klaw-agent

`klaw-agent` 负责 agent 执行主流程：上下文拼装、模型调用、工具循环与输出收敛。

## 能力

- 从配置构建模型 provider 实例。
- 组装系统提示词、历史会话与当前用户输入。
- 支持当前用户轮次同时携带媒体输入（`AgentExecutionInput.user_media`）。
- 执行工具调用循环，并将工具结果回填给模型。
- 当工具发出 `approval_required` 信号时立即停止当前工具循环，避免单轮对话重复堆积审批单。
- 在 `approval_required` 短路时保留模型原始审批提示文案，同时继续返回工具信号与请求级 usage/audit 记录。
- 输出最终文本与可选推理内容。
- 可选转发流式快照事件给 runtime/channel，用于支持逐步更新的前端或 IM channel。

## 架构

- `build_provider_from_config`：配置到 provider 的构建逻辑。
- `run_agent_execution`：单次 agent 执行循环。
- `ToolExecutor` trait：工具注册与执行边界。
- `build_provider_from_config` 支持根级 `model` 覆盖 provider 默认模型。

## 协议支持

- OpenAI-compatible provider 支持 `chat_completions` 与 `responses`。
