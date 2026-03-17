# Shell Tool 设计与实现

本文档记录 `klaw-tool` 中 `shell` 工具的升级实现：参数契约、审批模型、安全分级、执行语义、结构化输出与测试覆盖。

## 目标

- 提供可控的 shell 执行能力，而不是无约束的 `sh -c`。
- 支持审批与提权请求语义（`sandbox_permissions` / `justification` / `prefix_rule`）。
- 对模型返回结构化结果，明确 success/exit_code/timed_out。
- 增加 workspace 边界约束、输出截断和命令风险分级。

## 代码位置

- 工具实现：`klaw-tool/src/shell.rs`
- 配置模型：`klaw-config/src/lib.rs`（`tools.shell`）
- 运行时注册：`klaw-cli/src/commands/runtime.rs`

## 参数模型（强约束）

`shell` 使用强类型请求结构并开启 `deny_unknown_fields`。

字段：

- `command`（必填）：执行命令
- `workdir`（可选）：工作目录，支持相对路径
- `timeout_ms`（可选）：执行超时（毫秒）
- `login`（可选）：是否使用 login shell
- `sandbox_permissions`（可选）：`use_default` / `require_escalated`
- `justification`（可选）：当 `require_escalated` 时必填
- `prefix_rule`（可选）：用于匹配预批准前缀规则

工具 metadata 中设置了 `additionalProperties = false`，避免模型传入无效字段。

## 配置模型（`tools.shell`）

新增并使用以下配置项：

```toml
[tools.shell]
blocked_patterns = ["rm -rf /", "mkfs"]
safe_commands = ["ls", "cat", "echo", "rg", "find"]
approval_policy = "on_request" # never / on_request
allow_login_shell = true
max_timeout_ms = 120000
max_output_bytes = 131072
```

校验规则：

- `safe_commands` 不能为空
- `max_timeout_ms > 0`
- `max_output_bytes > 0`

## 风险分级与审批

命令分级：

- `safe`：命中 `safe_commands` 且不含 shell 组合操作符
- `mutating`：未知命令或包含 `&& | ; > < $( )` 等组合/重定向迹象
- `destructive`：命中 `blocked_patterns`

审批策略：

- `destructive`：直接拒绝
- `mutating`：要求审批
- `sandbox_permissions=require_escalated`：无论命令风险都要求审批，且 `justification` 必填

审批来源（metadata）：

- `shell.approved = true`
- `shell.approved_prefixes = [["cargo","test"],["git","status"]]`

若未批准，工具返回明确错误提示。

## 执行语义

- shell 选择：
  - 优先 `metadata["shell.path"]`
  - 否则环境变量 `SHELL`
  - 再回退 `sh`
- `login=true` 时使用 `-lc`（若配置允许）
- 支持 PowerShell 参数分支（`-Command`）
- 将 `KLAW_SESSION_KEY` 注入子进程环境

## 路径与边界控制

- 基准目录：`metadata["workspace"]`（否则 `tools.shell.workspace`，再否则 `(<storage.root_dir 或 ~/.klaw/data>)/workspace`）
- `workdir` 相对路径会基于 workspace 解析并 canonicalize
- 默认禁止越出 workspace
- 仅当 `sandbox_permissions=require_escalated` 且审批通过时允许 workspace 外目录

## 输出与可观测性

- 输出为结构化 JSON，字段包括：
  - `success`
  - `exit_code`
  - `risk`
  - `approval_required` / `approved`
  - `stdout` / `stderr`
  - `stdout_truncated` / `stderr_truncated`
  - `duration_ms`
- 按 `max_output_bytes` 截断输出，防止上下文膨胀
- 使用 `tracing` 记录 begin/finish 审计事件

## 特殊命令拦截

- `apply_patch` 命令会被 shell 工具显式拦截并拒绝。
- 引导使用专用 patch 工具路径，避免通过通用 shell 绕过约束。

## 测试覆盖

`klaw-tool/src/shell.rs` 当前覆盖：

- 基本执行成功
- 非 0 退出码结构化失败输出
- workspace 工作目录
- 超时行为（含配置上限 clamp）
- 危险命令阻断
- mutating 命令审批要求
- escalated 必填 justification
- prefix_rule 预批准路径
- apply_patch 拦截
- workspace 越界阻断
- login shell 禁用
- unknown field 拒绝
- 输出截断标记
