use std::{
    env,
    ffi::{OsStr, OsString},
    path::PathBuf,
};

#[cfg(any(target_os = "macos", test))]
const MACOS_STANDARD_BINARY_PATHS: &[&str] = &[
    "/opt/homebrew/bin",
    "/opt/homebrew/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/opt/local/bin",
    "/opt/local/sbin",
];

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

#[cfg(test)]
mod tests {
    use super::*;

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
