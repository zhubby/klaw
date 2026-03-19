use axum::{
    body::Body,
    http::{header, Response, StatusCode},
};
use opentelemetry::global;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use prometheus::{Encoder, Registry, TextEncoder};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PrometheusExporterError {
    #[error("failed to create prometheus registry: {0}")]
    Registry(String),
    #[error("failed to encode metrics: {0}")]
    Encode(String),
    #[error("failed to build exporter: {0}")]
    Build(String),
}

pub struct PrometheusExporter {
    registry: Registry,
    meter_provider: Option<SdkMeterProvider>,
}

impl Default for PrometheusExporter {
    fn default() -> Self {
        Self::new().expect("failed to create default prometheus exporter")
    }
}

impl PrometheusExporter {
    pub fn new() -> Result<Self, PrometheusExporterError> {
        let registry = Registry::new();
        Ok(Self {
            registry,
            meter_provider: None,
        })
    }

    pub fn with_registry(registry: Registry) -> Self {
        Self {
            registry,
            meter_provider: None,
        }
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn registry_arc(&self) -> Arc<Registry> {
        Arc::new(self.registry.clone())
    }

    pub fn install(&mut self, service_name: &str) -> Result<(), PrometheusExporterError> {
        let exporter = opentelemetry_prometheus::exporter()
            .with_registry(self.registry.clone())
            .build()
            .map_err(|e| PrometheusExporterError::Build(e.to_string()))?;

        let meter_provider = SdkMeterProvider::builder().with_reader(exporter).build();

        global::set_meter_provider(meter_provider.clone());
        self.meter_provider = Some(meter_provider);

        let _ = service_name;
        Ok(())
    }

    pub fn render_metrics(&self) -> Result<String, PrometheusExporterError> {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .map_err(|e| PrometheusExporterError::Encode(e.to_string()))?;
        String::from_utf8(buffer).map_err(|e| PrometheusExporterError::Encode(e.to_string()))
    }

    pub fn metrics_response(&self) -> Response<Body> {
        match self.render_metrics() {
            Ok(body) => Response::builder()
                .status(StatusCode::OK)
                .header(
                    header::CONTENT_TYPE,
                    "text/plain; version=0.0.4; charset=utf-8",
                )
                .body(Body::from(body))
                .unwrap_or_else(|_| {
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("failed to build response"))
                        .unwrap()
                }),
            Err(err) => {
                tracing::warn!(error = %err, "failed to render prometheus metrics");
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from(format!("failed to render metrics: {}", err)))
                    .unwrap()
            }
        }
    }

    pub fn shutdown(&self) {
        if let Some(provider) = &self.meter_provider {
            let _ = provider.shutdown();
        }
    }
}

impl Drop for PrometheusExporter {
    fn drop(&mut self) {
        self.shutdown();
    }
}
