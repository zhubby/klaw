use crate::{validate, AppConfig, ConfigError};
use std::{
    env, fs,
    path::{Path, PathBuf},
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
