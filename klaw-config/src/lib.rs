use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub model_provider: String,
    pub model_providers: BTreeMap<String, ModelProviderConfig>,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub cron: CronConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        let model_provider = "openai".to_string();
        let mut model_providers = BTreeMap::new();
        model_providers.insert(model_provider.clone(), ModelProviderConfig::default());
        Self {
            model_provider,
            model_providers,
            memory: MemoryConfig::default(),
            mcp: McpConfig::default(),
            tools: ToolsConfig::default(),
            cron: CronConfig::default(),
            skills: SkillsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default = "default_mcp_enabled")]
    pub enabled: bool,
    #[serde(default = "default_mcp_startup_timeout_seconds")]
    pub startup_timeout_seconds: u64,
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: default_mcp_enabled(),
            startup_timeout_seconds: default_mcp_startup_timeout_seconds(),
            servers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpServerMode {
    Stdio,
    Sse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub id: String,
    #[serde(default = "default_mcp_server_enabled")]
    pub enabled: bool,
    pub mode: McpServerMode,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

fn default_mcp_enabled() -> bool {
    true
}

fn default_mcp_startup_timeout_seconds() -> u64 {
    30
}

fn default_mcp_server_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    #[serde(default)]
    pub embedding: EmbeddingConfig,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            embedding: EmbeddingConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_memory_embedding_provider")]
    pub provider: String,
    #[serde(default = "default_memory_embedding_model")]
    pub model: String,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: default_memory_embedding_provider(),
            model: default_memory_embedding_model(),
        }
    }
}

fn default_memory_embedding_provider() -> String {
    "openai".to_string()
}

fn default_memory_embedding_model() -> String {
    "text-embedding-3-small".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProviderConfig {
    #[serde(default)]
    pub name: Option<String>,
    pub base_url: String,
    pub wire_api: String,
    pub default_model: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub env_key: Option<String>,
}

impl Default for ModelProviderConfig {
    fn default() -> Self {
        Self {
            name: Some("OpenAI".to_string()),
            base_url: "https://api.openai.com/v1".to_string(),
            wire_api: "chat_completions".to_string(),
            default_model: "gpt-4o-mini".to_string(),
            api_key: None,
            env_key: Some("OPENAI_API_KEY".to_string()),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default)]
    pub shell: ShellConfig,
    #[serde(default)]
    pub memory: MemoryToolConfig,
    #[serde(default)]
    pub web_fetch: WebFetchConfig,
    #[serde(default)]
    pub web_search: WebSearchConfig,
    #[serde(default)]
    pub sub_agent: SubAgentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsConfig {
    #[serde(default = "default_skill_sources")]
    pub sources: Vec<SkillSourceConfig>,
    #[serde(default)]
    pub installed: Vec<InstalledSkillConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSourceConfig {
    pub name: String,
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledSkillConfig {
    pub registry: String,
    pub name: String,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            sources: default_skill_sources(),
            installed: Vec::new(),
        }
    }
}

fn default_skill_sources() -> Vec<SkillSourceConfig> {
    vec![SkillSourceConfig {
        name: "anthropic".to_string(),
        address: "https://github.com/anthropics/skills".to_string(),
    }]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryToolConfig {
    #[serde(default = "default_memory_tool_enabled")]
    pub enabled: bool,
    #[serde(default = "default_memory_tool_search_limit")]
    pub search_limit: usize,
    #[serde(default = "default_memory_tool_fts_limit")]
    pub fts_limit: usize,
    #[serde(default = "default_memory_tool_vector_limit")]
    pub vector_limit: usize,
    #[serde(default = "default_memory_tool_use_vector")]
    pub use_vector: bool,
}

impl Default for MemoryToolConfig {
    fn default() -> Self {
        Self {
            enabled: default_memory_tool_enabled(),
            search_limit: default_memory_tool_search_limit(),
            fts_limit: default_memory_tool_fts_limit(),
            vector_limit: default_memory_tool_vector_limit(),
            use_vector: default_memory_tool_use_vector(),
        }
    }
}

fn default_memory_tool_enabled() -> bool {
    true
}

fn default_memory_tool_search_limit() -> usize {
    8
}

fn default_memory_tool_fts_limit() -> usize {
    20
}

fn default_memory_tool_vector_limit() -> usize {
    20
}

fn default_memory_tool_use_vector() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellConfig {
    #[serde(default = "default_shell_blocked_patterns")]
    pub blocked_patterns: Vec<String>,
    #[serde(default = "default_shell_safe_commands")]
    pub safe_commands: Vec<String>,
    #[serde(default = "default_shell_approval_policy")]
    pub approval_policy: ShellApprovalPolicy,
    #[serde(default = "default_shell_allow_login_shell")]
    pub allow_login_shell: bool,
    #[serde(default = "default_shell_max_timeout_ms")]
    pub max_timeout_ms: u64,
    #[serde(default = "default_shell_max_output_bytes")]
    pub max_output_bytes: usize,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            blocked_patterns: default_shell_blocked_patterns(),
            safe_commands: default_shell_safe_commands(),
            approval_policy: default_shell_approval_policy(),
            allow_login_shell: default_shell_allow_login_shell(),
            max_timeout_ms: default_shell_max_timeout_ms(),
            max_output_bytes: default_shell_max_output_bytes(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShellApprovalPolicy {
    Never,
    OnRequest,
}

fn default_shell_blocked_patterns() -> Vec<String> {
    vec![
        "rm -rf /".to_string(),
        "rm -rf ~".to_string(),
        ":(){ :|:& };:".to_string(),
        "mkfs".to_string(),
        "shutdown".to_string(),
        "reboot".to_string(),
    ]
}

fn default_shell_safe_commands() -> Vec<String> {
    vec![
        "ls".to_string(),
        "pwd".to_string(),
        "cat".to_string(),
        "echo".to_string(),
        "head".to_string(),
        "tail".to_string(),
        "grep".to_string(),
        "rg".to_string(),
        "find".to_string(),
        "wc".to_string(),
        "sed".to_string(),
        "awk".to_string(),
        "sort".to_string(),
        "uniq".to_string(),
        "cut".to_string(),
        "basename".to_string(),
        "dirname".to_string(),
        "date".to_string(),
        "sleep".to_string(),
        "printf".to_string(),
        "which".to_string(),
        "type".to_string(),
        "printenv".to_string(),
        "env".to_string(),
        "ps".to_string(),
        "whoami".to_string(),
    ]
}

fn default_shell_approval_policy() -> ShellApprovalPolicy {
    ShellApprovalPolicy::OnRequest
}

fn default_shell_allow_login_shell() -> bool {
    true
}

fn default_shell_max_timeout_ms() -> u64 {
    120_000
}

fn default_shell_max_output_bytes() -> usize {
    128 * 1024
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchConfig {
    #[serde(default = "default_web_search_enabled")]
    pub enabled: bool,
    #[serde(default = "default_web_search_provider")]
    pub provider: String,
    #[serde(default)]
    pub tavily: TavilyWebSearchConfig,
    #[serde(default)]
    pub brave: BraveWebSearchConfig,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            enabled: default_web_search_enabled(),
            provider: default_web_search_provider(),
            tavily: TavilyWebSearchConfig::default(),
            brave: BraveWebSearchConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchConfig {
    #[serde(default = "default_web_fetch_enabled")]
    pub enabled: bool,
    #[serde(default = "default_web_fetch_max_chars")]
    pub max_chars: usize,
    #[serde(default = "default_web_fetch_timeout_seconds")]
    pub timeout_seconds: u64,
    #[serde(default = "default_web_fetch_cache_ttl_minutes")]
    pub cache_ttl_minutes: u64,
    #[serde(default = "default_web_fetch_max_redirects")]
    pub max_redirects: u8,
    #[serde(default = "default_web_fetch_readability")]
    pub readability: bool,
    #[serde(default)]
    pub ssrf_allowlist: Vec<String>,
}

impl Default for WebFetchConfig {
    fn default() -> Self {
        Self {
            enabled: default_web_fetch_enabled(),
            max_chars: default_web_fetch_max_chars(),
            timeout_seconds: default_web_fetch_timeout_seconds(),
            cache_ttl_minutes: default_web_fetch_cache_ttl_minutes(),
            max_redirects: default_web_fetch_max_redirects(),
            readability: default_web_fetch_readability(),
            ssrf_allowlist: Vec::new(),
        }
    }
}

fn default_web_fetch_max_chars() -> usize {
    50_000
}

fn default_web_fetch_enabled() -> bool {
    true
}

fn default_web_fetch_timeout_seconds() -> u64 {
    15
}

fn default_web_fetch_cache_ttl_minutes() -> u64 {
    10
}

fn default_web_fetch_max_redirects() -> u8 {
    3
}

fn default_web_fetch_readability() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentConfig {
    #[serde(default = "default_sub_agent_enabled")]
    pub enabled: bool,
    #[serde(default = "default_sub_agent_max_iterations")]
    pub max_iterations: u32,
    #[serde(default = "default_sub_agent_max_tool_calls")]
    pub max_tool_calls: u32,
    #[serde(default = "default_sub_agent_inherit_parent_tools")]
    pub inherit_parent_tools: bool,
    #[serde(default = "default_sub_agent_exclude_tools")]
    pub exclude_tools: Vec<String>,
}

impl Default for SubAgentConfig {
    fn default() -> Self {
        Self {
            enabled: default_sub_agent_enabled(),
            max_iterations: default_sub_agent_max_iterations(),
            max_tool_calls: default_sub_agent_max_tool_calls(),
            inherit_parent_tools: default_sub_agent_inherit_parent_tools(),
            exclude_tools: default_sub_agent_exclude_tools(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronConfig {
    #[serde(default = "default_cron_tick_ms")]
    pub tick_ms: u64,
    #[serde(default = "default_cron_runtime_tick_ms")]
    pub runtime_tick_ms: u64,
    #[serde(default = "default_cron_runtime_drain_batch")]
    pub runtime_drain_batch: usize,
    #[serde(default = "default_cron_batch_limit")]
    pub batch_limit: i64,
}

impl Default for CronConfig {
    fn default() -> Self {
        Self {
            tick_ms: default_cron_tick_ms(),
            runtime_tick_ms: default_cron_runtime_tick_ms(),
            runtime_drain_batch: default_cron_runtime_drain_batch(),
            batch_limit: default_cron_batch_limit(),
        }
    }
}

fn default_cron_tick_ms() -> u64 {
    1_000
}

fn default_cron_runtime_tick_ms() -> u64 {
    200
}

fn default_cron_runtime_drain_batch() -> usize {
    8
}

fn default_cron_batch_limit() -> i64 {
    64
}

fn default_sub_agent_max_iterations() -> u32 {
    6
}

fn default_sub_agent_enabled() -> bool {
    true
}

fn default_sub_agent_max_tool_calls() -> u32 {
    12
}

fn default_sub_agent_inherit_parent_tools() -> bool {
    true
}

fn default_sub_agent_exclude_tools() -> Vec<String> {
    vec!["sub_agent".to_string()]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TavilyWebSearchConfig {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub env_key: Option<String>,
    #[serde(default = "default_tavily_search_depth")]
    pub search_depth: String,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub include_answer: Option<bool>,
    #[serde(default)]
    pub include_raw_content: Option<bool>,
    #[serde(default)]
    pub include_images: Option<bool>,
    #[serde(default)]
    pub project_id: Option<String>,
}

fn default_web_search_provider() -> String {
    "tavily".to_string()
}

fn default_web_search_enabled() -> bool {
    true
}

impl Default for TavilyWebSearchConfig {
    fn default() -> Self {
        Self {
            base_url: Some("https://api.tavily.com".to_string()),
            api_key: None,
            env_key: Some("TAVILY_API_KEY".to_string()),
            search_depth: default_tavily_search_depth(),
            topic: None,
            include_answer: None,
            include_raw_content: None,
            include_images: None,
            project_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BraveWebSearchConfig {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub env_key: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub search_lang: Option<String>,
    #[serde(default)]
    pub ui_lang: Option<String>,
    #[serde(default)]
    pub safesearch: Option<String>,
    #[serde(default)]
    pub freshness: Option<String>,
}

impl Default for BraveWebSearchConfig {
    fn default() -> Self {
        Self {
            base_url: Some("https://api.search.brave.com".to_string()),
            api_key: None,
            env_key: Some("BRAVE_SEARCH_API_KEY".to_string()),
            country: None,
            search_lang: None,
            ui_lang: None,
            safesearch: None,
            freshness: None,
        }
    }
}

fn default_tavily_search_depth() -> String {
    "basic".to_string()
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub path: PathBuf,
    pub config: AppConfig,
    pub created_default: bool,
}

#[derive(Debug, Clone)]
pub struct MigratedConfig {
    pub path: PathBuf,
    pub created_file: bool,
}

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
    ReadConfig {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config file {path}: {source}")]
    ParseConfig {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}

pub fn load_or_init(config_path: Option<&Path>) -> Result<LoadedConfig, ConfigError> {
    let explicit = config_path.map(Path::to_path_buf);
    let path = match explicit {
        Some(path) => path,
        None => default_config_path()?,
    };

    let create_if_missing = config_path.is_none();
    load_from_path(&path, create_if_missing)
}

pub fn default_config_path() -> Result<PathBuf, ConfigError> {
    let home = env::var_os("HOME").ok_or(ConfigError::HomeDirUnavailable)?;
    Ok(PathBuf::from(home).join(".klaw").join("config.toml"))
}

pub fn default_config_template() -> String {
    toml::to_string_pretty(&AppConfig::default()).expect("default app config should serialize")
}

pub fn migrate_with_defaults(config_path: Option<&Path>) -> Result<MigratedConfig, ConfigError> {
    let explicit = config_path.map(Path::to_path_buf);
    let path = match explicit {
        Some(path) => path,
        None => default_config_path()?,
    };

    migrate_path_with_defaults(&path)
}

pub fn reset_to_defaults(config_path: Option<&Path>) -> Result<MigratedConfig, ConfigError> {
    let explicit = config_path.map(Path::to_path_buf);
    let path = match explicit {
        Some(path) => path,
        None => default_config_path()?,
    };

    reset_path_to_defaults(&path)
}

fn load_from_path(path: &Path, create_if_missing: bool) -> Result<LoadedConfig, ConfigError> {
    let created_default = if !path.exists() {
        if !create_if_missing {
            return Err(ConfigError::ConfigNotFound(path.to_path_buf()));
        }
        write_default_config(path)?;
        true
    } else {
        false
    };

    let raw = fs::read_to_string(path).map_err(|source| ConfigError::ReadConfig {
        path: path.to_path_buf(),
        source,
    })?;

    let config: AppConfig = toml::from_str(&raw).map_err(|source| ConfigError::ParseConfig {
        path: path.to_path_buf(),
        source,
    })?;

    validate(&config)?;

    Ok(LoadedConfig {
        path: path.to_path_buf(),
        config,
        created_default,
    })
}

fn write_default_config(path: &Path) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(ConfigError::CreateDir)?;
    }
    fs::write(path, default_config_template()).map_err(ConfigError::WriteConfig)?;
    Ok(())
}

fn migrate_path_with_defaults(path: &Path) -> Result<MigratedConfig, ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(ConfigError::CreateDir)?;
    }

    let default_value = toml::Value::try_from(AppConfig::default())
        .expect("default app config should convert to toml value");
    let mut merged_value = default_value;
    let created_file = !path.exists();

    if !created_file {
        let raw = fs::read_to_string(path).map_err(|source| ConfigError::ReadConfig {
            path: path.to_path_buf(),
            source,
        })?;
        let existing_value: toml::Value =
            toml::from_str(&raw).map_err(|source| ConfigError::ParseConfig {
                path: path.to_path_buf(),
                source,
            })?;
        merge_toml_values(&mut merged_value, existing_value);
    }

    let config: AppConfig =
        merged_value
            .clone()
            .try_into()
            .map_err(|source| ConfigError::ParseConfig {
                path: path.to_path_buf(),
                source,
            })?;
    validate(&config)?;

    let rendered = toml::to_string_pretty(&merged_value).expect("merged config should serialize");
    fs::write(path, rendered).map_err(ConfigError::WriteConfig)?;

    Ok(MigratedConfig {
        path: path.to_path_buf(),
        created_file,
    })
}

fn reset_path_to_defaults(path: &Path) -> Result<MigratedConfig, ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(ConfigError::CreateDir)?;
    }
    let created_file = !path.exists();
    fs::write(path, default_config_template()).map_err(ConfigError::WriteConfig)?;
    Ok(MigratedConfig {
        path: path.to_path_buf(),
        created_file,
    })
}

fn merge_toml_values(base: &mut toml::Value, overlay: toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base_table), toml::Value::Table(overlay_table)) => {
            for (key, overlay_value) in overlay_table {
                if let Some(base_value) = base_table.get_mut(&key) {
                    merge_toml_values(base_value, overlay_value);
                } else {
                    base_table.insert(key, overlay_value);
                }
            }
        }
        (base_value, overlay_value) => {
            *base_value = overlay_value;
        }
    }
}

fn validate(config: &AppConfig) -> Result<(), ConfigError> {
    if config.model_provider.trim().is_empty() {
        return Err(ConfigError::InvalidConfig(
            "model_provider cannot be empty".to_string(),
        ));
    }

    let active = config
        .model_providers
        .get(&config.model_provider)
        .ok_or_else(|| {
            ConfigError::InvalidConfig(format!(
                "model_provider '{}' not found in model_providers",
                config.model_provider
            ))
        })?;

    if active.base_url.trim().is_empty() {
        return Err(ConfigError::InvalidConfig(format!(
            "provider '{}' base_url cannot be empty",
            config.model_provider
        )));
    }
    if active.default_model.trim().is_empty() {
        return Err(ConfigError::InvalidConfig(format!(
            "provider '{}' default_model cannot be empty",
            config.model_provider
        )));
    }
    if active.wire_api.trim().is_empty() {
        return Err(ConfigError::InvalidConfig(format!(
            "provider '{}' wire_api cannot be empty",
            config.model_provider
        )));
    }

    if config.memory.embedding.enabled {
        if config.memory.embedding.provider.trim().is_empty() {
            return Err(ConfigError::InvalidConfig(
                "memory.embedding.provider cannot be empty when memory.embedding.enabled=true"
                    .to_string(),
            ));
        }
        if config.memory.embedding.model.trim().is_empty() {
            return Err(ConfigError::InvalidConfig(
                "memory.embedding.model cannot be empty when memory.embedding.enabled=true"
                    .to_string(),
            ));
        }
        if !config
            .model_providers
            .contains_key(&config.memory.embedding.provider)
        {
            return Err(ConfigError::InvalidConfig(format!(
                "memory.embedding.provider '{}' not found in model_providers",
                config.memory.embedding.provider
            )));
        }
    }

    if config.mcp.startup_timeout_seconds == 0 {
        return Err(ConfigError::InvalidConfig(
            "mcp.startup_timeout_seconds must be greater than 0".to_string(),
        ));
    }
    let mut mcp_ids = std::collections::BTreeSet::new();
    for server in &config.mcp.servers {
        if server.id.trim().is_empty() {
            return Err(ConfigError::InvalidConfig(
                "mcp.servers.id cannot be empty".to_string(),
            ));
        }
        if !mcp_ids.insert(server.id.trim().to_string()) {
            return Err(ConfigError::InvalidConfig(format!(
                "mcp.servers contains duplicated id '{}'",
                server.id
            )));
        }
        match server.mode {
            McpServerMode::Stdio => {
                let command = server.command.as_deref().map(str::trim).unwrap_or_default();
                if command.is_empty() {
                    return Err(ConfigError::InvalidConfig(format!(
                        "mcp.servers '{}' requires non-empty command when mode=stdio",
                        server.id
                    )));
                }
            }
            McpServerMode::Sse => {
                let url = server.url.as_deref().map(str::trim).unwrap_or_default();
                if url.is_empty() {
                    return Err(ConfigError::InvalidConfig(format!(
                        "mcp.servers '{}' requires non-empty url when mode=sse",
                        server.id
                    )));
                }
                let parsed = url::Url::parse(url).map_err(|err| {
                    ConfigError::InvalidConfig(format!(
                        "mcp.servers '{}' has invalid url '{}': {}",
                        server.id, url, err
                    ))
                })?;
                let scheme = parsed.scheme();
                if scheme != "http" && scheme != "https" {
                    return Err(ConfigError::InvalidConfig(format!(
                        "mcp.servers '{}' url scheme must be http or https",
                        server.id
                    )));
                }
            }
        }
    }

    if config.tools.web_search.enabled {
        if config.tools.web_search.provider.trim().is_empty() {
            return Err(ConfigError::InvalidConfig(
                "tools.web_search.provider cannot be empty when enabled".to_string(),
            ));
        }

        match config.tools.web_search.provider.as_str() {
            "tavily" => {
                if !has_tavily_web_search_key_source(&config.tools.web_search.tavily) {
                    return Err(ConfigError::InvalidConfig(
                        "tools.web_search.tavily requires api_key or env_key".to_string(),
                    ));
                }
            }
            "brave" => {
                if !has_brave_web_search_key_source(&config.tools.web_search.brave) {
                    return Err(ConfigError::InvalidConfig(
                        "tools.web_search.brave requires api_key or env_key".to_string(),
                    ));
                }
            }
            other => {
                return Err(ConfigError::InvalidConfig(format!(
                    "tools.web_search.provider '{}' is not supported, expected one of: tavily, brave",
                    other
                )));
            }
        }
    }

    if config.tools.web_fetch.enabled {
        if config.tools.web_fetch.max_chars == 0 {
            return Err(ConfigError::InvalidConfig(
                "tools.web_fetch.max_chars must be greater than 0".to_string(),
            ));
        }
        if config.tools.web_fetch.timeout_seconds == 0 {
            return Err(ConfigError::InvalidConfig(
                "tools.web_fetch.timeout_seconds must be greater than 0".to_string(),
            ));
        }
    }

    if config.tools.sub_agent.max_iterations == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.sub_agent.max_iterations must be greater than 0".to_string(),
        ));
    }
    if config.tools.sub_agent.max_tool_calls == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.sub_agent.max_tool_calls must be greater than 0".to_string(),
        ));
    }
    if config
        .skills
        .sources
        .iter()
        .any(|source| source.name.trim().is_empty() || source.address.trim().is_empty())
    {
        return Err(ConfigError::InvalidConfig(
            "skills.sources name/address cannot be empty".to_string(),
        ));
    }
    {
        let mut names = std::collections::BTreeSet::new();
        for source in &config.skills.sources {
            let inserted = names.insert(source.name.trim().to_string());
            if !inserted {
                return Err(ConfigError::InvalidConfig(format!(
                    "skills.sources contains duplicated name '{}'",
                    source.name
                )));
            }
        }
    }
    {
        let source_names: std::collections::BTreeSet<String> = config
            .skills
            .sources
            .iter()
            .map(|source| source.name.trim().to_string())
            .collect();
        let mut pairs = std::collections::BTreeSet::new();
        for installed in &config.skills.installed {
            let registry = installed.registry.trim();
            let name = installed.name.trim();
            if registry.is_empty() || name.is_empty() {
                return Err(ConfigError::InvalidConfig(
                    "skills.installed registry/name cannot be empty".to_string(),
                ));
            }
            if !source_names.contains(registry) {
                return Err(ConfigError::InvalidConfig(format!(
                    "skills.installed references unknown registry '{}'",
                    installed.registry
                )));
            }
            if !pairs.insert((registry.to_string(), name.to_string())) {
                return Err(ConfigError::InvalidConfig(format!(
                    "skills.installed contains duplicated entry '{}/{}'",
                    registry, name
                )));
            }
        }
    }
    if config.cron.tick_ms == 0 {
        return Err(ConfigError::InvalidConfig(
            "cron.tick_ms must be greater than 0".to_string(),
        ));
    }
    if config.cron.runtime_tick_ms == 0 {
        return Err(ConfigError::InvalidConfig(
            "cron.runtime_tick_ms must be greater than 0".to_string(),
        ));
    }
    if config.cron.runtime_drain_batch == 0 {
        return Err(ConfigError::InvalidConfig(
            "cron.runtime_drain_batch must be greater than 0".to_string(),
        ));
    }
    if config.cron.batch_limit <= 0 {
        return Err(ConfigError::InvalidConfig(
            "cron.batch_limit must be greater than 0".to_string(),
        ));
    }
    if config.tools.memory.search_limit == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.memory.search_limit must be greater than 0".to_string(),
        ));
    }
    if config.tools.memory.fts_limit == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.memory.fts_limit must be greater than 0".to_string(),
        ));
    }
    if config.tools.memory.vector_limit == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.memory.vector_limit must be greater than 0".to_string(),
        ));
    }
    if config.tools.shell.safe_commands.is_empty() {
        return Err(ConfigError::InvalidConfig(
            "tools.shell.safe_commands must contain at least one command".to_string(),
        ));
    }
    if config.tools.shell.max_timeout_ms == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.shell.max_timeout_ms must be greater than 0".to_string(),
        ));
    }
    if config.tools.shell.max_output_bytes == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.shell.max_output_bytes must be greater than 0".to_string(),
        ));
    }

    Ok(())
}

fn has_tavily_web_search_key_source(provider: &TavilyWebSearchConfig) -> bool {
    provider
        .api_key
        .as_ref()
        .is_some_and(|v| !v.trim().is_empty())
        || provider
            .env_key
            .as_ref()
            .is_some_and(|v| !v.trim().is_empty())
}

fn has_brave_web_search_key_source(provider: &BraveWebSearchConfig) -> bool {
    provider
        .api_key
        .as_ref()
        .is_some_and(|v| !v.trim().is_empty())
        || provider
            .env_key
            .as_ref()
            .is_some_and(|v| !v.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn parse_default_template_succeeds() {
        let template = default_config_template();
        let parsed: AppConfig = toml::from_str(&template).expect("default template should parse");
        assert_eq!(parsed.model_provider, "openai");
        assert!(parsed.model_providers.contains_key("openai"));
        assert!(!parsed.memory.embedding.enabled);
        assert_eq!(parsed.memory.embedding.provider, "openai");
        assert_eq!(parsed.memory.embedding.model, "text-embedding-3-small");
        assert_eq!(
            parsed.tools.shell.blocked_patterns,
            default_shell_blocked_patterns()
        );
        assert_eq!(
            parsed.tools.shell.safe_commands,
            default_shell_safe_commands()
        );
        assert_eq!(
            parsed.tools.shell.approval_policy,
            ShellApprovalPolicy::OnRequest
        );
        assert!(parsed.tools.shell.allow_login_shell);
        assert_eq!(parsed.tools.shell.max_timeout_ms, 120_000);
        assert_eq!(parsed.tools.shell.max_output_bytes, 128 * 1024);
        assert!(parsed.tools.memory.enabled);
        assert_eq!(parsed.tools.memory.search_limit, 8);
        assert_eq!(parsed.tools.memory.fts_limit, 20);
        assert_eq!(parsed.tools.memory.vector_limit, 20);
        assert!(parsed.tools.memory.use_vector);
        assert!(parsed.tools.web_fetch.enabled);
        assert_eq!(parsed.tools.web_fetch.max_chars, 50_000);
        assert_eq!(parsed.tools.web_fetch.timeout_seconds, 15);
        assert_eq!(parsed.tools.web_fetch.cache_ttl_minutes, 10);
        assert_eq!(parsed.tools.web_fetch.max_redirects, 3);
        assert!(parsed.tools.web_fetch.readability);
        assert!(parsed.tools.web_search.enabled);
        assert_eq!(parsed.tools.web_search.provider, "tavily");
        assert_eq!(
            parsed.tools.web_search.tavily.env_key.as_deref(),
            Some("TAVILY_API_KEY")
        );
        assert_eq!(
            parsed.tools.web_search.brave.env_key.as_deref(),
            Some("BRAVE_SEARCH_API_KEY")
        );
        assert!(parsed.tools.sub_agent.enabled);
        assert_eq!(parsed.tools.sub_agent.max_iterations, 6);
        assert_eq!(parsed.tools.sub_agent.max_tool_calls, 12);
        assert!(!parsed.skills.sources.is_empty());
        assert_eq!(
            parsed
                .skills
                .sources
                .iter()
                .find(|source| source.name == "anthropic")
                .map(|source| source.address.as_str()),
            Some("https://github.com/anthropics/skills")
        );
        assert!(parsed.skills.installed.is_empty());
        assert_eq!(parsed.cron.tick_ms, 1_000);
        assert_eq!(parsed.cron.runtime_tick_ms, 200);
        assert_eq!(parsed.cron.runtime_drain_batch, 8);
        assert_eq!(parsed.cron.batch_limit, 64);
        assert_eq!(
            parsed.tools.sub_agent.exclude_tools,
            vec!["sub_agent".to_string()]
        );
        assert!(parsed.mcp.enabled);
        assert_eq!(parsed.mcp.startup_timeout_seconds, 30);
        assert!(parsed.mcp.servers.is_empty());
        validate(&parsed).expect("default template should be valid");
    }

    #[test]
    fn validate_fails_when_active_provider_missing() {
        let cfg = AppConfig {
            model_provider: "missing".to_string(),
            model_providers: BTreeMap::new(),
            memory: MemoryConfig::default(),
            mcp: McpConfig::default(),
            tools: ToolsConfig::default(),
            cron: CronConfig::default(),
            skills: SkillsConfig::default(),
        };
        let err = validate(&cfg).expect_err("should fail");
        assert!(format!("{err}").contains("not found in model_providers"));
    }

    #[test]
    fn parse_tools_shell_blocked_patterns_succeeds() {
        let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[tools.shell]
blocked_patterns = ["sudo rm -rf /tmp/example"]
safe_commands = ["ls", "cat"]
approval_policy = "never"
allow_login_shell = false
max_timeout_ms = 30000
max_output_bytes = 64000
"#;

        let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
        assert_eq!(
            parsed.tools.shell.blocked_patterns,
            vec!["sudo rm -rf /tmp/example".to_string()]
        );
        assert_eq!(
            parsed.tools.shell.safe_commands,
            vec!["ls".to_string(), "cat".to_string()]
        );
        assert_eq!(
            parsed.tools.shell.approval_policy,
            ShellApprovalPolicy::Never
        );
        assert!(!parsed.tools.shell.allow_login_shell);
        assert_eq!(parsed.tools.shell.max_timeout_ms, 30_000);
        assert_eq!(parsed.tools.shell.max_output_bytes, 64_000);
    }

    #[test]
    fn parse_tools_web_search_succeeds() {
        let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[tools.web_search]
enabled = false
provider = "tavily"

[tools.web_search.tavily]
base_url = "https://api.tavily.com"
env_key = "TAVILY_API_KEY"
"#;

        let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
        assert_eq!(parsed.tools.web_search.provider, "tavily");
        assert_eq!(parsed.tools.web_search.enabled, false);
        assert_eq!(
            parsed.tools.web_search.tavily.base_url.as_deref(),
            Some("https://api.tavily.com")
        );
    }

    #[test]
    fn parse_tools_web_fetch_succeeds() {
        let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[tools.web_fetch]
enabled = true
max_chars = 12000
timeout_seconds = 20
cache_ttl_minutes = 30
max_redirects = 2
readability = true
ssrf_allowlist = ["172.22.0.0/16"]
"#;

        let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
        assert!(parsed.tools.web_fetch.enabled);
        assert_eq!(parsed.tools.web_fetch.max_chars, 12000);
        assert_eq!(parsed.tools.web_fetch.timeout_seconds, 20);
        assert_eq!(parsed.tools.web_fetch.cache_ttl_minutes, 30);
        assert_eq!(parsed.tools.web_fetch.max_redirects, 2);
        assert!(parsed.tools.web_fetch.readability);
        assert_eq!(
            parsed.tools.web_fetch.ssrf_allowlist,
            vec!["172.22.0.0/16".to_string()]
        );
    }

    #[test]
    fn parse_mcp_servers_succeeds() {
        let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[mcp]
enabled = true
startup_timeout_seconds = 30

[[mcp.servers]]
id = "filesystem"
enabled = true
mode = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
cwd = "/tmp"

[mcp.servers.env]
NODE_ENV = "production"

[[mcp.servers]]
id = "remote"
enabled = true
mode = "sse"
url = "https://mcp.example.com/sse"

[mcp.servers.headers]
Authorization = "Bearer test"
"#;

        let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
        assert!(parsed.mcp.enabled);
        assert_eq!(parsed.mcp.startup_timeout_seconds, 30);
        assert_eq!(parsed.mcp.servers.len(), 2);
        assert_eq!(parsed.mcp.servers[0].mode, McpServerMode::Stdio);
        assert_eq!(parsed.mcp.servers[0].command.as_deref(), Some("npx"),);
        assert_eq!(
            parsed.mcp.servers[1].url.as_deref(),
            Some("https://mcp.example.com/sse")
        );
        assert_eq!(
            parsed.mcp.servers[1].headers.get("Authorization"),
            Some(&"Bearer test".to_string())
        );
    }

    #[test]
    fn validate_fails_when_web_fetch_limits_are_invalid() {
        let mut cfg = AppConfig::default();
        cfg.tools.web_fetch.enabled = true;
        cfg.tools.web_fetch.max_chars = 0;
        let err = validate(&cfg).expect_err("should fail");
        assert!(format!("{err}").contains("tools.web_fetch.max_chars"));

        let mut cfg2 = AppConfig::default();
        cfg2.tools.web_fetch.enabled = true;
        cfg2.tools.web_fetch.timeout_seconds = 0;
        let err2 = validate(&cfg2).expect_err("should fail");
        assert!(format!("{err2}").contains("tools.web_fetch.timeout_seconds"));
    }

    #[test]
    fn validate_fails_when_mcp_timeout_is_zero() {
        let mut cfg = AppConfig::default();
        cfg.mcp.startup_timeout_seconds = 0;
        let err = validate(&cfg).expect_err("should fail");
        assert!(format!("{err}").contains("mcp.startup_timeout_seconds"));
    }

    #[test]
    fn validate_fails_when_mcp_server_ids_duplicate() {
        let mut cfg = AppConfig::default();
        cfg.mcp.servers = vec![
            McpServerConfig {
                id: "dup".to_string(),
                enabled: true,
                mode: McpServerMode::Stdio,
                command: Some("echo".to_string()),
                args: vec![],
                env: BTreeMap::new(),
                cwd: None,
                url: None,
                headers: BTreeMap::new(),
            },
            McpServerConfig {
                id: "dup".to_string(),
                enabled: true,
                mode: McpServerMode::Sse,
                command: None,
                args: vec![],
                env: BTreeMap::new(),
                cwd: None,
                url: Some("https://example.com/sse".to_string()),
                headers: BTreeMap::new(),
            },
        ];
        let err = validate(&cfg).expect_err("should fail");
        assert!(format!("{err}").contains("duplicated id"));
    }

    #[test]
    fn validate_fails_when_stdio_server_command_missing() {
        let mut cfg = AppConfig::default();
        cfg.mcp.servers = vec![McpServerConfig {
            id: "stdio".to_string(),
            enabled: true,
            mode: McpServerMode::Stdio,
            command: Some("".to_string()),
            args: vec![],
            env: BTreeMap::new(),
            cwd: None,
            url: None,
            headers: BTreeMap::new(),
        }];
        let err = validate(&cfg).expect_err("should fail");
        assert!(format!("{err}").contains("mode=stdio"));
    }

    #[test]
    fn validate_fails_when_sse_server_url_invalid() {
        let mut cfg = AppConfig::default();
        cfg.mcp.servers = vec![McpServerConfig {
            id: "sse".to_string(),
            enabled: true,
            mode: McpServerMode::Sse,
            command: None,
            args: vec![],
            env: BTreeMap::new(),
            cwd: None,
            url: Some("ftp://example.com/sse".to_string()),
            headers: BTreeMap::new(),
        }];
        let err = validate(&cfg).expect_err("should fail");
        assert!(format!("{err}").contains("http or https"));
    }

    #[test]
    fn validate_fails_when_web_search_provider_missing() {
        let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[tools.web_search]
enabled = true
provider = "missing"
"#;
        let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
        let err = validate(&parsed).expect_err("should fail");
        assert!(format!("{err}").contains("is not supported"));
    }

    #[test]
    fn validate_fails_when_sub_agent_limits_are_zero() {
        let mut cfg = AppConfig::default();
        cfg.tools.sub_agent.max_iterations = 0;
        let err = validate(&cfg).expect_err("should fail");
        assert!(format!("{err}").contains("max_iterations"));

        let mut cfg2 = AppConfig::default();
        cfg2.tools.sub_agent.max_tool_calls = 0;
        let err2 = validate(&cfg2).expect_err("should fail");
        assert!(format!("{err2}").contains("max_tool_calls"));
    }

    #[test]
    fn validate_fails_when_skills_config_invalid() {
        let mut cfg = AppConfig::default();
        cfg.skills.sources.push(SkillSourceConfig {
            name: String::new(),
            address: "https://github.com/anthropics/skills".to_string(),
        });
        let err = validate(&cfg).expect_err("should fail");
        assert!(format!("{err}").contains("skills.sources"));

        let mut cfg2 = AppConfig::default();
        cfg2.skills.sources.push(SkillSourceConfig {
            name: "empty-address".to_string(),
            address: String::new(),
        });
        let err2 = validate(&cfg2).expect_err("should fail");
        assert!(format!("{err2}").contains("skills.sources"));

        let mut cfg3 = AppConfig::default();
        cfg3.skills.installed.push(InstalledSkillConfig {
            registry: "missing".to_string(),
            name: "code-review".to_string(),
        });
        let err3 = validate(&cfg3).expect_err("should fail");
        assert!(format!("{err3}").contains("unknown registry"));

        let mut cfg4 = AppConfig::default();
        cfg4.skills.installed.push(InstalledSkillConfig {
            registry: "anthropic".to_string(),
            name: "code-review".to_string(),
        });
        cfg4.skills.installed.push(InstalledSkillConfig {
            registry: "anthropic".to_string(),
            name: "code-review".to_string(),
        });
        let err4 = validate(&cfg4).expect_err("should fail");
        assert!(format!("{err4}").contains("duplicated entry"));
    }

    #[test]
    fn validate_fails_when_cron_limits_are_zero() {
        let mut cfg = AppConfig::default();
        cfg.cron.tick_ms = 0;
        let err = validate(&cfg).expect_err("should fail");
        assert!(format!("{err}").contains("cron.tick_ms"));

        let mut cfg2 = AppConfig::default();
        cfg2.cron.runtime_tick_ms = 0;
        let err2 = validate(&cfg2).expect_err("should fail");
        assert!(format!("{err2}").contains("cron.runtime_tick_ms"));

        let mut cfg3 = AppConfig::default();
        cfg3.cron.runtime_drain_batch = 0;
        let err3 = validate(&cfg3).expect_err("should fail");
        assert!(format!("{err3}").contains("cron.runtime_drain_batch"));

        let mut cfg4 = AppConfig::default();
        cfg4.cron.batch_limit = 0;
        let err4 = validate(&cfg4).expect_err("should fail");
        assert!(format!("{err4}").contains("cron.batch_limit"));
    }

    #[test]
    fn validate_fails_when_memory_tool_limits_are_zero() {
        let mut cfg = AppConfig::default();
        cfg.tools.memory.search_limit = 0;
        let err = validate(&cfg).expect_err("should fail");
        assert!(format!("{err}").contains("tools.memory.search_limit"));

        let mut cfg2 = AppConfig::default();
        cfg2.tools.memory.fts_limit = 0;
        let err2 = validate(&cfg2).expect_err("should fail");
        assert!(format!("{err2}").contains("tools.memory.fts_limit"));

        let mut cfg3 = AppConfig::default();
        cfg3.tools.memory.vector_limit = 0;
        let err3 = validate(&cfg3).expect_err("should fail");
        assert!(format!("{err3}").contains("tools.memory.vector_limit"));
    }

    #[test]
    fn validate_fails_when_shell_limits_are_invalid() {
        let mut cfg = AppConfig::default();
        cfg.tools.shell.safe_commands.clear();
        let err = validate(&cfg).expect_err("should fail");
        assert!(format!("{err}").contains("tools.shell.safe_commands"));

        let mut cfg2 = AppConfig::default();
        cfg2.tools.shell.max_timeout_ms = 0;
        let err2 = validate(&cfg2).expect_err("should fail");
        assert!(format!("{err2}").contains("tools.shell.max_timeout_ms"));

        let mut cfg3 = AppConfig::default();
        cfg3.tools.shell.max_output_bytes = 0;
        let err3 = validate(&cfg3).expect_err("should fail");
        assert!(format!("{err3}").contains("tools.shell.max_output_bytes"));
    }

    #[test]
    fn validate_fails_when_memory_embedding_provider_missing() {
        let mut cfg = AppConfig::default();
        cfg.memory.embedding.enabled = true;
        cfg.memory.embedding.provider = String::new();
        let err = validate(&cfg).expect_err("should fail");
        assert!(format!("{err}").contains("memory.embedding.provider cannot be empty when"));
    }

    #[test]
    fn validate_fails_when_memory_embedding_model_missing() {
        let mut cfg = AppConfig::default();
        cfg.memory.embedding.enabled = true;
        cfg.memory.embedding.model = String::new();
        let err = validate(&cfg).expect_err("should fail");
        assert!(format!("{err}").contains("memory.embedding.model cannot be empty when"));
    }

    #[test]
    fn validate_fails_when_memory_embedding_provider_not_found() {
        let mut cfg = AppConfig::default();
        cfg.memory.embedding.enabled = true;
        cfg.memory.embedding.provider = "missing".to_string();
        let err = validate(&cfg).expect_err("should fail");
        assert!(format!("{err}").contains("not found in model_providers"));
    }

    #[test]
    fn validate_allows_missing_embedding_provider_when_disabled() {
        let mut cfg = AppConfig::default();
        cfg.memory.embedding.enabled = false;
        cfg.memory.embedding.provider = String::new();
        cfg.memory.embedding.model = String::new();
        validate(&cfg).expect("should be valid when embedding disabled");
    }

    #[test]
    fn load_from_path_creates_default_and_reloads() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = env::temp_dir().join(format!("klaw-config-test-{suffix}"));
        let path = root.join("config.toml");

        let loaded = load_from_path(&path, true).expect("should create and load");
        assert!(loaded.created_default);
        assert!(path.exists());

        let loaded2 = load_from_path(&path, false).expect("should reload");
        assert!(!loaded2.created_default);

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_path_with_defaults_creates_file_when_missing() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = env::temp_dir().join(format!("klaw-config-migrate-test-{suffix}"));
        let path = root.join("config.toml");

        let migrated = migrate_path_with_defaults(&path).expect("should create and migrate");
        assert!(migrated.created_file);
        assert!(path.exists());

        let loaded = load_from_path(&path, false).expect("migrated file should load");
        assert_eq!(loaded.config.model_provider, "openai");

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_path_with_defaults_merges_existing_and_preserves_unknown_keys() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = env::temp_dir().join(format!("klaw-config-migrate-test-{suffix}"));
        let path = root.join("config.toml");

        let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4.1-mini"
env_key = "OPENAI_API_KEY"

[tools.web_fetch]
max_chars = 12000

[custom]
flag = true
"#;
        fs::create_dir_all(&root).expect("should create temp root");
        fs::write(&path, raw).expect("should write source config");

        let migrated =
            migrate_path_with_defaults(&path).expect("should merge defaults with existing");
        assert!(!migrated.created_file);

        let loaded = load_from_path(&path, false).expect("migrated file should load");
        assert_eq!(
            loaded.config.model_providers["openai"].default_model,
            "gpt-4.1-mini"
        );
        assert_eq!(loaded.config.tools.web_fetch.max_chars, 12000);
        assert!(loaded.config.tools.memory.enabled);

        let merged_raw = fs::read_to_string(&path).expect("should read migrated config");
        let merged_value: toml::Value =
            toml::from_str(&merged_raw).expect("migrated toml should parse");
        assert_eq!(
            merged_value["custom"]["flag"].as_bool(),
            Some(true),
            "unknown keys should be preserved"
        );

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reset_path_to_defaults_overwrites_existing_config() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = env::temp_dir().join(format!("klaw-config-reset-test-{suffix}"));
        let path = root.join("config.toml");

        let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4.1-mini"
env_key = "OPENAI_API_KEY"
"#;
        fs::create_dir_all(&root).expect("should create temp root");
        fs::write(&path, raw).expect("should write source config");

        let migrated = reset_path_to_defaults(&path).expect("should reset to defaults");
        assert!(!migrated.created_file);

        let loaded = load_from_path(&path, false).expect("reset file should load");
        assert_eq!(
            loaded.config.model_providers["openai"].default_model,
            "gpt-4o-mini"
        );

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&root);
    }
}
