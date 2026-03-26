# Sub Agent Tool 设计与实现

本文档说明 `klaw-tool` 中 `sub_agent` 工具的设计目标、执行架构、配置策略与关键实现细节。

## 目标

- 提供一个可由父 Agent 调用的子代理工具，执行一轮委托任务直到产出最终结果。
- 子代理必须继承父 Agent 的模型能力与工具能力，避免模型自行越权选择。
- 通过共享执行内核，避免 `klaw-core` 与 `sub_agent` 逻辑分叉。

## 代码位置

- 子代理工具：`klaw-tool/src/sub_agent.rs`
- 执行内核：`klaw-agent/src/lib.rs`
- 核心 Agent 接入：`klaw-core/src/agent/agent_loop.rs`
- 运行时注册：`klaw-cli/src/commands/runtime.rs`
- 配置模型：`klaw-config/src/lib.rs`

## 执行架构

`sub_agent` 不直接依赖 `klaw-core::AgentLoop`，而是复用 `klaw-agent` 提供的执行内核：

- `run_agent_execution`：统一处理 “模型调用 -> 工具调用 -> 模型续跑” 循环。
- `ToolExecutor`：抽象工具执行接口，供 `klaw-core` 与 `klaw-tool` 各自实现。

这样可以避免 crate 循环依赖，同时保证父 Agent 与子 Agent 执行语义一致。

## 参数模型

`sub_agent` 当前参数模型聚焦在委托任务本身：

- `task`：必填，子代理任务文本。
- `context`：可选对象，用于补充委托元数据；会原样写入子执行 `metadata["sub_agent.context"]`。

不允许模型传入以下字段：

- `model_provider`
- `model`
- `max_iterations`
- `max_tool_calls`

实现上使用 `#[serde(deny_unknown_fields)]` 强制拒绝顶层多余参数，避免模型绕过约束；但不再要求模型传入运行时 `session` 标识。

## 继承策略

### 模型继承（强制）

子代理的 `provider/model` 必须从父工具上下文继承：

- `agent.provider_id`
- `agent.model`

若上下文缺失这两个键，直接返回执行错误，不允许回退到模型自选。

### 工具继承（受配置控制）

子代理工具集由父工具注册表派生：

- 默认继承父工具集合（`inherit_parent_tools = true`）。
- 通过 `exclude_tools` 过滤工具，默认排除 `sub_agent`，防止递归调用失控。

## 配置模型（`tools.sub_agent`）

位于 `klaw-config`：

```toml
[tools.sub_agent]
enabled = true
max_iterations = 6
max_tool_calls = 12
inherit_parent_tools = true
exclude_tools = ["sub_agent"]
```

说明：

- `max_iterations` / `max_tool_calls` 仅来自配置，不接受模型传参。
- 配置校验要求上限值必须大于 0。

## 上下文透传

`sub_agent` 在执行前会扩展元数据并传给子调用：

- `sub_agent.parent_session_key`
- `sub_agent.context`（完整 context 对象）
- 继承后的 `agent.provider_id` / `agent.model`

父会话键来自当前 `ToolContext.session_key`，不要求模型传参。

子会话键格式为：`{parent_session}:subagent:{uuid}`，每次委托执行都会生成新的唯一子会话作用域。

## 运行时接入

在 `klaw-cli` 运行时构建阶段：

- 当 `tools.sub_agent.enabled=true` 时注册 `SubAgentTool`。
- 同时将父工具注册表克隆后注入 `SubAgentTool`，作为子工具继承来源。

在 `klaw-core` 中：

- `AgentLoop` 会把当前 `provider/model` 写入 `ToolContext.metadata`。
- 使任何工具（尤其 `sub_agent`）都可按父上下文继承模型身份。

## 错误处理

常见错误路径：

- 参数缺失或类型不符：`ToolError::InvalidArgs`
- 缺失父模型元信息：`ToolError::ExecutionFailed`
- 子代理循环耗尽：`ToolError::ExecutionFailed`
- Provider 调用失败：`ToolError::ExecutionFailed`
- 子代理触发 `approval_required` / `stop`：通过结构化错误把信号继续透传给父 Agent，而不是在 `sub_agent` 边界吞掉

## 测试覆盖

`sub_agent` 单测已覆盖：

- `context` 可选与类型约束
- 拒绝 legacy/未知字段
- 父模型元信息继承
- 缺失父元信息报错
- child session key 唯一化
- 子执行信号透传

配置测试已覆盖：

- `tools.sub_agent` 默认值
- `max_iterations` / `max_tool_calls` 非法值校验
