# 配置（Configuration）

配置模块提供基于 TOML 的配置管理能力，支持配置加载、验证、迁移与热重载。

## 快速开始

### 1. 首次启动

```bash
# 首次运行会自动创建默认配置
klaw stdio
```

### 2. 手动创建配置

```bash
# 使用默认配置初始化
klaw --migrate-config
```

### 3. 编辑配置

配置文件位于 `~/.klaw/config.toml`：

```toml
# 设置模型 Provider
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
env_key = "OPENAI_API_KEY"

# 启用 Shell 工具
[tools.shell]
enabled = true
safe_commands = ["ls", "cat", "echo", "git status"]
```

### 4. 验证配置

```bash
# 验证配置文件
klaw --validate-config
```

## 配置结构

```
AppConfig
├── model_provider       # 当前使用的 Provider
├── model                # 可选：覆盖默认模型
├── model_providers      # Provider 配置映射
├── gateway              # WebSocket 网关配置
├── channels             # 渠道配置（DingTalk 等）
├── tools                # 工具配置
├── storage              # 存储配置
├── cron                 # 定时任务配置
├── heartbeat            # 心跳配置
├── mcp                  # MCP 配置
└── skills               # Skills 配置
```

## 文档导航

- [配置概述](./overview.md) - 配置系统设计与实现
- [配置字段详解](./fields.md) - 所有配置字段详细说明

## 环境变量

部分配置支持通过环境变量覆盖：

| 环境变量 | 配置项 | 描述 |
|----------|--------|------|
| `KLAW_CONFIG` | 配置文件路径 | 覆盖默认配置路径 |
| `OPENAI_API_KEY` | `model_providers.openai.env_key` | OpenAI API 密钥 |
| `TAVILY_API_KEY` | `tools.web_search.tavily.env_key` | Tavily 搜索密钥 |
| `BRAVE_SEARCH_API_KEY` | `tools.web_search.brave.env_key` | Brave 搜索密钥 |

## 配置示例

### 最小配置

```toml
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
env_key = "OPENAI_API_KEY"
default_model = "gpt-4o-mini"
```

### 完整配置

参见 [配置字段详解](./fields.md)。

## 配置管理 API

### Rust API

```rust
use klaw_config::ConfigStore;

// 打开配置存储
let store = ConfigStore::open(None)?;

// 获取当前配置
let snapshot = store.snapshot();
println!("当前版本：{}", snapshot.revision);

// 验证新配置
store.validate_raw_toml(&new_toml)?;

// 保存新配置
let new_snapshot = store.save_raw_toml(&new_toml)?;

// 重新加载配置
let reloaded = store.reload()?;

// 重置为默认值
let reset = store.reset_to_defaults()?;

// 迁移配置（添加新字段）
let migrated = store.migrate_with_defaults()?;
```

## 配置验证规则

配置验证在以下时机执行：

1. **加载时**：TOML 解析后自动验证
2. **保存前**：`save_raw_toml()` 会先验证
3. **显式验证**：`validate_raw_toml()`

验证失败返回 `ConfigError::InvalidConfig`。

## 配置迁移

当添加新配置字段时，现有用户的配置文件需要迁移：

```bash
# 迁移配置，保留现有配置并添加新字段
klaw --migrate-config
```

迁移行为：
- 保留用户现有的所有配置
- 添加缺失的字段（使用默认值）
- 格式化为美观的 TOML

## 故障排查

### 配置无法加载

```
Error: config file not found: /Users/xxx/.klaw/config.toml
```

**解决**: 运行 `klaw --migrate-config` 创建默认配置。

### 验证失败

```
Error: invalid config: model_provider cannot be empty
```

**解决**: 检查配置文件中 `model_provider` 是否配置。

### TOML 解析失败

```
Error: failed to parse config file: TOML parse error
```

**解决**: 检查 TOML 语法是否正确。

## 相关文档

- [工具配置](../tools/README.md)
- [渠道配置](../channels/README.md)
- [MCP 配置](../mcp/README.md)
