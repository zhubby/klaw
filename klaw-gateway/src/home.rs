use axum::{
    body::Body,
    http::{HeaderValue, Response, StatusCode, header},
    response::Html,
};

const HOME_PAGE: &str = include_str!("../static/index.html");
const HOME_LOGO: &[u8] = include_bytes!("../assets/logo.webp");

pub async fn home_page_handler() -> Html<&'static str> {
    Html(HOME_PAGE)
}

pub async fn home_logo_handler() -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("image/webp"),
        )
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=86400"),
        )
        .body(Body::from(HOME_LOGO))
        .unwrap_or_else(|_| Response::new(Body::from(Vec::from(HOME_LOGO))))
}
