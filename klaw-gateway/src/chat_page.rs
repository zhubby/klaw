use crate::embedded::{static_asset_response, static_html_response};
use axum::{body::Body, http::Response};

pub async fn chat_page_handler() -> Response<Body> {
    static_html_response("chat/index.html")
}

pub async fn chat_dist_js_handler() -> Response<Body> {
    static_asset_response(
        "chat/dist/klaw_webui.js",
        "application/javascript; charset=utf-8",
        "public, max-age=3600",
    )
}

pub async fn chat_dist_wasm_handler() -> Response<Body> {
    static_asset_response(
        "chat/dist/klaw_webui_bg.wasm",
        "application/wasm",
        "public, max-age=3600",
    )
}
