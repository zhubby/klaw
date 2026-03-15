mod app;
mod domain;
mod panels;
mod state;
mod theme;
mod ui;
mod widgets;

pub use domain::menu::WorkbenchMenu;
pub use panels::{PanelRenderer, RenderCtx};
pub use state::workbench::{TabId, WorkbenchState, WorkbenchTab};
pub use state::UiAction;

pub fn run() -> anyhow::Result<()> {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Klaw Workbench",
        native_options,
        Box::new(|creation_ctx| Ok(Box::new(app::KlawGuiApp::new(creation_ctx)))),
    )
    .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    Ok(())
}
