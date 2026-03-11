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
            tools: ToolsConfig::default(),
        }
    }
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

    Ok(())
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
        assert_eq!(
            parsed.tools.shell.blocked_patterns,
            default_shell_blocked_patterns()
        );
        validate(&parsed).expect("default template should be valid");
    }

    #[test]
    fn validate_fails_when_active_provider_missing() {
        let cfg = AppConfig {
            model_provider: "missing".to_string(),
            model_providers: BTreeMap::new(),
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
