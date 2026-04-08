use axum::{
    body::Body,
    http::{HeaderValue, Response, StatusCode, header},
    response::Html,
};

const CHAT_INDEX_HTML: &str = include_str!("../static/chat/index.html");
const CHAT_PKG_JS: &[u8] = include_bytes!("../static/chat/pkg/klaw_webui.js");
const CHAT_PKG_WASM: &[u8] = include_bytes!("../static/chat/pkg/klaw_webui_bg.wasm");

pub async fn chat_page_handler() -> Html<&'static str> {
    Html(CHAT_INDEX_HTML)
}

pub async fn chat_pkg_js_handler() -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/javascript; charset=utf-8"),
        )
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=3600"),
        )
        .body(Body::from(CHAT_PKG_JS))
        .unwrap_or_else(|_| Response::new(Body::from(Vec::from(CHAT_PKG_JS))))
}

pub async fn chat_pkg_wasm_handler() -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/wasm"),
        )
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=3600"),
        )
        .body(Body::from(CHAT_PKG_WASM))
        .unwrap_or_else(|_| Response::new(Body::from(Vec::from(CHAT_PKG_WASM))))
}
