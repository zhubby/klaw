use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const KLAW_DIR_NAME: &str = ".klaw";
pub const UTC_TIMEZONE_NAME: &str = "UTC";
pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const SETTINGS_FILE_NAME: &str = "settings.json";
pub const GUI_STATE_FILE_NAME: &str = "gui_state.json";
pub const DB_FILE_NAME: &str = "klaw.db";
pub const MEMORY_DB_FILE_NAME: &str = "memory.db";
pub const ARCHIVE_DB_FILE_NAME: &str = "archive.db";
pub const OBSERVABILITY_DB_FILE_NAME: &str = "observability.db";
pub const TMP_DIR_NAME: &str = "tmp";
pub const WORKSPACE_DIR_NAME: &str = "workspace";
pub const SESSIONS_DIR_NAME: &str = "sessions";
pub const ARCHIVES_DIR_NAME: &str = "archives";
pub const LOGS_DIR_NAME: &str = "logs";
pub const SKILLS_DIR_NAME: &str = "skills";
pub const SKILLS_REGISTRY_DIR_NAME: &str = "skills-registry";
pub const SKILLS_REGISTRY_MANIFEST_FILE_NAME: &str = "skills-registry-manifest.json";
pub const TOKENIZERS_DIR_NAME: &str = "tokenizers";

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub fn data_dir_in_home(home: impl AsRef<Path>) -> PathBuf {
    home.as_ref().join(KLAW_DIR_NAME)
}

pub fn default_data_dir() -> Option<PathBuf> {
    home_dir().map(data_dir_in_home)
}

pub fn system_timezone_name() -> String {
    iana_time_zone::get_timezone()
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| UTC_TIMEZONE_NAME.to_string())
}

pub fn config_path(root_dir: impl AsRef<Path>) -> PathBuf {
    root_dir.as_ref().join(CONFIG_FILE_NAME)
}

pub fn settings_path(root_dir: impl AsRef<Path>) -> PathBuf {
    root_dir.as_ref().join(SETTINGS_FILE_NAME)
}

pub fn gui_state_path(root_dir: impl AsRef<Path>) -> PathBuf {
    root_dir.as_ref().join(GUI_STATE_FILE_NAME)
}

pub fn workspace_dir(root_dir: impl AsRef<Path>) -> PathBuf {
    root_dir.as_ref().join(WORKSPACE_DIR_NAME)
}

pub fn default_workspace_dir() -> Option<PathBuf> {
    default_data_dir().map(workspace_dir)
}

pub fn tokenizer_dir(root_dir: impl AsRef<Path>) -> PathBuf {
    root_dir.as_ref().join(TOKENIZERS_DIR_NAME)
}

pub fn skills_dir(root_dir: impl AsRef<Path>) -> PathBuf {
    root_dir.as_ref().join(SKILLS_DIR_NAME)
}

pub fn skills_registry_dir(root_dir: impl AsRef<Path>) -> PathBuf {
    root_dir.as_ref().join(SKILLS_REGISTRY_DIR_NAME)
}

pub fn skills_registry_manifest_path(root_dir: impl AsRef<Path>) -> PathBuf {
    root_dir.as_ref().join(SKILLS_REGISTRY_MANIFEST_FILE_NAME)
}

pub fn db_path(root_dir: impl AsRef<Path>) -> PathBuf {
    root_dir.as_ref().join(DB_FILE_NAME)
}

pub fn memory_db_path(root_dir: impl AsRef<Path>) -> PathBuf {
    root_dir.as_ref().join(MEMORY_DB_FILE_NAME)
}

pub fn archive_db_path(root_dir: impl AsRef<Path>) -> PathBuf {
    root_dir.as_ref().join(ARCHIVE_DB_FILE_NAME)
}

pub fn observability_db_path(root_dir: impl AsRef<Path>) -> PathBuf {
    root_dir.as_ref().join(OBSERVABILITY_DB_FILE_NAME)
}

pub fn tmp_dir(root_dir: impl AsRef<Path>) -> PathBuf {
    root_dir.as_ref().join(TMP_DIR_NAME)
}

pub fn sessions_dir(root_dir: impl AsRef<Path>) -> PathBuf {
    root_dir.as_ref().join(SESSIONS_DIR_NAME)
}

pub fn archives_dir(root_dir: impl AsRef<Path>) -> PathBuf {
    root_dir.as_ref().join(ARCHIVES_DIR_NAME)
}

pub fn logs_dir(root_dir: impl AsRef<Path>) -> PathBuf {
    root_dir.as_ref().join(LOGS_DIR_NAME)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvironmentCheckReport {
    pub checks: Vec<DependencyStatus>,
    pub checked_at: time::OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyStatus {
    pub name: String,
    pub description: String,
    pub project_url: Option<String>,
    pub available: bool,
    pub version: Option<String>,
    pub required: bool,
    pub category: DependencyCategory,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DependencyCategory {
    Required,
    Preferred,
    OptionalWithFallback,
}

impl EnvironmentCheckReport {
    pub fn all_required_available(&self) -> bool {
        self.checks
            .iter()
            .filter(|c| c.required)
            .all(|c| c.available)
    }

    pub fn terminal_multiplexer_available(&self) -> bool {
        self.checks
            .iter()
            .filter(|c| c.name == "zellij" || c.name == "tmux")
            .any(|c| c.available)
    }

    pub fn all_preferred_available(&self) -> bool {
        self.checks
            .iter()
            .filter(|c| matches!(c.category, DependencyCategory::Preferred))
            .all(|c| c.available)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn joins_paths_under_root_dir() {
        let root = Path::new("/tmp/demo");

        assert_eq!(config_path(root), PathBuf::from("/tmp/demo/config.toml"));
        assert_eq!(workspace_dir(root), PathBuf::from("/tmp/demo/workspace"));
        assert_eq!(
            skills_registry_manifest_path(root),
            PathBuf::from("/tmp/demo/skills-registry-manifest.json")
        );
        assert_eq!(
            observability_db_path(root),
            PathBuf::from("/tmp/demo/observability.db")
        );
    }

    #[test]
    fn system_timezone_name_is_non_empty() {
        assert!(!system_timezone_name().trim().is_empty());
    }
}
