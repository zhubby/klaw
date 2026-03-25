use crate::{AppConfig, ConfigError, validate};
use klaw_util::{config_path, default_data_dir};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

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

#[derive(Debug, Clone)]
pub struct ConfigSnapshot {
    pub path: PathBuf,
    pub config: AppConfig,
    pub raw_toml: String,
    pub revision: u64,
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    inner: Arc<RwLock<ConfigSnapshot>>,
}

impl ConfigStore {
    pub fn open(config_path: Option<&Path>) -> Result<Self, ConfigError> {
        let loaded = load_or_init(config_path)?;
        let raw_toml =
            fs::read_to_string(&loaded.path).map_err(|source| ConfigError::ReadConfig {
                path: loaded.path.clone(),
                source,
            })?;
        let snapshot = ConfigSnapshot {
            path: loaded.path,
            config: loaded.config,
            raw_toml,
            revision: 1,
        };
        Ok(Self {
            inner: Arc::new(RwLock::new(snapshot)),
        })
    }

    pub fn snapshot(&self) -> ConfigSnapshot {
        self.inner
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    pub fn save_raw_toml(&self, raw: &str) -> Result<ConfigSnapshot, ConfigError> {
        let path = self
            .inner
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .path
            .clone();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(ConfigError::CreateDir)?;
        }
        let config = parse_and_validate_config(&path, raw)?;
        fs::write(&path, raw).map_err(ConfigError::WriteConfig)?;
        let mut guard = self.inner.write().unwrap_or_else(|err| err.into_inner());
        let next_revision = guard.revision.saturating_add(1);
        *guard = ConfigSnapshot {
            path,
            config,
            raw_toml: raw.to_string(),
            revision: next_revision,
        };
        Ok(guard.clone())
    }

    pub fn update_config<F, T>(&self, mutate: F) -> Result<(ConfigSnapshot, T), ConfigError>
    where
        F: FnOnce(&mut AppConfig) -> Result<T, ConfigError>,
    {
        let mut guard = self.inner.write().unwrap_or_else(|err| err.into_inner());
        let path = guard.path.clone();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(ConfigError::CreateDir)?;
        }

        let raw_toml = read_raw_toml(&path)?;
        let mut config = parse_and_validate_config(&path, &raw_toml)?;
        let result = mutate(&mut config)?;
        validate(&config)?;

        let raw_toml = toml::to_string_pretty(&config)
            .map_err(|err| ConfigError::SerializeConfig(err.to_string()))?;
        fs::write(&path, &raw_toml).map_err(ConfigError::WriteConfig)?;

        let next_revision = guard.revision.saturating_add(1);
        *guard = ConfigSnapshot {
            path,
            config,
            raw_toml,
            revision: next_revision,
        };
        Ok((guard.clone(), result))
    }

    pub fn validate_raw_toml(&self, raw: &str) -> Result<(), ConfigError> {
        let path = self
            .inner
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .path
            .clone();
        parse_and_validate_config(&path, raw)?;
        Ok(())
    }

    pub fn reload(&self) -> Result<ConfigSnapshot, ConfigError> {
        let mut guard = self.inner.write().unwrap_or_else(|err| err.into_inner());
        let path = guard.path.clone();
        let raw_toml = read_raw_toml(&path)?;
        let config = parse_and_validate_config(&path, &raw_toml)?;
        let next_revision = guard.revision.saturating_add(1);
        *guard = ConfigSnapshot {
            path,
            config,
            raw_toml,
            revision: next_revision,
        };
        Ok(guard.clone())
    }

    pub fn reset_to_defaults(&self) -> Result<ConfigSnapshot, ConfigError> {
        let path = self
            .inner
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .path
            .clone();
        reset_path_to_defaults(&path)?;
        self.reload()
    }

    pub fn migrate_with_defaults(&self) -> Result<ConfigSnapshot, ConfigError> {
        let path = self
            .inner
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .path
            .clone();
        migrate_path_with_defaults(&path)?;
        self.reload()
    }

    pub fn save_observability_config(
        &self,
        observability: &crate::ObservabilityConfig,
    ) -> Result<ConfigSnapshot, ConfigError> {
        self.update_config(|config| {
            config.observability = observability.clone();
            Ok(())
        })
        .map(|(snapshot, ())| snapshot)
    }
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

pub fn validate_config_file(config_path: Option<&Path>) -> Result<PathBuf, ConfigError> {
    let explicit = config_path.map(Path::to_path_buf);
    let path = match explicit {
        Some(path) => path,
        None => default_config_path()?,
    };

    load_from_path(&path, false)?;
    Ok(path)
}

pub fn default_config_path() -> Result<PathBuf, ConfigError> {
    let root_dir = default_data_dir().ok_or(ConfigError::HomeDirUnavailable)?;
    Ok(config_path(root_dir))
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

pub(crate) fn load_from_path(
    path: &Path,
    create_if_missing: bool,
) -> Result<LoadedConfig, ConfigError> {
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

fn read_raw_toml(path: &Path) -> Result<String, ConfigError> {
    fs::read_to_string(path).map_err(|source| ConfigError::ReadConfig {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn migrate_path_with_defaults(path: &Path) -> Result<MigratedConfig, ConfigError> {
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

pub(crate) fn reset_path_to_defaults(path: &Path) -> Result<MigratedConfig, ConfigError> {
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

fn parse_and_validate_config(path: &Path, raw: &str) -> Result<AppConfig, ConfigError> {
    let config: AppConfig = toml::from_str(raw).map_err(|source| ConfigError::ParseConfig {
        path: path.to_path_buf(),
        source,
    })?;
    validate(&config)?;
    Ok(config)
}
