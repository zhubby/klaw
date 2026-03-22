use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};

#[derive(Clone)]
pub struct GatewayAuth {
    token: String,
}

impl GatewayAuth {
    pub fn new(token: String) -> Self {
        Self { token }
    }

    pub async fn middleware(
        State(auth): State<Self>,
        request: Request<Body>,
        next: Next,
    ) -> Result<Response, StatusCode> {
        let path = request.uri().path();

        if should_skip_auth(path) {
            return Ok(next.run(request).await);
        }

        let auth_header = request
            .headers()
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok());

        match auth_header {
            Some(header) if header.starts_with("Bearer ") => {
                let token = &header[7..];
                if token == auth.token {
                    Ok(next.run(request).await)
                } else {
                    Err(StatusCode::UNAUTHORIZED)
                }
            }
            _ => Err(StatusCode::UNAUTHORIZED),
        }
    }
}

fn should_skip_auth(path: &str) -> bool {
    path.starts_with("/health/")
}
