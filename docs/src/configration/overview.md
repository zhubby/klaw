# 配置系统设计与实现

本文档记录 `klaw-config` 模块的实现：配置加载、验证、迁移与热重载机制。

## 目标

- 基于 TOML 的配置文件格式
- 支持配置加载、验证、迁移与热重载
- 提供安全的配置存储与访问机制
- 完整的配置验证规则

## 代码位置

- 配置模型：`klaw-config/src/lib.rs`
- IO 操作：`klaw-config/src/io.rs`
- 验证逻辑：`klaw-config/src/validate.rs`

## 配置文件位置

默认配置文件位于 `~/.klaw/config.toml`。

可通过环境变量或命令行参数覆盖：
- 默认路径：`$HOME/.klaw/config.toml`

## 配置结构

### 顶层配置 (`AppConfig`)

```toml
# 模型 Provider 配置
model_provider = "openai"
model = "gpt-4o-mini"  # 可选，覆盖 provider 的 default_model

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

# 网关配置
[gateway]
listen_ip = "127.0.0.1"
listen_port = 8080

# 渠道配置
[[channels.dingtalk]]
id = "default"
enabled = true
client_id = "..."
client_secret = "..."

# 工具配置
[tools.shell]
enabled = true
safe_commands = ["ls", "cat", "echo"]

[tools.web_search]
enabled = true
provider = "tavily"

# 存储配置
[storage]
root_dir = "~/.klaw/data"

# 定时任务配置
[cron]
tick_ms = 1000
runtime_tick_ms = 200

# 心跳配置
[heartbeat.defaults]
enabled = true
every = "30m"
prompt = "Review the session state..."

# MCP 配置
[mcp]
enabled = true
startup_timeout_seconds = 60

[[mcp.servers]]
id = "filesystem"
mode = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem"]

# Skills 配置
[skills]
sync_timeout = 60

[skills.registries.anthropic]
address = "https://github.com/anthropics/skills"
installed = []
```

## 配置加载流程

### 1. 加载或初始化

```rust
pub fn load_or_init(config_path: Option<&Path>) -> Result<LoadedConfig, ConfigError> {
    let path = match config_path {
        Some(path) => path.to_path_buf(),
        None => default_config_path()?,
    };

    let create_if_missing = config_path.is_none();
    load_from_path(&path, create_if_missing)
}
```

- 未指定路径时使用默认路径
- 自动创建默认配置（如果不存在）

### 2. 解析与验证

```rust
let raw = fs::read_to_string(path)?;
let config: AppConfig = toml::from_str(&raw)?;
validate(&config)?;  // 业务规则验证
```

### 3. 配置存储

```rust
let store = ConfigStore::open(None)?;  // 加载默认配置
let snapshot = store.snapshot();  // 获取当前配置快照
```

## 配置存储 (`ConfigStore`)

### 核心 API

| 方法 | 描述 |
|------|------|
| `open()` | 打开配置存储 |
| `snapshot()` | 获取当前配置快照 |
| `save_raw_toml()` | 保存原始 TOML 并重载 |
| `validate_raw_toml()` | 验证 TOML 内容 |
| `reload()` | 从磁盘重新加载 |
| `reset_to_defaults()` | 重置为默认值 |
| `migrate_with_defaults()` | 迁移配置（合并默认值） |

### 配置快照

```rust
#[derive(Debug, Clone)]
pub struct ConfigSnapshot {
    pub path: PathBuf,
    pub config: AppConfig,
    pub raw_toml: String,
    pub revision: u64,  // 版本号，每次保存递增
}
```

### 线程安全

```rust
#[derive(Debug, Clone)]
pub struct ConfigStore {
    inner: Arc<RwLock<ConfigSnapshot>>,
}
```

- 使用 `Arc<RwLock>` 实现线程安全
- 读操作无锁阻塞（`read()`）
- 写操作独占访问（`write()`）

## 配置迁移

### 迁移与合并

```rust
pub fn migrate_with_defaults(config_path: Option<&Path>) -> Result<MigratedConfig, ConfigError> {
    // 1. 读取现有配置
    let existing_value: toml::Value = toml::from_str(&raw)?;

    // 2. 获取默认配置
    let default_value = toml::Value::try_from(AppConfig::default())?;

    // 3. 合并（现有配置优先）
    merge_toml_values(&mut merged_value, existing_value);

    // 4. 写回磁盘
    fs::write(path, rendered)?;
}
```

### 重置为默认

```rust
pub fn reset_to_defaults(config_path: Option<&Path>) -> Result<MigratedConfig, ConfigError> {
    fs::write(path, default_config_template())?;
}
```

## 配置验证

### 验证规则分类

#### 1. 模型 Provider 验证

| 规则 | 描述 |
|------|------|
| `model_provider` | 不能为空 |
| `model` | 如配置则不能为空 |
| `model_providers.<name>` | 必须存在于映射中 |
| `base_url` | 不能为空 |
| `default_model` | 不能为空 |
| `wire_api` | 不能为空 |

#### 2. 网关验证

| 规则 | 描述 |
|------|------|
| `listen_ip` | 必须是有效 IP 地址 |
| `listen_port` | 必须 > 0 |
| `tls.enabled=true` 时 | `cert_path` 和 `key_path` 必填 |

#### 3. 渠道验证

| 规则 | 描述 |
|------|------|
| `dingtalk.id` | 不能为空，不能重复 |
| `client_id` / `client_secret` | 不能为空（启用时） |
| `bot_title` | 不能为空 |
| `proxy.enabled=true` 时 | `proxy.url` 必填且为有效 URL |

#### 4. 工具验证

**Shell 工具**:
- `safe_commands` 不能为空
- `max_timeout_ms` > 0
- `max_output_bytes` > 0
- `workspace` 如配置则不能为空

**Web Search 工具**:
- `provider` 不能为空
- `provider=tavily` 时需要 `api_key` 或 `env_key`
- `provider=brave` 时需要 `api_key` 或 `env_key`

**Web Fetch 工具**:
- `max_chars` > 0
- `timeout_seconds` > 0

**Apply Patch 工具**:
- `allowed_roots` 不能包含空路径
- `workspace` 如配置则不能为空

**Sub Agent 工具**:
- `max_iterations` > 0
- `max_tool_calls` > 0

**Memory 工具**:
- `search_limit` > 0
- `fts_limit` > 0
- `vector_limit` > 0

#### 5. MCP 验证

| 规则 | 描述 |
|------|------|
| `startup_timeout_seconds` | 必须 > 0 |
| `servers.id` | 不能为空，不能重复 |
| `mode=stdio` | `command` 必填 |
| `mode=sse` | `url` 必填且为有效 HTTP(S) URL |

#### 6. 内存嵌入验证

| 规则 | 描述 |
|------|------|
| `enabled=true` 时 | `provider` 和 `model` 必填 |
| `provider` | 必须存在于 `model_providers` |

#### 7. Skills 验证

| 规则 | 描述 |
|------|------|
| `sync_timeout` | 必须 > 0 |
| `registries.<name>.address` | 不能为空 |
| `installed` | 不能包含空名称或重复 |

#### 8. Cron 验证

| 规则 | 描述 |
|------|------|
| `tick_ms` | 必须 > 0 |
| `runtime_tick_ms` | 必须 > 0 |
| `runtime_drain_batch` | 必须 > 0 |
| `batch_limit` | 必须 > 0 |

#### 9. 心跳验证

| 规则 | 描述 |
|------|------|
| `defaults.every` | 不能为空，必须是有效时长 |
| `defaults.prompt` | 不能为空 |
| `defaults.silent_ack_token` | 不能为空 |
| `defaults.timezone` | 不能为空 |
| `sessions.session_key` | 不能为空，不能重复 |
| `sessions.chat_id` | 不能为空 |
| `sessions.channel` | 不能为空 |

### 验证错误类型

```rust
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot resolve home directory for default config path")]
    HomeDirUnavailable,

    #[error("config file not found: {0}")]
    ConfigNotFound(PathBuf),

    #[error("failed to create config directory: {0}")]
    CreateDir(#[source] std::io::Error),

    #[error("failed to write config file: {0}")]
    WriteConfig(#[source] std::io::Error),

    #[error("failed to read config file {path}: {source}")]
    ReadConfig { path: PathBuf, source: std::io::Error },

    #[error("failed to parse config file {path}: {source}")]
    ParseConfig { path: PathBuf, source: toml::de::Error },

    #[error("invalid config: {0}")]
    InvalidConfig(String),
}
```

## 默认配置

### 模型 Provider 默认

```toml
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"
```

### 工具默认状态

| 工具 | 默认启用 |
|------|----------|
| `shell` | ✓ |
| `apply_patch` | ✓ |
| `approval` | ✓ |
| `local_search` | ✓ |
| `terminal_multiplexers` | ✓ |
| `cron_manager` | ✓ |
| `skills_registry` | ✓ |
| `memory` | ✓ |
| `web_fetch` | ✓ |
| `web_search` | ✓ |
| `sub_agent` | ✓ |

### Shell 安全命令默认列表

```
ls, pwd, cat, echo, head, tail, grep, rg, find, wc, sed, awk, sort, uniq, cut,
basename, dirname, date, sleep, printf, which, type, printenv, env, ps, whoami
```

### Shell 危险模式默认

```
rm -rf /, rm -rf ~, :(){ :|:& };:, mkfs, shutdown, reboot
```

## 配置使用示例

### 1. 程序化加载

```rust
use klaw_config::{ConfigStore, AppConfig};

// 加载配置
let store = ConfigStore::open(None)?;

// 获取当前配置
let snapshot = store.snapshot();
let config: &AppConfig = &snapshot.config;

// 验证新配置
store.validate_raw_toml(&new_toml)?;

// 保存新配置
let new_snapshot = store.save_raw_toml(&new_toml)?;
println!("配置已更新到版本 {}", new_snapshot.revision);
```

### 2. 配置迁移

```rust
use klaw_config::migrate_with_defaults;

// 迁移现有配置（添加新字段）
let migrated = migrate_with_defaults(None)?;
println!("创建了新文件：{}", migrated.created_file);
```

### 3. 重置配置

```rust
use klaw_config::reset_to_defaults;

// 重置为默认配置
let result = reset_to_defaults(None)?;
```

## 配置热重载

```rust
let store = ConfigStore::open(None)?;

// 监听配置文件变化并重新加载
let snapshot = store.reload()?;
println!("配置从版本 {} 更新到 {}", old_snapshot.revision, snapshot.revision);
```

## 最佳实践

### 1. 环境变量管理

```toml
# 推荐：使用 env_key 而非硬编码密钥
[model_providers.openai]
env_key = "OPENAI_API_KEY"  # ✓

# 不推荐：硬编码密钥
[model_providers.openai]
api_key = "sk-xxx"  # ✗
```

### 2. 配置验证前置

```rust
// 启动前验证配置
let store = ConfigStore::open(None)?;
println!("配置加载成功，版本 {}", store.snapshot().revision);
```

### 3. 配置迁移策略

```toml
# 升级时执行迁移，保留现有配置并添加新字段
# 新增字段会自动使用默认值
```

### 4. 配置备份

```bash
# 修改配置前备份
cp ~/.klaw/config.toml ~/.klaw/config.toml.bak
```

## 故障排查

### 配置无法加载

```
Error: failed to read config file ~/.klaw/config.toml: No such file or directory
```

**解决**: 确认文件存在或运行 `klaw --migrate-config` 创建默认配置。

### 验证失败

```
Error: invalid config: tools.shell.safe_commands must contain at least one command
```

**解决**: 检查配置字段是否符合验证规则。

### TOML 解析失败

```
Error: failed to parse config file ~/.klaw/config.toml: TOML parse error
```

**解决**: 检查 TOML 语法，确保键值对格式正确。
