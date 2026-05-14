use crate::state::UiState;
use klaw_util::{default_data_dir, gui_state_path};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const UI_STATE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PersistedUiState {
    schema_version: u32,
    state: UiState,
}

impl PersistedUiState {
    fn from_state(state: &UiState) -> Self {
        let mut state = state.clone();
        state.workbench.sanitize_for_persistence();
        Self {
            schema_version: UI_STATE_SCHEMA_VERSION,
            state,
        }
    }
}

pub fn load_ui_state() -> UiState {
    let Some(path) = default_state_path() else {
        return UiState::default();
    };
    load_ui_state_from_path(&path).unwrap_or_default()
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
    let Ok(persisted) = serde_json::from_str::<PersistedUiState>(&raw) else {
        return Ok(UiState::default());
    };
    if persisted.schema_version != UI_STATE_SCHEMA_VERSION {
        return Ok(UiState::default());
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
    default_data_dir().map(gui_state_path)
}

#[cfg(test)]
mod tests {
    use super::{load_ui_state_from_path, save_ui_state_to_path};
    use crate::domain::menu::WorkbenchMenu;
    use crate::state::workbench::TabId;
    use crate::state::{
        DarkThemePreset, LightThemePreset, LogsLevelFilterState, ThemeMode, UiAction, UiState,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn roundtrip_ui_state() {
        let path = unique_test_path();
        let mut state = UiState {
            theme_mode: ThemeMode::Dark,
            light_theme: LightThemePreset::Crab,
            dark_theme: DarkThemePreset::Mocha,
            ..Default::default()
        };
        state.apply(UiAction::OpenMenu(WorkbenchMenu::Provider));
        state.apply(UiAction::ActivateTab(TabId::from_menu(
            WorkbenchMenu::Provider,
        )));

        save_ui_state_to_path(&path, &state).expect("save ui state");
        let restored = load_ui_state_from_path(&path).expect("load ui state");

        assert_eq!(restored.theme_mode, ThemeMode::Dark);
        assert_eq!(restored.light_theme, LightThemePreset::Crab);
        assert_eq!(restored.dark_theme, DarkThemePreset::Mocha);
        assert_eq!(restored.workbench.active_tab, state.workbench.active_tab);
        assert_eq!(restored.workbench.tab_count(), state.workbench.tab_count());
        assert_eq!(
            restored.logs_panel.level_filter,
            LogsLevelFilterState::default()
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_ui_state_falls_back_for_legacy_state_schema() {
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

        assert_eq!(restored.theme_mode, ThemeMode::System);
        assert_eq!(restored.light_theme, LightThemePreset::Default);
        assert_eq!(restored.dark_theme, DarkThemePreset::Default);
        assert_eq!(
            restored.workbench.active_tab,
            Some(TabId::from_menu(WorkbenchMenu::Profile))
        );
        assert_eq!(restored.workbench.tab_count(), 1);
        assert_eq!(
            restored.logs_panel.level_filter,
            LogsLevelFilterState::default()
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_ui_state_falls_back_for_legacy_state_shape_even_with_current_schema() {
        let path = unique_test_path();
        let json = r#"{
          "schema_version": 2,
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

        assert_eq!(restored.theme_mode, ThemeMode::System);
        assert_eq!(
            restored.workbench.active_tab,
            Some(TabId::from_menu(WorkbenchMenu::Profile))
        );
        assert_eq!(restored.workbench.tab_count(), 1);

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
