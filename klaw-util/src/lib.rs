use serde::{Deserialize, Serialize};
use std::{
    env,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

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
#[cfg(any(target_os = "macos", test))]
const MACOS_STANDARD_BINARY_PATHS: &[&str] = &[
    "/opt/homebrew/bin",
    "/opt/homebrew/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/opt/local/bin",
    "/opt/local/sbin",
];

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPathUpdate {
    pub joined_path: OsString,
    pub added_paths: Vec<PathBuf>,
}

pub fn command_search_path() -> Option<OsString> {
    command_path_update().map(|update| update.joined_path)
}

pub fn augment_current_process_command_path() -> Option<CommandPathUpdate> {
    let update = command_path_update()?;
    // SAFETY: Callers must only invoke this during single-threaded startup before other
    // tasks or threads begin concurrently reading or mutating process environment variables.
    unsafe {
        env::set_var("PATH", &update.joined_path);
    }
    Some(update)
}

fn command_path_update() -> Option<CommandPathUpdate> {
    #[cfg(target_os = "macos")]
    {
        let installed_candidates = MACOS_STANDARD_BINARY_PATHS
            .iter()
            .map(PathBuf::from)
            .filter(|candidate| candidate.exists())
            .collect::<Vec<_>>();
        return compute_command_path_update(env::var_os("PATH"), installed_candidates);
    }

    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(any(target_os = "macos", test))]
fn compute_command_path_update(
    current_path: Option<OsString>,
    installed_candidates: Vec<PathBuf>,
) -> Option<CommandPathUpdate> {
    let (merged_paths, added_paths) =
        compute_augmented_path_entries(current_path.as_deref(), installed_candidates);
    if added_paths.is_empty() {
        return None;
    }
    let joined_path = env::join_paths(&merged_paths).ok()?;
    Some(CommandPathUpdate {
        joined_path,
        added_paths,
    })
}

#[cfg(any(target_os = "macos", test))]
fn compute_augmented_path_entries(
    current_path: Option<&OsStr>,
    installed_candidates: Vec<PathBuf>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut merged_paths: Vec<PathBuf> = current_path
        .map(env::split_paths)
        .into_iter()
        .flatten()
        .collect();
    let mut added_paths = Vec::new();

    for candidate in installed_candidates.into_iter().rev() {
        if merged_paths.iter().any(|existing| existing == &candidate) {
            continue;
        }
        merged_paths.insert(0, candidate.clone());
        added_paths.push(candidate);
    }

    added_paths.reverse();
    (merged_paths, added_paths)
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

    #[test]
    fn compute_augmented_path_entries_prepends_missing_candidates_once() {
        let existing = env::join_paths([
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
            PathBuf::from("/usr/local/bin"),
        ])
        .expect("join test PATH");
        let candidates = vec![
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/opt/local/bin"),
        ];

        let (merged_paths, added_paths) =
            compute_augmented_path_entries(Some(existing.as_os_str()), candidates);

        assert_eq!(
            merged_paths,
            vec![
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/opt/local/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
                PathBuf::from("/usr/local/bin"),
            ]
        );
        assert_eq!(
            added_paths,
            vec![
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/opt/local/bin"),
            ]
        );
    }

    #[test]
    fn compute_augmented_path_entries_handles_empty_path() {
        let candidates = vec![PathBuf::from("/opt/homebrew/bin")];

        let (merged_paths, added_paths) = compute_augmented_path_entries(None, candidates);

        assert_eq!(merged_paths, vec![PathBuf::from("/opt/homebrew/bin")]);
        assert_eq!(added_paths, vec![PathBuf::from("/opt/homebrew/bin")]);
    }

    #[test]
    fn compute_command_path_update_returns_none_when_no_paths_added() {
        let existing = env::join_paths([PathBuf::from("/usr/local/bin")]).expect("join test PATH");
        let candidates = vec![PathBuf::from("/usr/local/bin")];

        let update = compute_command_path_update(Some(existing), candidates);

        assert!(update.is_none());
    }
}
