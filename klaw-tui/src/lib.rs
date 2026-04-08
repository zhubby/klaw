mod app;
mod event;
mod state;
mod view;

pub use app::run_tui;
pub use state::{AppMessage, AppState, MessageRole, TuiMeta};
