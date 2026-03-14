use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub model_provider: String,
    #[serde(default)]
    pub model: Option<String>,
    pub model_providers: BTreeMap<String, ModelProviderConfig>,
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub channels: ChannelsConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub cron: CronConfig,
    #[serde(default)]
    pub heartbeat: HeartbeatConfig,
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
            model: None,
            model_providers,
            gateway: GatewayConfig::default(),
            channels: ChannelsConfig::default(),
            memory: MemoryConfig::default(),
            mcp: McpConfig::default(),
            tools: ToolsConfig::default(),
            cron: CronConfig::default(),
            heartbeat: HeartbeatConfig::default(),
            skills: SkillsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelsConfig {
    #[serde(default)]
    pub dingtalk: Vec<DingtalkConfig>,
    #[serde(default)]
    pub disable_session_commands_for: Vec<String>,
}

impl Default for ChannelsConfig {
    fn default() -> Self {
        Self {
            dingtalk: Vec::new(),
            disable_session_commands_for: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DingtalkConfig {
    pub id: String,
    #[serde(default = "default_channel_enabled")]
    pub enabled: bool,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default = "default_dingtalk_bot_title")]
    pub bot_title: String,
    #[serde(default)]
    pub show_reasoning: bool,
    #[serde(default)]
    pub allowlist: Vec<String>,
}

impl Default for DingtalkConfig {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            enabled: default_channel_enabled(),
            client_id: String::new(),
            client_secret: String::new(),
            bot_title: default_dingtalk_bot_title(),
            show_reasoning: false,
            allowlist: Vec::new(),
        }
    }
}

fn default_channel_enabled() -> bool {
    true
}

fn default_dingtalk_bot_title() -> String {
    "Klaw".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    #[serde(default)]
    pub defaults: HeartbeatDefaultsConfig,
    #[serde(default)]
    pub sessions: Vec<HeartbeatSessionConfig>,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            defaults: HeartbeatDefaultsConfig::default(),
            sessions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatDefaultsConfig {
    #[serde(default = "default_heartbeat_enabled")]
    pub enabled: bool,
    #[serde(default = "default_heartbeat_every")]
    pub every: String,
    #[serde(default = "default_heartbeat_prompt")]
    pub prompt: String,
    #[serde(default = "default_heartbeat_silent_ack_token")]
    pub silent_ack_token: String,
    #[serde(default = "default_heartbeat_timezone")]
    pub timezone: String,
}

impl Default for HeartbeatDefaultsConfig {
    fn default() -> Self {
        Self {
            enabled: default_heartbeat_enabled(),
            every: default_heartbeat_every(),
            prompt: default_heartbeat_prompt(),
            silent_ack_token: default_heartbeat_silent_ack_token(),
            timezone: default_heartbeat_timezone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatSessionConfig {
    pub session_key: String,
    pub chat_id: String,
    pub channel: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub every: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub silent_ack_token: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
}

fn default_heartbeat_enabled() -> bool {
    true
}

fn default_heartbeat_every() -> String {
    "30m".to_string()
}

fn default_heartbeat_prompt() -> String {
    "Review the session state. If no user-visible action is needed, reply exactly HEARTBEAT_OK."
        .to_string()
}

fn default_heartbeat_silent_ack_token() -> String {
    "HEARTBEAT_OK".to_string()
}

fn default_heartbeat_timezone() -> String {
    "UTC".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_gateway_listen_ip")]
    pub listen_ip: String,
    #[serde(default = "default_gateway_listen_port")]
    pub listen_port: u16,
    #[serde(default)]
    pub tls: GatewayTlsConfig,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            listen_ip: default_gateway_listen_ip(),
            listen_port: default_gateway_listen_port(),
            tls: GatewayTlsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayTlsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub cert_path: Option<String>,
    #[serde(default)]
    pub key_path: Option<String>,
}

impl Default for GatewayTlsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cert_path: None,
            key_path: None,
        }
    }
}

fn default_gateway_listen_ip() -> String {
    "127.0.0.1".to_string()
}

fn default_gateway_listen_port() -> u16 {
    8080
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
    60
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
    pub apply_patch: ApplyPatchConfig,
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
pub struct ApplyPatchConfig {
    #[serde(default)]
    pub workspace: Option<String>,
    #[serde(default = "default_apply_patch_allow_absolute_paths")]
    pub allow_absolute_paths: bool,
    #[serde(default)]
    pub allowed_roots: Vec<String>,
}

impl Default for ApplyPatchConfig {
    fn default() -> Self {
        Self {
            workspace: None,
            allow_absolute_paths: default_apply_patch_allow_absolute_paths(),
            allowed_roots: Vec::new(),
        }
    }
}

fn default_apply_patch_allow_absolute_paths() -> bool {
    false
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsConfig {
    #[serde(default = "default_skills_sync_timeout")]
    pub sync_timeout: u64,
    #[serde(flatten)]
    pub registries: BTreeMap<String, SkillRegistryConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRegistryConfig {
    pub address: String,
    #[serde(default)]
    pub installed: Vec<String>,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        let mut registries = BTreeMap::new();
        registries.insert(
            "anthropic".to_string(),
            SkillRegistryConfig {
                address: "https://github.com/anthropics/skills".to_string(),
                installed: Vec::new(),
            },
        );
        Self {
            sync_timeout: default_skills_sync_timeout(),
            registries,
        }
    }
}

fn default_skills_sync_timeout() -> u64 {
    60
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
    #[serde(default)]
    pub workspace: Option<String>,
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
            workspace: None,
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

mod io;
mod validate;

pub use io::{
    default_config_path, default_config_template, load_or_init, migrate_with_defaults,
    reset_to_defaults, validate_config_file, LoadedConfig, MigratedConfig,
};
#[cfg(test)]
pub(crate) use io::{load_from_path, migrate_path_with_defaults, reset_path_to_defaults};
pub(crate) use validate::validate;

#[cfg(test)]
mod tests;
