use crate::state::UiState;
use klaw_util::{default_data_dir, gui_state_path};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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

pub fn update_ui_state<F>(mutate: F) -> io::Result<UiState>
where
    F: FnOnce(&mut UiState),
{
    let Some(path) = default_state_path() else {
        let mut state = UiState::default();
        mutate(&mut state);
        return Ok(state);
    };

    let mut state = load_ui_state_from_path(&path).unwrap_or_default();
    mutate(&mut state);
    save_ui_state_to_path(&path, &state)?;
    Ok(state)
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
    let mut state = persisted.state;
    state.workbench.normalize_titles();
    Ok(state)
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
    default_data_dir().map(gui_state_path)
}

#[cfg(test)]
mod tests {
    use super::{load_ui_state_from_path, save_ui_state_to_path};
    use crate::domain::menu::WorkbenchMenu;
    use crate::state::workbench::TabId;
    use crate::state::{DarkThemePreset, LightThemePreset, ThemeMode, UiAction, UiState};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn roundtrip_ui_state() {
        let path = unique_test_path();
        let mut state = UiState::default();
        state.theme_mode = ThemeMode::Dark;
        state.light_theme = LightThemePreset::Latte;
        state.dark_theme = DarkThemePreset::Mocha;
        state.apply(UiAction::OpenMenu(WorkbenchMenu::Provider));
        state.apply(UiAction::ActivateTab(TabId::from_menu(
            WorkbenchMenu::Provider,
        )));

        save_ui_state_to_path(&path, &state).expect("save ui state");
        let restored = load_ui_state_from_path(&path).expect("load ui state");

        assert_eq!(restored.theme_mode, ThemeMode::Dark);
        assert_eq!(restored.light_theme, LightThemePreset::Latte);
        assert_eq!(restored.dark_theme, DarkThemePreset::Mocha);
        assert_eq!(restored.workbench.active_tab, state.workbench.active_tab);
        assert_eq!(restored.workbench.tabs.len(), state.workbench.tabs.len());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_ui_state_normalizes_persisted_tab_titles() {
        let path = unique_test_path();
        let mut state = UiState::default();
        state.workbench.tabs[0].title = "Profile".to_string();

        save_ui_state_to_path(&path, &state).expect("save ui state");
        let restored = load_ui_state_from_path(&path).expect("load ui state");

        assert_eq!(restored.workbench.tabs[0].title, "Profile Prompt");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_ui_state_backfills_missing_theme_presets() {
        let path = unique_test_path();
        let json = r#"{
          "schema_version": 1,
          "state": {
            "workbench": {
              "tabs": [
                {
                  "id": { "menu": "Profile" },
                  "menu": "Profile",
                  "title": "Profile",
                  "closable": true
                }
              ],
              "active_tab": { "menu": "Profile" }
            },
            "theme_mode": "dark",
            "fullscreen": false,
            "show_about": false
          }
        }"#;
        fs::create_dir_all(path.parent().expect("legacy ui state parent"))
            .expect("create legacy ui state parent");
        fs::write(&path, json).expect("write legacy ui state");

        let restored = load_ui_state_from_path(&path).expect("load ui state");

        assert_eq!(restored.theme_mode, ThemeMode::Dark);
        assert_eq!(restored.light_theme, LightThemePreset::Default);
        assert_eq!(restored.dark_theme, DarkThemePreset::Default);

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
