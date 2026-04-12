use axum::{
    body::Body,
    http::{HeaderValue, Response, StatusCode, header},
};
use rust_embed::{EmbeddedFile, RustEmbed};

#[derive(RustEmbed)]
#[folder = "static/"]
struct GatewayStatic;

fn embedded_response(
    file: Option<EmbeddedFile>,
    content_type: &'static str,
    cache_control: Option<&'static str>,
) -> Response<Body> {
    let Some(file) = file else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap_or_else(|_| Response::new(Body::empty()));
    };

    let mut response = Response::new(Body::from(file.data.into_owned()));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Some(cache_control) = cache_control {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(cache_control),
        );
    }
    response
}

pub(crate) fn static_html_response(path: &str) -> Response<Body> {
    embedded_response(GatewayStatic::get(path), "text/html; charset=utf-8", None)
}

pub(crate) fn static_asset_response(
    path: &str,
    content_type: &'static str,
    cache_control: &'static str,
) -> Response<Body> {
    embedded_response(GatewayStatic::get(path), content_type, Some(cache_control))
}

pub(crate) fn logo_response() -> Response<Body> {
    embedded_response(
        GatewayStatic::get("logo.webp"),
        "image/webp",
        Some("public, max-age=86400"),
    )
}

pub(crate) fn favicon_response() -> Response<Body> {
    embedded_response(
        GatewayStatic::get("favicon.ico"),
        "image/x-icon",
        Some("public, max-age=86400"),
    )
}

pub(crate) fn image_response(filename: &str) -> Response<Body> {
    let path = format!("images/{}", filename);
    let content_type = match filename.rsplit('.').next() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    };
    embedded_response(
        GatewayStatic::get(&path),
        content_type,
        Some("public, max-age=86400"),
    )
}
