use thiserror::Error;

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("invalid listen address '{0}:{1}': {2}")]
    InvalidListenAddress(String, u16, std::net::AddrParseError),
    #[error("TLS listener is not implemented yet; set gateway.tls.enabled=false")]
    TlsNotImplemented,
    #[error("failed to bind gateway listener: {0}")]
    Bind(#[source] std::io::Error),
    #[error("gateway server failed: {0}")]
    Serve(#[source] std::io::Error),
    #[error("gateway server task failed: {0}")]
    Join(String),
    #[error("failed to create prometheus exporter: {0}")]
    PrometheusExporter(String),
    #[error("gateway webhook token could not be resolved")]
    MissingWebhookToken,
    #[error("gateway webhook handler is required when gateway.webhook.enabled=true")]
    MissingWebhookHandler,
}
