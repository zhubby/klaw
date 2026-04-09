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

fn embedded_asset(bytes: &'static [u8], content_type: &'static str) -> Response<Body> {
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    response
}

pub async fn chat_pkg_js_handler() -> Response<Body> {
    embedded_asset(CHAT_PKG_JS, "application/javascript; charset=utf-8")
}

pub async fn chat_pkg_wasm_handler() -> Response<Body> {
    embedded_asset(CHAT_PKG_WASM, "application/wasm")
}
