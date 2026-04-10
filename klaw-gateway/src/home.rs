use crate::embedded::{logo_response, static_html_response};
use axum::{body::Body, http::Response};

pub async fn home_page_handler() -> Response<Body> {
    static_html_response("index.html")
}

pub async fn home_logo_handler() -> Response<Body> {
    logo_response()
}
