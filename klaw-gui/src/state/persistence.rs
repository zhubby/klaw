use crate::state::UiState;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const UI_STATE_FILENAME: &str = "gui_state.json";
const UI_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PersistedUiState {
    schema_version: u32,
    state: UiState,
}

impl PersistedUiState {
    fn from_state(state: &UiState) -> Self {
        Self {
            schema_version: UI_STATE_SCHEMA_VERSION,
            state: state.clone(),
        }
    }
}

pub fn load_ui_state() -> UiState {
    let Some(path) = default_state_path() else {
        return UiState::default();
    };
    match load_ui_state_from_path(&path) {
        Ok(state) => state,
        Err(_) => UiState::default(),
    }
}

pub fn save_ui_state(state: &UiState) -> io::Result<()> {
    let Some(path) = default_state_path() else {
        return Ok(());
    };
    save_ui_state_to_path(&path, state)
}

fn load_ui_state_from_path(path: &Path) -> io::Result<UiState> {
    let raw = fs::read_to_string(path)?;
    let persisted: PersistedUiState = serde_json::from_str(&raw)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    if persisted.schema_version != UI_STATE_SCHEMA_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported ui state schema version",
        ));
    }
    Ok(persisted.state)
}

fn save_ui_state_to_path(path: &Path, state: &UiState) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ui state path must have a parent directory",
        ));
    };

    fs::create_dir_all(parent)?;

    let tmp_path = path.with_extension("json.tmp");
    let payload = PersistedUiState::from_state(state);
    let serialized = serde_json::to_string_pretty(&payload)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(&tmp_path, serialized)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

fn default_state_path() -> Option<PathBuf> {
    home_dir().map(|home| home.join(".klaw").join(UI_STATE_FILENAME))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|raw| !raw.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::{load_ui_state_from_path, save_ui_state_to_path};
    use crate::domain::menu::WorkbenchMenu;
    use crate::state::workbench::TabId;
    use crate::state::{ThemeMode, UiAction, UiState};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn roundtrip_ui_state() {
        let path = unique_test_path();
        let mut state = UiState::default();
        state.theme_mode = ThemeMode::Dark;
        state.apply(UiAction::OpenMenu(WorkbenchMenu::Provider));
        state.apply(UiAction::ActivateTab(TabId::from_menu(
            WorkbenchMenu::Provider,
        )));

        save_ui_state_to_path(&path, &state).expect("save ui state");
        let restored = load_ui_state_from_path(&path).expect("load ui state");

        assert_eq!(restored.theme_mode, ThemeMode::Dark);
        assert_eq!(restored.workbench.active_tab, state.workbench.active_tab);
        assert_eq!(restored.workbench.tabs.len(), state.workbench.tabs.len());

        let _ = fs::remove_file(path);
    }

    fn unique_test_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time must advance")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("klaw-gui-persist-{nanos}"))
            .join("gui_state.json")
    }
}
