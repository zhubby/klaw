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
    pub tools: ToolsConfig,
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
            tools: ToolsConfig::default(),
        }
    }
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
    pub web_search: WebSearchConfig,
    #[serde(default)]
    pub sub_agent: SubAgentConfig,
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
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            blocked_patterns: default_shell_blocked_patterns(),
        }
    }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchConfig {
    #[serde(default)]
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
            enabled: false,
            provider: default_web_search_provider(),
            tavily: TavilyWebSearchConfig::default(),
            brave: BraveWebSearchConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentConfig {
    #[serde(default)]
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
            enabled: false,
            max_iterations: default_sub_agent_max_iterations(),
            max_tool_calls: default_sub_agent_max_tool_calls(),
            inherit_parent_tools: default_sub_agent_inherit_parent_tools(),
            exclude_tools: default_sub_agent_exclude_tools(),
        }
    }
}

fn default_sub_agent_max_iterations() -> u32 {
    6
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

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot resolve home directory for default config path")]
    HomeDirUnavailable,
    #[error("config file not found: {0}")]
    ConfigNotFound(PathBuf),
    #[error("failed to create config directory: {0}")]
    CreateDir(#[source] std::io::Error),
    #[error("failed to write default config file: {0}")]
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

    if config.tools.web_search.enabled {
        if config.tools.web_search.provider.trim().is_empty() {
            return Err(ConfigError::InvalidConfig(
                "tools.web_search.provider cannot be empty when enabled".to_string(),
            ));
        }

        match config.tools.web_search.provider.as_str() {
            "tavily" => {
                if resolve_tavily_web_search_api_key(&config.tools.web_search.tavily).is_none() {
                    return Err(ConfigError::InvalidConfig(
                        "tools.web_search.tavily requires api_key or env_key".to_string(),
                    ));
                }
            }
            "brave" => {
                if resolve_brave_web_search_api_key(&config.tools.web_search.brave).is_none() {
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

    Ok(())
}

fn resolve_tavily_web_search_api_key(provider: &TavilyWebSearchConfig) -> Option<String> {
    provider.api_key.clone().or_else(|| {
        provider
            .env_key
            .as_ref()
            .and_then(|env_name| env::var(env_name).ok())
    })
}

fn resolve_brave_web_search_api_key(provider: &BraveWebSearchConfig) -> Option<String> {
    provider.api_key.clone().or_else(|| {
        provider
            .env_key
            .as_ref()
            .and_then(|env_name| env::var(env_name).ok())
    })
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
        assert!(parsed.tools.memory.enabled);
        assert_eq!(parsed.tools.memory.search_limit, 8);
        assert_eq!(parsed.tools.memory.fts_limit, 20);
        assert_eq!(parsed.tools.memory.vector_limit, 20);
        assert!(parsed.tools.memory.use_vector);
        assert!(!parsed.tools.web_search.enabled);
        assert_eq!(parsed.tools.web_search.provider, "tavily");
        assert_eq!(
            parsed.tools.web_search.tavily.env_key.as_deref(),
            Some("TAVILY_API_KEY")
        );
        assert_eq!(
            parsed.tools.web_search.brave.env_key.as_deref(),
            Some("BRAVE_SEARCH_API_KEY")
        );
        assert!(!parsed.tools.sub_agent.enabled);
        assert_eq!(parsed.tools.sub_agent.max_iterations, 6);
        assert_eq!(parsed.tools.sub_agent.max_tool_calls, 12);
        assert_eq!(
            parsed.tools.sub_agent.exclude_tools,
            vec!["sub_agent".to_string()]
        );
        validate(&parsed).expect("default template should be valid");
    }

    #[test]
    fn validate_fails_when_active_provider_missing() {
        let cfg = AppConfig {
            model_provider: "missing".to_string(),
            model_providers: BTreeMap::new(),
            memory: MemoryConfig::default(),
            tools: ToolsConfig::default(),
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
"#;

        let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
        assert_eq!(
            parsed.tools.shell.blocked_patterns,
            vec!["sudo rm -rf /tmp/example".to_string()]
        );
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
}
