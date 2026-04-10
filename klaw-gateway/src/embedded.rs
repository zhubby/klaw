use axum::{
    body::Body,
    http::{HeaderValue, Response, StatusCode, header},
};
use rust_embed::{EmbeddedFile, RustEmbed};

#[derive(RustEmbed)]
#[folder = "static/"]
struct GatewayStatic;

#[derive(RustEmbed)]
#[folder = "assets/"]
struct GatewayAssets;

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
        GatewayAssets::get("logo.webp"),
        "image/webp",
        Some("public, max-age=86400"),
    )
}
