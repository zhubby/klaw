# Skills Manager 工具

## 功能描述

`SkillsManager` 工具用于动态管理 Skills，支持从远程 Git 仓库安装、更新、卸载 Skills。

Skills 是预定义的提示工程模板，可以被模型调用，帮助模型掌握特定技能。

## 配置

```toml
[skills]
sync_timeout_secs = 60
data_dir = "~/.klaw/skills"

[skills.anthropic]
address = "https://github.com/anthropics/skills"
installed = []

[skills.vercel]
address = "https://github.com/vercel-labs/skills"
installed = ["brainstorming"]
```

```toml
[tools.skills_manager]
enabled = true
```

## 参数说明

### 从仓库安装 Skill

```json
{
  "action": "install",
  "source": "anthropic",
  "skill_name": "connector"
}
```

参数：
- `action`: `"install"` - 安装技能
- `source`: `string` - 源名称（对应配置中的 skill source）
- `skill_name`: `string` - 技能名称

### 卸载 Skill

```json
{
  "action": "uninstall",
  "source": "anthropic",
  "skill_name": "connector"
}
```

### 列出已安装 Skills

```json
{
  "action": "list",
  "source": "anthropic"
}
```

### 同步所有已安装 Skills

```json
{
  "action": "sync",
  "source": "anthropic"
}
```

同步会拉取远程仓库最新版本更新到本地。

### 搜索可用 Skills

```json
{
  "action": "search",
  "source": "anthropic",
  "query": "prompt"
}
```

## 输出说明

安装成功返回技能信息，列表返回已安装技能的详细描述，搜索返回匹配技能列表。

## 工作流程

1. 从配置中读取源仓库地址
2. git clone 或 git pull 到本地缓存
3. 解析技能定义（JSON 格式）
4. 复制到技能数据目录
5. 注册到 `SkillsRegistry` 供调用

## Skills 格式兼容性

- 兼容 Anthropic `skills` 仓库格式
- 兼容 Vercel `skills` 仓库格式

## 区别：`SkillsRegistryTool` 用于查询注册表获取技能信息，`SkillsManagerTool` 用于生命周期管理（安装/卸载/同步）。
