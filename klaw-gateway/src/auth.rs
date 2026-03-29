use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode, header},
    middleware::Next,
    response::Response,
};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::Sha256;

type HmacSha1 = Hmac<Sha1>;
type HmacSha256 = Hmac<Sha256>;

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
            .get(header::AUTHORIZATION)
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

pub fn should_skip_auth(path: &str) -> bool {
    path.starts_with("/health/") || path.starts_with("/webhook/")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookAuthResult {
    pub mode: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookAuthError {
    message: String,
}

impl WebhookAuthError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ValidatorOutcome {
    Matched,
    NotMatched,
}

trait WebhookRequestValidator: Send + Sync {
    fn mode(&self) -> &'static str;

    fn validate(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        secret: &str,
    ) -> Result<ValidatorOutcome, WebhookAuthError>;
}

pub struct WebhookAuth {
    enabled: bool,
    secret: Option<String>,
    validators: Vec<Box<dyn WebhookRequestValidator>>,
}

impl WebhookAuth {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            secret: None,
            validators: default_webhook_validators(),
        }
    }

    pub fn enabled(secret: Option<String>) -> Self {
        Self {
            enabled: true,
            secret,
            validators: default_webhook_validators(),
        }
    }

    pub fn validate(
        &self,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Result<WebhookAuthResult, WebhookAuthError> {
        if !self.enabled {
            return Ok(WebhookAuthResult { mode: "disabled" });
        }

        let secret = self.secret.as_deref().ok_or_else(|| {
            WebhookAuthError::new("webhook auth is enabled but no gateway auth token is configured")
        })?;

        for validator in &self.validators {
            match validator.validate(headers, body, secret)? {
                ValidatorOutcome::Matched => {
                    return Ok(WebhookAuthResult {
                        mode: validator.mode(),
                    });
                }
                ValidatorOutcome::NotMatched => {}
            }
        }

        Err(WebhookAuthError::new(
            "missing or invalid webhook authentication header",
        ))
    }
}

fn default_webhook_validators() -> Vec<Box<dyn WebhookRequestValidator>> {
    vec![
        Box::new(BearerTokenValidator),
        Box::new(GitHubSha256Validator),
        Box::new(GitHubSha1Validator),
        Box::new(GitLabTokenValidator),
        Box::new(GitLabSignatureValidator),
    ]
}

struct BearerTokenValidator;

impl WebhookRequestValidator for BearerTokenValidator {
    fn mode(&self) -> &'static str {
        "bearer"
    }

    fn validate(
        &self,
        headers: &HeaderMap,
        _body: &[u8],
        secret: &str,
    ) -> Result<ValidatorOutcome, WebhookAuthError> {
        let Some(value) = header_str(headers, header::AUTHORIZATION) else {
            return Ok(ValidatorOutcome::NotMatched);
        };
        let Some(token) = value.strip_prefix("Bearer ") else {
            return Err(WebhookAuthError::new(
                "invalid Authorization header; expected Bearer token",
            ));
        };
        if token == secret {
            Ok(ValidatorOutcome::Matched)
        } else {
            Err(WebhookAuthError::new("invalid bearer token"))
        }
    }
}

struct GitHubSha256Validator;

impl WebhookRequestValidator for GitHubSha256Validator {
    fn mode(&self) -> &'static str {
        "github_sha256"
    }

    fn validate(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        secret: &str,
    ) -> Result<ValidatorOutcome, WebhookAuthError> {
        let Some(value) = header_str(headers, "x-hub-signature-256") else {
            return Ok(ValidatorOutcome::NotMatched);
        };
        verify_prefixed_hmac::<HmacSha256>(value, "sha256=", secret, body)
            .map(|()| ValidatorOutcome::Matched)
    }
}

struct GitHubSha1Validator;

impl WebhookRequestValidator for GitHubSha1Validator {
    fn mode(&self) -> &'static str {
        "github_sha1"
    }

    fn validate(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        secret: &str,
    ) -> Result<ValidatorOutcome, WebhookAuthError> {
        let Some(value) = header_str(headers, "x-hub-signature") else {
            return Ok(ValidatorOutcome::NotMatched);
        };
        verify_prefixed_hmac::<HmacSha1>(value, "sha1=", secret, body)
            .map(|()| ValidatorOutcome::Matched)
    }
}

struct GitLabTokenValidator;

impl WebhookRequestValidator for GitLabTokenValidator {
    fn mode(&self) -> &'static str {
        "gitlab_token"
    }

    fn validate(
        &self,
        headers: &HeaderMap,
        _body: &[u8],
        secret: &str,
    ) -> Result<ValidatorOutcome, WebhookAuthError> {
        let Some(value) = header_str(headers, "x-gitlab-token") else {
            return Ok(ValidatorOutcome::NotMatched);
        };
        if value == secret {
            Ok(ValidatorOutcome::Matched)
        } else {
            Err(WebhookAuthError::new("invalid X-Gitlab-Token header"))
        }
    }
}

struct GitLabSignatureValidator;

impl WebhookRequestValidator for GitLabSignatureValidator {
    fn mode(&self) -> &'static str {
        "gitlab_signature"
    }

    fn validate(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        secret: &str,
    ) -> Result<ValidatorOutcome, WebhookAuthError> {
        let Some(value) = header_str(headers, "x-gitlab-signature") else {
            return Ok(ValidatorOutcome::NotMatched);
        };
        verify_hex_hmac::<HmacSha256>(value, secret, body).map(|()| ValidatorOutcome::Matched)
    }
}

fn header_str<'a>(headers: &'a HeaderMap, name: impl header::AsHeaderName) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn verify_prefixed_hmac<M>(
    value: &str,
    prefix: &str,
    secret: &str,
    body: &[u8],
) -> Result<(), WebhookAuthError>
where
    M: Mac + hmac::digest::KeyInit,
{
    let Some(signature) = value.strip_prefix(prefix) else {
        return Err(WebhookAuthError::new(format!(
            "invalid signature header; expected prefix {prefix}"
        )));
    };
    verify_hex_hmac::<M>(signature, secret, body)
}

fn verify_hex_hmac<M>(signature: &str, secret: &str, body: &[u8]) -> Result<(), WebhookAuthError>
where
    M: Mac + hmac::digest::KeyInit,
{
    let decoded =
        hex::decode(signature).map_err(|_| WebhookAuthError::new("signature is not valid hex"))?;
    let mut mac = <M as hmac::digest::KeyInit>::new_from_slice(secret.as_bytes())
        .map_err(|_| WebhookAuthError::new("invalid webhook secret"))?;
    mac.update(body);
    mac.verify_slice(&decoded)
        .map_err(|_| WebhookAuthError::new("signature verification failed"))
}

#[cfg(test)]
mod tests {
    use super::{GatewayAuth, WebhookAuth, should_skip_auth};
    use axum::http::{HeaderMap, HeaderValue};
    use hmac::{Hmac, Mac};
    use sha1::Sha1;
    use sha2::Sha256;

    type HmacSha1 = Hmac<Sha1>;
    type HmacSha256 = Hmac<Sha256>;

    #[test]
    fn gateway_auth_skips_health_and_webhook_paths() {
        assert!(should_skip_auth("/health/status"));
        assert!(should_skip_auth("/webhook/events"));
        assert!(should_skip_auth("/webhook/agents"));
        assert!(!should_skip_auth("/ws/chat"));
    }

    #[test]
    fn webhook_auth_accepts_requests_when_disabled() {
        let auth = WebhookAuth::disabled();
        let result = auth
            .validate(&HeaderMap::new(), br#"{"hello":"world"}"#)
            .expect("disabled auth should pass");
        assert_eq!(result.mode, "disabled");
    }

    #[test]
    fn webhook_auth_validates_bearer_token() {
        let auth = WebhookAuth::enabled(Some("secret".to_string()));
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer secret"));

        let result = auth
            .validate(&headers, br#"{"hello":"world"}"#)
            .expect("bearer token should validate");
        assert_eq!(result.mode, "bearer");
    }

    #[test]
    fn webhook_auth_rejects_invalid_bearer_token() {
        let auth = WebhookAuth::enabled(Some("secret".to_string()));
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer wrong"));

        let err = auth
            .validate(&headers, br#"{"hello":"world"}"#)
            .expect_err("invalid bearer token should fail");
        assert_eq!(err.message(), "invalid bearer token");
    }

    #[test]
    fn webhook_auth_validates_github_sha256_signature() {
        let auth = WebhookAuth::enabled(Some("secret".to_string()));
        let body = br#"{"hello":"world"}"#;
        let signature = github_signature_sha256("secret", body);
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-hub-signature-256",
            HeaderValue::from_str(&signature).expect("signature header should be valid"),
        );

        let result = auth
            .validate(&headers, body)
            .expect("github sha256 signature should validate");
        assert_eq!(result.mode, "github_sha256");
    }

    #[test]
    fn webhook_auth_rejects_tampered_github_sha256_signature() {
        let auth = WebhookAuth::enabled(Some("secret".to_string()));
        let body = br#"{"hello":"world"}"#;
        let signature = github_signature_sha256("secret", body);
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-hub-signature-256",
            HeaderValue::from_str(&signature).expect("signature header should be valid"),
        );

        let err = auth
            .validate(&headers, br#"{"hello":"tampered"}"#)
            .expect_err("tampered payload should fail");
        assert_eq!(err.message(), "signature verification failed");
    }

    #[test]
    fn webhook_auth_validates_github_sha1_signature() {
        let auth = WebhookAuth::enabled(Some("secret".to_string()));
        let body = br#"{"hello":"world"}"#;
        let signature = github_signature_sha1("secret", body);
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-hub-signature",
            HeaderValue::from_str(&signature).expect("signature header should be valid"),
        );

        let result = auth
            .validate(&headers, body)
            .expect("github sha1 signature should validate");
        assert_eq!(result.mode, "github_sha1");
    }

    #[test]
    fn webhook_auth_validates_gitlab_token() {
        let auth = WebhookAuth::enabled(Some("secret".to_string()));
        let mut headers = HeaderMap::new();
        headers.insert("x-gitlab-token", HeaderValue::from_static("secret"));

        let result = auth
            .validate(&headers, br#"{"hello":"world"}"#)
            .expect("gitlab token should validate");
        assert_eq!(result.mode, "gitlab_token");
    }

    #[test]
    fn webhook_auth_validates_gitlab_signature() {
        let auth = WebhookAuth::enabled(Some("secret".to_string()));
        let body = br#"{"hello":"world"}"#;
        let signature = gitlab_signature("secret", body);
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-gitlab-signature",
            HeaderValue::from_str(&signature).expect("signature header should be valid"),
        );

        let result = auth
            .validate(&headers, body)
            .expect("gitlab signature should validate");
        assert_eq!(result.mode, "gitlab_signature");
    }

    #[test]
    fn webhook_auth_requires_configured_secret_when_enabled() {
        let auth = WebhookAuth::enabled(None);
        let err = auth
            .validate(&HeaderMap::new(), br#"{"hello":"world"}"#)
            .expect_err("enabled auth without secret should fail");
        assert_eq!(
            err.message(),
            "webhook auth is enabled but no gateway auth token is configured"
        );
    }

    fn github_signature_sha256(secret: &str, body: &[u8]) -> String {
        let mut mac =
            HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac key should be valid");
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    fn github_signature_sha1(secret: &str, body: &[u8]) -> String {
        let mut mac =
            HmacSha1::new_from_slice(secret.as_bytes()).expect("hmac key should be valid");
        mac.update(body);
        format!("sha1={}", hex::encode(mac.finalize().into_bytes()))
    }

    fn gitlab_signature(secret: &str, body: &[u8]) -> String {
        let mut mac =
            HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac key should be valid");
        mac.update(body);
        hex::encode(mac.finalize().into_bytes())
    }

    #[test]
    fn gateway_auth_keeps_constructor_for_route_middleware() {
        let auth = GatewayAuth::new("secret".to_string());
        let _ = auth;
    }
}
