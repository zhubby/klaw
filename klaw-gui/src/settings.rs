use klaw_util::{default_data_dir, settings_path as default_settings_path};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const SETTINGS_SCHEMA_VERSION: u32 = 1;

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

// ==================== General Settings ====================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GeneralSettings {
    #[serde(default)]
    pub launch_at_startup: bool,
}

// ==================== Privacy Settings ====================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PrivacySettings {
    // Reserved for future use
}

// ==================== Security Settings ====================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SecuritySettings {
    // Reserved for future use
}

// ==================== Network Settings ====================

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

// ==================== Sync Settings ====================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
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
            SyncItem::Skills,
            SyncItem::Mcp,
            SyncItem::SkillsRegistry,
            SyncItem::GuiSettings,
            SyncItem::Archive,
            SyncItem::UserWorkspace,
            SyncItem::Memory,
            SyncItem::Config,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SyncSettings {
    #[serde(default)]
    pub backup_items: Vec<SyncItem>,
}

// ==================== Persistence ====================

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
    let settings: AppSettings = serde_json::from_str(&raw)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

    // Migrate if needed
    if settings.schema_version < SETTINGS_SCHEMA_VERSION {
        let migrated = AppSettings {
            schema_version: SETTINGS_SCHEMA_VERSION,
            ..settings
        };
        return Ok(migrated);
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
        settings.sync.backup_items = vec![SyncItem::Session, SyncItem::Skills, SyncItem::Config];

        let json = serde_json::to_string_pretty(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.sync.backup_items.len(), 3);
        assert!(restored.sync.backup_items.contains(&SyncItem::Session));
        assert!(restored.sync.backup_items.contains(&SyncItem::Skills));
        assert!(restored.sync.backup_items.contains(&SyncItem::Config));
    }
}
