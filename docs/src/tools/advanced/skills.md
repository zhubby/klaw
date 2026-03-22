# Skills 模块设计与实现

本文档说明 `klaw-skill` 与 `klaw-tool` 中 skills 能力的设计目标、配置模型、registry 同步策略，以及 `skills_registry` / `skills_manager` 的职责划分。

## 目标

- 在本地数据目录统一管理 skills（`~/.klaw/skills`）。
- 抽象并拆分两类能力：
  - registry 目录/镜像的只读浏览与检索
  - 已安装 skill 的安装、卸载、查看与加载
- 支持多个可配置 registry 源（Git 仓库），并从配置文件读取。
- 在程序启动时加载本地 skill 内容，并注入模型 system prompt。

## 代码位置

- Skills 领域模块：`klaw-skill/src/`
  - `model.rs`：`SkillSource` / `SkillSummary` / `SkillRecord`
  - `error.rs`：`SkillError`
  - `fetcher.rs`：`SkillFetcher` / `ReqwestSkillFetcher`
  - `store.rs`：`SkillsRegistry` / `SkillsManager` traits
  - `fs_store.rs`：`FileSystemSkillStore` 默认实现
  - `lib.rs`：模块导出
- Skills 工具：`klaw-tool/src/skills_registry.rs` / `klaw-tool/src/skills_manager.rs`
- 运行时注册：`klaw-cli/src/runtime/mod.rs`
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
2. 使用 `skills-registry-manifest.json` 作为受管 registry skills 的唯一索引：
   - `managed` 记录安装了哪些 `<registry>/<skill>`
   - `registry_commits` 记录每个 registry 当前 `HEAD commit`
   - `stale_registries` 记录同步失败但可用本地缓存的 registry
3. `install_from_registry` 只写 manifest 索引，不再复制 skill 到 `~/.klaw/skills`。
4. `list_installed/get_installed/load_all_installed_skill_markdowns` 合并两类来源：
   - manifest 索引的 registry skills（直接读取 `~/.klaw/skills-registry`）
   - 本地手工 skills（`~/.klaw/skills`）
   - 同名冲突时 registry(managed) 优先，本地同名会被忽略并告警。

### 递归发现 SKILL.md 文件

Registry 同步时会递归扫描目录树，发现所有 `SKILL.md` 文件：

- 扫描深度无限制
- 支持 `SKILL.md` 位于任意子目录
- 从 `SKILL.md` 文件中解析 `name` 字段作为 skill 标识
- 自动跳过 `.git` 目录

**示例目录结构**：

```
skills-registry/
└── vercel/
    ├── SKILL.md              # skill name: vercel/root
    ├── category/
    │   └── SKILL.md          # skill name: vercel/category
    └── tools/
        └── brainstorming/
            └── SKILL.md      # skill name: vercel/brainstorming
```

### 同步状态指示器

GUI Skills Registry 面板显示每个 registry 的同步状态：

| 状态 | 图标 | 说明 |
|------|------|------|
| Synced | ✓ (绿色) | 同步成功，显示 commit hash |
| Stale | ⚠ (黄色) | 同步失败，使用本地缓存 |
| Error | ✗ (红色) | 同步失败且无本地缓存 |
| Pending | ◌ (灰色) | 正在同步中 |

**状态判断逻辑**：

- 如果 `registry_commits` 中有记录且 `stale_registries` 中没有 → Synced
- 如果 `stale_registries` 中有该 registry → Stale
- 如果首次同步失败且无本地缓存 → Error

## SkillsRegistry / SkillsManager 抽象

`SkillsRegistry` 负责只读 registry 镜像能力：

- `list_source_skills(source_name)`
- `get_source_skill(source_name, skill_name)`
- `search_source_skills(source_name, query)`

`SkillsManager` 负责已安装 skill 生命周期：

- `install_from_registry(source_name, skill_name)`
- `uninstall(skill_name)`
- `uninstall_from_registry(source_name, skill_name)`
- `list_installed()`
- `get_installed(skill_name)`
- `load_all_installed_skill_markdowns()`

`FileSystemSkillStore` 还提供 registry 安装同步接口：

- `sync_registry_installed_skills(sources, installed, sync_timeout_secs)`

## Tool 层

### `skills_registry`

工具名：`skills_registry`

支持 action：

- `list`（需 `source`）
- `search`（需 `query`，`source` 可选，支持 `limit`）
- `show`（需 `source` + `skill_name`）

### `skills_manager`

工具名：`skills_manager`

支持 action：

- `install_from_registry`（需 `source` + `skill_name`）
- `uninstall`（需 `skill_name`）
- `list_installed`
- `show_installed`（需 `skill_name`）
- `load_all`

典型调用示例：

```json
{"action":"install_from_registry","source":"vercel","skill_name":"find-skills"}
```

## 启动加载与 System Prompt 注入

运行时行为：

1. 先执行 registry 同步与 `installed` 安装。
2. 再从“registry 索引 + 本地目录”的合并视图加载全部 skill 文本。
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
- manifest 中受管 skill 重名冲突

工具层统一映射为 `ToolError::ExecutionFailed`，并返回可读错误信息。

## 测试覆盖

`klaw-skill` 已覆盖：

- 名称校验与路径安全
- `list_installed/get_installed` 空目录与有数据场景
- `uninstall` 的 manifest/local 联合删除
- `load_all_installed_skill_markdowns` 聚合行为
- `install_from_registry` / `uninstall_from_registry` 的 manifest 更新与冲突校验

`klaw-config` 已覆盖：

- 顶层 `skills.<registry>` 默认解析
- `name/address` 非法值校验
- 重名源校验

## 参考下载源

- Anthropic skills: [https://github.com/anthropics/skills](https://github.com/anthropics/skills)
- Vercel skills: [https://github.com/vercel-labs/skills](https://github.com/vercel-labs/skills)
