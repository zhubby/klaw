# Local Search Tool 设计与实现

本文档记录 `klaw-tool` 中 `local_search` 工具的设计目标、参数约束、实现细节与测试覆盖。

## 目标

- 提供统一的本地代码检索入口：按内容模式快速定位“哪些文件命中”。
- 只返回文件路径，避免一次性回传大量文件内容。
- 保持调用参数简单，便于模型稳定调用。

## 代码位置

- 工具实现：`klaw-tool/src/local_search.rs`
- 工具导出：`klaw-tool/src/lib.rs`
- 运行时注册：`klaw-cli/src/commands/runtime.rs`

## 工具语义

`local_search` 采用 “`rg` 优先，`grep` 兜底” 的文件级检索策略：

- 首选后端：`rg --files-with-matches`
- fallback 后端：BSD/macOS 兼容的 `grep -R -l -E`
- 返回值：命中模式的文件路径列表
- 默认忽略：`.git` 与 `node_modules`
- 默认会搜索隐藏文件，和 `rg --hidden` 保持一致

当系统安装了 `ripgrep` 时，`local_search` 会直接使用 `rg`。如果 `rg` 在 PATH 中不存在，则会回退到 `grep`，并在 Rust 侧完成：

- `include_pattern` 的 glob 过滤
- `.git` 与 `node_modules` 的目录排除
- 大量候选文件的分批执行，避免参数过长

适用场景：

- 先定位定义/引用位置，再按需读文件。
- 在较大仓库中快速缩小分析范围。

## Tool Metadata（面向 LLM）

`local_search` 的参数 schema：

- `query`（必填）：内容模式（字符串或正则）
- `path`（可选，默认 `.`）：检索根目录
- `include_pattern`（可选）：按文件路径做 glob include 过滤
- `limit`（可选，默认 `20`，范围 `1..=200`）：最多返回多少条路径
- `timeout_ms`（可选，默认 `10000`，范围 `1..=60000`）：执行超时

并设置：

- `additionalProperties = false`：拒绝无效字段
- `description/examples`：明确字段语义与典型用法

## 错误处理

- 参数错误：`ToolError::InvalidArgs`
  - `query` 为空
  - `limit` 非法（0 或超过上限）
  - `timeout_ms` 非法（0 或超过上限）
- 执行错误：`ToolError::ExecutionFailed`
  - 进程超时
  - 非预期退出码（非 0/1）
  - `rg` 与 `grep` 都不存在，或 fallback 执行失败

说明：

- `rg` 的退出码 `1` 代表“无匹配”，不视为错误，会返回 `no matching files`
- fallback `grep` 的退出码 `1` 同样代表“无匹配”，不视为错误

## 输出格式

- 有结果：返回 query/path、命中数量与编号文件列表
- 超出 limit：附带 `truncated` 提示
- 无结果：返回 `no matching files`

## 测试覆盖

`klaw-tool/src/local_search.rs` 当前包含单测：

- 能找到命中文件
- `include_pattern` 生效
- `limit` 截断提示
- 无匹配时返回稳定文本
- 非法参数（`limit = 0`）报错
- fallback 候选文件会跳过 `.git` 与 `node_modules`
- fallback 路径归一化后与 `rg` 的相对路径输出保持一致
