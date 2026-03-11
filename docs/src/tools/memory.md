# Memory Tool 设计与实现

本文档记录 `klaw-tool` 中 `memory` 工具的设计目标、参数收敛策略、配置模型与实现细节。

## 目标

- 提供长期记忆能力给大模型，用于跨轮次保留关键事实。
- 仅暴露高价值、低歧义能力，降低模型参数选择复杂度。
- 将检索策略从工具参数迁移到配置层，保证行为稳定可控。

## 代码位置

- 工具实现：`klaw-tool/src/memory.rs`
- 记忆服务：`klaw-memory/src/lib.rs`
- 配置结构：`klaw-config/src/lib.rs`
- 运行时注册：`klaw-cli/src/commands/runtime.rs`

## 能力边界

当前 `memory` 工具仅支持两种 action：

- `add`：新增记忆
- `search`：检索当前会话范围内的记忆

不再对模型暴露 `get/delete/pin`，避免让模型在不必要分支上做选择。

## Tool Metadata（面向 LLM）

`parameters` 仅保留最小必要字段：

- `action`：`add` 或 `search`
- `content`：`add` 必填
- `metadata`：`add` 可选
- `pinned`：`add` 可选
- `query`：`search` 必填

并通过 schema 的 `oneOf` 强约束 action 对应必填：

- `action=add` 必须包含 `content`
- `action=search` 必须包含 `query`

已移除模型侧策略字段：

- `scope`
- `limit`
- `fts_limit`
- `vector_limit`
- `use_vector`

## 自动回退策略

### 作用域回退

- `add` 自动写入 `ctx.session_key` 作用域。
- `search` 自动在 `ctx.session_key` 作用域检索。

模型不需要也不能指定 scope，减少误用。

### 检索策略回退

检索策略来自 `tools.memory` 配置，而不是模型参数。

```toml
[tools.memory]
enabled = true
search_limit = 8
fts_limit = 20
vector_limit = 20
use_vector = true
```

实现中会进行上限保护（最大 50），并在配置校验阶段保证值大于 0。

## 配置模型

`klaw-config` 中新增：

- `tools.memory.enabled`：是否注册 memory tool（默认 `true`）
- `tools.memory.search_limit`：返回结果上限
- `tools.memory.fts_limit`：BM25 召回候选池
- `tools.memory.vector_limit`：向量召回候选池
- `tools.memory.use_vector`：是否启用向量召回

校验规则：

- `search_limit > 0`
- `fts_limit > 0`
- `vector_limit > 0`

## 运行时接入

在 `build_runtime_bundle` 中按开关注册：

- `tools.memory.enabled = true` 时注册 `MemoryTool`
- 否则不注册，模型看不到该工具定义

## 错误处理

- 参数缺失/类型错误：`ToolError::InvalidArgs`
- 存储或 embedding 等执行失败：`ToolError::ExecutionFailed`

当模型传入非支持 action（如 `upsert`、`delete`）会直接返回参数错误。

## 测试覆盖

`klaw-tool` 的 `memory` 单测覆盖：

- `add` 自动使用 `session_key` 作为 scope，且不允许外部指定 `id`
- `search` 使用 runtime config 注入的策略参数
- 非法 action 被拒绝

`klaw-config` 的单测覆盖：

- `tools.memory` 默认值
- `search_limit/fts_limit/vector_limit` 的非零校验

## 设计收益

- 模型侧参数显著减少，调用决策更简单。
- 检索行为由配置统一治理，线上稳定性更高。
- 保留核心能力（写入与检索），避免低价值动作暴露。
