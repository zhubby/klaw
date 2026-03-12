# Skills 模块设计与实现

本文档说明 `klaw-skill` 与 `klaw-tool` 中 skills 能力的设计目标、配置模型、registry 同步策略与运行时接入方式。

## 目标

- 在本地数据目录统一管理 skills（`~/.klaw/skills`）。
- 抽象并实现 skills 生命周期能力：查询、删除、加载全部 `SKILL.md`，以及基于 registry 的安装同步。
- 支持多个可配置 registry 源（Git 仓库），并从配置文件读取。
- 在程序启动时加载本地 skill 内容，并注入模型 system prompt。

## 代码位置

- Skills 领域模块：`klaw-skill/src/`
  - `model.rs`：`SkillSource` / `SkillSummary` / `SkillRecord`
  - `error.rs`：`SkillError`
  - `fetcher.rs`：`SkillFetcher` / `ReqwestSkillFetcher`
  - `store.rs`：`SkillStore` trait
  - `fs_store.rs`：`FileSystemSkillStore` 默认实现
  - `lib.rs`：模块导出
- Skills 工具：`klaw-tool/src/skills_registry.rs`
- 运行时注册：`klaw-cli/src/commands/runtime.rs`
- 配置模型：`klaw-config/src/lib.rs`

## 数据目录约定

- 根目录：`~/.klaw`
- skills 目录：`~/.klaw/skills`
- registry 目录：`~/.klaw/skills-registry`
- registry 安装清单：`~/.klaw/skills-registry-manifest.json`
- 单个 skill 文件：`~/.klaw/skills/<skill_name>/SKILL.md`

写入采用“临时文件 + rename”的原子覆盖策略，避免部分写入导致损坏。

## 配置模型（顶层 `skills`）

skills 配置位于 `AppConfig` 顶层，而不是 `tools` 下：

```toml
[skills]
sync_timeout = 60

[skills.anthropic]
address = "https://github.com/anthropics/skills"

[skills.vercel]
address = "https://github.com/vercel-labs/skills"
installed = ["brainstorming"]
```

约束：

- `<registry>` 作为表名。
- `skills.sync_timeout` 为每个 registry 同步任务超时（秒），默认 `60`。
- `skills.<registry>.address` 非空。
- `skills.<registry>.installed` 可选，元素非空，且在同一个 registry 内不可重复。
- `installed` 条目优先按 `skills/<name>` 目录匹配；若未命中，会回退按 `SKILL.md` 中解析出的名称匹配。

## Registry 同步与安装

启动时会执行以下流程：

1. 遍历 `skills.<registry>`，将每个仓库同步到 `~/.klaw/skills-registry/<registry>`。
   - 首次：`git clone`
   - 后续：`git fetch` + `git reset --hard origin/HEAD`（失败时回退 `origin/master`）
2. 遍历每个 `skills.<registry>.installed`，将
   `~/.klaw/skills-registry/<registry>/skills/<name>` 复制到 `~/.klaw/skills/<name>`。
3. 依据 `skills-registry-manifest.json` 做差异清理：
   - 只删除“manifest 中标记为受管”且本次不再安装的 skill；
   - 不删除用户手工放入 `~/.klaw/skills` 的未受管目录，避免冲突。

## SkillStore 抽象

`SkillStore` 提供统一异步接口：

- `download(skill_name)`
- `download_with_source(skill_name, source_name, template)`
- `delete(skill_name)`
- `list()`
- `get(skill_name)`
- `update(skill_name)`
- `update_with_source(skill_name, source_name, template)`
- `load_all_skill_markdowns()`

`FileSystemSkillStore` 还提供 registry 安装同步接口：

- `sync_registry_installed_skills(sources, installed)`

其中：

- `update` 语义是“重下载并覆盖”。
- `list/get` 用于本地查询。

## SkillsRegistryTool（工具层）

工具名：`skills_registry`

支持 action：

- `download`（需 `source` + `skill_name`）
- `update`（需 `source` + `skill_name`）
- `delete`（需 `skill_name`）
- `list`
- `get`（需 `skill_name`，`source` 可选，仅用于输出上下文）
- `load_all`

典型调用示例：

```json
{"action":"download","source":"vercel","skill_name":"find-skills"}
```

## 启动加载与 System Prompt 注入

运行时行为：

1. 先执行 registry 同步与 `installed` 安装。
2. 再从本地 `~/.klaw/skills/**/SKILL.md` 加载全部 skill 文本。
3. 拼接成统一 system prompt 文本，并在模型调用前注入。

这样可确保已安装 skill 在请求处理阶段对模型持续生效。

## 错误模型

`SkillError` 覆盖：

- 非法 skill 名
- HOME 不可用
- skill 不存在
- 网络失败 / 远端状态异常
- 文件 I/O 失败
- git 同步失败
- 受管 skill 与本地非受管同名目录冲突

工具层统一映射为 `ToolError::ExecutionFailed`，并返回可读错误信息。

## 测试覆盖

`klaw-skill` 已覆盖：

- 名称校验与路径安全
- `list/get` 空目录与有数据场景
- `delete` 存在/不存在分支
- `load_all` 聚合行为
- `download/update` 的可注入 fetcher 路径（不依赖真实网络）

`klaw-config` 已覆盖：

- 顶层 `skills.<registry>` 默认解析
- `name/address` 非法值校验
- 重名源校验

## 参考下载源

- Anthropic skills: [https://github.com/anthropics/skills](https://github.com/anthropics/skills)
- Vercel skills: [https://github.com/vercel-labs/skills](https://github.com/vercel-labs/skills)
