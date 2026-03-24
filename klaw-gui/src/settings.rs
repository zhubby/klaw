use klaw_util::{default_data_dir, settings_path as default_settings_path};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const SETTINGS_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppSettings {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub general: GeneralSettings,
    #[serde(default)]
    pub privacy: PrivacySettings,
    #[serde(default)]
    pub security: SecuritySettings,
    #[serde(default)]
    pub network: NetworkSettings,
    #[serde(default)]
    pub sync: SyncSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            general: GeneralSettings::default(),
            privacy: PrivacySettings::default(),
            security: SecuritySettings::default(),
            network: NetworkSettings::default(),
            sync: SyncSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GeneralSettings {
    #[serde(default)]
    pub launch_at_startup: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PrivacySettings {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SecuritySettings {}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    #[default]
    NoProxy,
    SystemProxy,
    ManualProxy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct NetworkSettings {
    #[serde(default)]
    pub proxy_mode: ProxyMode,
    #[serde(default)]
    pub http_proxy: ProxyConfig,
    #[serde(default)]
    pub https_proxy: ProxyConfig,
    #[serde(default)]
    pub socks5_proxy: ProxyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyConfig {
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 0,
        }
    }
}

#[allow(dead_code)]
impl ProxyConfig {
    pub fn is_empty(&self) -> bool {
        self.host.is_empty() || self.port == 0
    }

    pub fn to_address(&self) -> String {
        if self.is_empty() {
            String::new()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default, Ord, PartialOrd,
)]
#[serde(rename_all = "snake_case")]
pub enum SyncItem {
    #[default]
    Session,
    Skills,
    Mcp,
    SkillsRegistry,
    GuiSettings,
    Archive,
    UserWorkspace,
    Memory,
    Config,
}

impl SyncItem {
    pub fn label(&self) -> &'static str {
        match self {
            SyncItem::Session => "Session",
            SyncItem::Skills => "Skills",
            SyncItem::Mcp => "MCP",
            SyncItem::SkillsRegistry => "Skills Registry",
            SyncItem::GuiSettings => "GUI Settings",
            SyncItem::Archive => "Archive",
            SyncItem::UserWorkspace => "User Workspace",
            SyncItem::Memory => "Memory",
            SyncItem::Config => "Config",
        }
    }

    pub fn all() -> &'static [SyncItem] {
        &[
            SyncItem::Session,
            SyncItem::Archive,
            SyncItem::Memory,
            SyncItem::Config,
            SyncItem::GuiSettings,
            SyncItem::Skills,
            SyncItem::SkillsRegistry,
            SyncItem::UserWorkspace,
        ]
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SyncProvider {
    #[default]
    S3,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    #[default]
    ManifestVersioned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncSchedule {
    #[serde(default)]
    pub auto_backup: bool,
    #[serde(default = "default_sync_interval_minutes")]
    pub interval_minutes: u32,
}

impl Default for SyncSchedule {
    fn default() -> Self {
        Self {
            auto_backup: false,
            interval_minutes: default_sync_interval_minutes(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetentionPolicy {
    #[serde(default = "default_keep_last")]
    pub keep_last: u32,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            keep_last: default_keep_last(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct S3SyncConfig {
    #[serde(default)]
    pub endpoint: String,
    #[serde(default = "default_s3_region")]
    pub region: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub access_key: String,
    #[serde(default)]
    pub secret_key: String,
    #[serde(default)]
    pub session_token: String,
    #[serde(default = "default_access_key_env")]
    pub access_key_env: String,
    #[serde(default = "default_secret_key_env")]
    pub secret_key_env: String,
    #[serde(default)]
    pub session_token_env: String,
    #[serde(default)]
    pub force_path_style: bool,
}

impl Default for S3SyncConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            region: default_s3_region(),
            bucket: String::new(),
            prefix: String::new(),
            access_key: String::new(),
            secret_key: String::new(),
            session_token: String::new(),
            access_key_env: default_access_key_env(),
            secret_key_env: default_secret_key_env(),
            session_token_env: String::new(),
            force_path_style: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub provider: SyncProvider,
    #[serde(default)]
    pub mode: SyncMode,
    #[serde(default)]
    pub backup_items: Vec<SyncItem>,
    #[serde(default)]
    pub schedule: SyncSchedule,
    #[serde(default)]
    pub s3: S3SyncConfig,
    #[serde(default)]
    pub retention: RetentionPolicy,
    #[serde(default)]
    pub last_sync_at: Option<i64>,
    #[serde(default)]
    pub last_manifest_id: Option<String>,
    #[serde(default = "default_device_id")]
    pub device_id: String,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: SyncProvider::S3,
            mode: SyncMode::ManifestVersioned,
            backup_items: default_backup_items(),
            schedule: SyncSchedule::default(),
            s3: S3SyncConfig::default(),
            retention: RetentionPolicy::default(),
            last_sync_at: None,
            last_manifest_id: None,
            device_id: default_device_id(),
        }
    }
}

pub fn load_settings() -> AppSettings {
    let Some(path) = settings_path() else {
        return AppSettings::default();
    };
    match load_settings_from_path(&path) {
        Ok(settings) => settings,
        Err(_) => AppSettings::default(),
    }
}

pub fn save_settings(settings: &AppSettings) -> io::Result<()> {
    let Some(path) = settings_path() else {
        return Ok(());
    };
    save_settings_to_path(&path, settings)
}

fn load_settings_from_path(path: &Path) -> io::Result<AppSettings> {
    let raw = fs::read_to_string(path)?;
    let mut settings: AppSettings = serde_json::from_str(&raw)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

    if settings.schema_version < SETTINGS_SCHEMA_VERSION {
        settings.schema_version = SETTINGS_SCHEMA_VERSION;
    }

    sanitize_sync_settings(&mut settings.sync);
    if settings.sync.device_id.trim().is_empty() {
        settings.sync.device_id = default_device_id();
    }
    Ok(settings)
}

fn save_settings_to_path(path: &Path, settings: &AppSettings) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "settings path must have a parent directory",
        ));
    };

    fs::create_dir_all(parent)?;

    let tmp_path = path.with_extension("json.tmp");
    let serialized = serde_json::to_string_pretty(settings)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(&tmp_path, serialized)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

fn settings_path() -> Option<PathBuf> {
    default_data_dir().map(default_settings_path)
}

fn sanitize_sync_settings(sync: &mut SyncSettings) {
    let mut seen = BTreeSet::new();
    sync.backup_items
        .retain(|item| *item != SyncItem::Mcp && seen.insert(*item));
}

fn default_backup_items() -> Vec<SyncItem> {
    vec![
        SyncItem::Session,
        SyncItem::Memory,
        SyncItem::Archive,
        SyncItem::Config,
        SyncItem::GuiSettings,
        SyncItem::Skills,
        SyncItem::UserWorkspace,
    ]
}

fn default_sync_interval_minutes() -> u32 {
    60
}

fn default_keep_last() -> u32 {
    10
}

fn default_s3_region() -> String {
    "us-east-1".to_string()
}

fn default_access_key_env() -> String {
    "AWS_ACCESS_KEY_ID".to_string()
}

fn default_secret_key_env() -> String {
    "AWS_SECRET_ACCESS_KEY".to_string()
}

fn default_device_id() -> String {
    hostname::get()
        .ok()
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "klaw-device".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_roundtrip() {
        let settings = AppSettings::default();
        let json = serde_json::to_string_pretty(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, settings);
    }

    #[test]
    fn proxy_config_to_address() {
        let config = ProxyConfig {
            host: "127.0.0.1".to_string(),
            port: 8080,
        };
        assert_eq!(config.to_address(), "127.0.0.1:8080");

        let empty = ProxyConfig::default();
        assert!(empty.is_empty());
        assert!(empty.to_address().is_empty());
    }

    #[test]
    fn settings_serialization() {
        let mut settings = AppSettings::default();
        settings.general.launch_at_startup = true;
        settings.network.proxy_mode = ProxyMode::ManualProxy;
        settings.network.http_proxy = ProxyConfig {
            host: "proxy.example.com".to_string(),
            port: 3128,
        };

        let json = serde_json::to_string_pretty(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();

        assert!(restored.general.launch_at_startup);
        assert_eq!(restored.network.proxy_mode, ProxyMode::ManualProxy);
        assert_eq!(restored.network.http_proxy.host, "proxy.example.com");
        assert_eq!(restored.network.http_proxy.port, 3128);
    }

    #[test]
    fn sync_settings_serialization() {
        let mut settings = AppSettings::default();
        settings.sync.enabled = true;
        settings.sync.backup_items = vec![SyncItem::Session, SyncItem::Memory, SyncItem::Config];
        settings.sync.s3.bucket = "demo".to_string();
        settings.sync.s3.access_key = "ak".to_string();
        settings.sync.s3.secret_key = "sk".to_string();
        settings.sync.last_manifest_id = Some("manifest-1".to_string());

        let json = serde_json::to_string_pretty(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();

        assert!(restored.sync.enabled);
        assert_eq!(restored.sync.backup_items.len(), 3);
        assert!(restored.sync.backup_items.contains(&SyncItem::Session));
        assert!(restored.sync.backup_items.contains(&SyncItem::Memory));
        assert!(restored.sync.backup_items.contains(&SyncItem::Config));
        assert_eq!(restored.sync.s3.bucket, "demo");
        assert_eq!(restored.sync.s3.access_key, "ak");
        assert_eq!(restored.sync.s3.secret_key, "sk");
        assert_eq!(
            restored.sync.last_manifest_id.as_deref(),
            Some("manifest-1")
        );
    }

    #[test]
    fn sync_settings_migrate_missing_new_fields() {
        let json = r#"{
          "schema_version": 1,
          "sync": {
            "backup_items": ["session", "config"]
          }
        }"#;

        let path = std::env::temp_dir().join("klaw-settings-migrate.json");
        fs::write(&path, json).unwrap();
        let settings = load_settings_from_path(&path).unwrap();

        assert_eq!(settings.schema_version, SETTINGS_SCHEMA_VERSION);
        assert_eq!(settings.sync.provider, SyncProvider::S3);
        assert_eq!(settings.sync.mode, SyncMode::ManifestVersioned);
        assert!(!settings.sync.device_id.is_empty());
    }

    #[test]
    fn sync_settings_strip_mcp_item_on_load() {
        let json = r#"{
          "schema_version": 2,
          "sync": {
            "backup_items": ["session", "mcp", "session", "config"]
          }
        }"#;

        let path = std::env::temp_dir().join("klaw-settings-strip-mcp.json");
        fs::write(&path, json).unwrap();
        let settings = load_settings_from_path(&path).unwrap();

        assert_eq!(
            settings.sync.backup_items,
            vec![SyncItem::Session, SyncItem::Config]
        );
    }
}
