//! WASM-only egui chat client for `/ws/chat`.

mod app;
mod protocol;
mod session;
mod storage;
mod transport;
mod ui;

use app::ChatApp;
use wasm_bindgen::prelude::*;

/// Start the chat UI on the given canvas (install from `index.html` via wasm-bindgen).
#[wasm_bindgen]
pub fn start_chat_ui(canvas: web_sys::HtmlCanvasElement) {
    console_error_panic_hook::set_once();
    let web_options = eframe::WebOptions::default();
    let runner = eframe::WebRunner::new();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = runner
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(ChatApp::new(cc)))),
            )
            .await;
    });
}
