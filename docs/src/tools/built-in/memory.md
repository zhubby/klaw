# Memory Tool 设计与实现

本文档记录当前 `memory` 工具的真实语义、长期记忆治理规则，以及 session 记忆的检索边界。

## 当前定位

`memory` 工具现在承担两件事：

- `add`：写入长期记忆（`long_term`）
- `search`：检索 session 记忆

它不再承担以下职责：

- 不再让模型直接搜索 `long_term`
- 不再让模型写入 `session` 作用域记忆
- 不暴露 `get/delete/pin` 等维护动作给模型

## 代码位置

- 工具实现：`klaw-tool/src/memory.rs`
- 长期记忆治理：`klaw-memory/src/governance.rs`
- 长期记忆 prompt 渲染：`klaw-memory/src/prompt.rs`
- 记忆服务：`klaw-memory/src/service.rs`
- runtime 注入：`klaw-cli/src/runtime/mod.rs`

## Tool Metadata（面向 LLM）

### `add`

用于写入长期记忆。最小参数集合：

- `action`
- `content`
- `metadata`
- `pinned`

其中 `metadata` 支持的治理字段：

- `kind`：`identity | preference | project_rule | workflow | fact | constraint`
- `priority`：可选，`high | medium | low`，用于覆盖默认的 prompt 注入优先级
- `topic`：可选，用于冲突替换
- `supersedes`：可选，字符串或字符串数组

以下字段属于系统托管，不建议模型主动写：

- `status`
- `superseded_by`

### `search`

仅用于检索 session 记忆。最小参数集合：

- `action`
- `query`
- `scope`
- `within_days`
- `limit`

约束：

- `scope` 只允许 `session`
- `within_days` 默认为 `3`
- `limit` 会被 runtime 上限裁剪

## 长期记忆方案

### 写入语义

`add` 永远写入 `long_term`。旧版通过 `scope` 让模型决定写长期还是会话的做法已经移除。

长期记忆的 source of truth 仍然是 `memory.db` 中的 `memories.scope = "long_term"` 记录。

### 注入语义

长期记忆不再通过工具检索给模型，而是在 runtime 每轮执行前，整理出一份受控的 `Memory` 章节并拼入 `system prompt`。

渲染规则：

- 仅渲染 `status=active` 的长期记忆
- 跳过 `superseded`、`archived`、`rejected`
- 跳过 `summary=true` 的归档摘要记录
- 先按 `pinned` 排序
- 再按显式 `priority` 排序（若未提供则根据 `kind` 推导默认值）
- 再按 `kind` 优先级排序
- 最后按更新时间排序
- 做去重、单条裁剪和整体字符预算控制

### 治理规则

长期记忆写入前会经过正式治理流程：

- 规范化 `content`，去除多余空白
- 规范化 `kind`
- 强制 `status=active`
- 规范化 `supersedes`
- 如果命中完全重复的 active 记录，则复用原记录 ID
- 如果命中同一 `kind + topic` 的 active 记录，则把旧记录自动标记为 `superseded`

这套逻辑的目标不是“日志累积”，而是“事实替换”。

例如：

- 旧：`kind=preference`，`topic=reply_language`，内容为“默认使用英文回复”
- 新：`kind=preference`，`topic=reply_language`，内容为“默认使用中文回复”

则新记录会生效，旧记录会被标记为 `superseded`，不再进入 prompt。

### 自动归档

runtime 在后台 tick 期间会执行一个内建的长期记忆维护任务：

- 以系统时区每天凌晨 `2:00` 为目标窗口
- 扫描 `long_term` 中 `priority=low`、未 pinned、超过 `30` 天未更新的 active 记录
- 将原记录标记为 `archived`
- 为同一 `kind + topic` 分组生成或更新一条 `summary=true` 的摘要索引记录

摘要记录写回同一个 `long_term` scope，并带有：

- `source_ids`
- `archived_at`
- `summary_type = "archive_rollup"`

它们默认不会进入 prompt，但会保留为可追溯的检索索引和 GUI 明细。

## Session 记忆方案

session 记忆不再单独持久化到第二套 memory 表，而是直接复用现有 session/chat 存储。

实现要点：

- source of truth 是 `SessionStorage` 的 chat JSONL
- `search` 时优先解析 `channel.base_session_key`
- 若存在 active session，则会合并 base 与 active 的相关历史
- 只读取 `user` / `assistant` 消息
- 只检索 `within_days` 时间窗内的数据
- 只返回裁剪后的命中条数

因此：

- session 记忆是“检索视图”
- 不是新的独立写入实体
- 也不会进入 `system prompt`

## 配置

当前 `memory` 工具仍受 `tools.memory.enabled` 与 `tools.memory.search_limit` 控制。

```toml
[tools.memory]
enabled = true
search_limit = 8
```

其中：

- `search_limit` 现在主要约束 session 检索的返回上限
- `fts_limit`、`vector_limit`、`use_vector` 仍保留在配置结构中，但不再用于模型侧 session 检索参数

## 返回结构

### `add`

返回：

- `record`
- `governance.kind`
- `governance.reused_existing_id`
- `governance.supersedes`

### `search`

返回：

- `base_session_key`
- `session_keys`
- `within_days`
- `limit`
- `hits`

其中单个 `hit` 包含：

- `session_key`
- `ts_ms`
- `role`
- `content`
- `score`

## 测试覆盖

当前测试覆盖了以下核心路径：

- 长期记忆默认写入 `long_term`
- 拒绝旧版 `add.scope`
- 拒绝 `search scope=long_term`
- session 检索 obey `within_days` 和 `limit`
- 长期记忆治理支持 `kind/topic`
- 同一 `kind + topic` 冲突会自动 supersede 旧记录
- 拒绝外部直接写入系统托管的 `status`
