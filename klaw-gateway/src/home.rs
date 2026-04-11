use crate::embedded::{favicon_response, image_response, logo_response, static_html_response};
use axum::{
    body::Body,
    extract::Path,
    http::Response,
};

pub async fn home_page_handler() -> Response<Body> {
    static_html_response("index.html")
}

pub async fn home_logo_handler() -> Response<Body> {
    logo_response()
}

pub async fn home_favicon_handler() -> Response<Body> {
    favicon_response()
}

pub async fn image_handler(Path(filename): Path<String>) -> Response<Body> {
    image_response(&filename)
}
