use opentelemetry::global;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    metrics::{PeriodicReader, SdkMeterProvider},
    propagation::TraceContextPropagator,
    runtime::Tokio,
    trace::{RandomIdGenerator, Sampler, TracerProvider},
};
use std::collections::BTreeMap;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OtlpExporterError {
    #[error("failed to create OTLP exporter: {0}")]
    Create(String),
    #[error("failed to set global tracer provider: {0}")]
    SetGlobalTracer(String),
    #[error("failed to set global meter provider: {0}")]
    SetGlobalMeter(String),
}

pub struct OtlpExporter {
    tracer_provider: Option<TracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
}

impl OtlpExporter {
    pub fn new(
        endpoint: &str,
        _headers: &BTreeMap<String, String>,
        sample_rate: f64,
        export_interval_seconds: u64,
    ) -> Result<Self, OtlpExporterError> {
        let mut exporter = Self {
            tracer_provider: None,
            meter_provider: None,
        };

        let tracer_provider = Self::build_tracer_provider(endpoint, sample_rate)?;
        global::set_tracer_provider(tracer_provider.clone());
        exporter.tracer_provider = Some(tracer_provider);

        let meter_provider = Self::build_meter_provider(endpoint, export_interval_seconds)?;
        global::set_meter_provider(meter_provider.clone());
        exporter.meter_provider = Some(meter_provider);

        global::set_text_map_propagator(TraceContextPropagator::new());

        Ok(exporter)
    }

    fn build_tracer_provider(
        endpoint: &str,
        sample_rate: f64,
    ) -> Result<TracerProvider, OtlpExporterError> {
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()
            .map_err(|e| OtlpExporterError::Create(e.to_string()))?;

        let tracer_provider = TracerProvider::builder()
            .with_sampler(Sampler::TraceIdRatioBased(sample_rate))
            .with_id_generator(RandomIdGenerator::default())
            .with_batch_exporter(exporter, Tokio)
            .build();

        Ok(tracer_provider)
    }

    fn build_meter_provider(
        endpoint: &str,
        export_interval_seconds: u64,
    ) -> Result<SdkMeterProvider, OtlpExporterError> {
        let exporter = opentelemetry_otlp::MetricExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .with_temporality(opentelemetry_sdk::metrics::Temporality::Cumulative)
            .build()
            .map_err(|e| OtlpExporterError::Create(e.to_string()))?;

        let reader = PeriodicReader::builder(exporter, Tokio)
            .with_interval(Duration::from_secs(export_interval_seconds))
            .build();

        let meter_provider = SdkMeterProvider::builder().with_reader(reader).build();

        Ok(meter_provider)
    }

    pub fn tracer_provider(&self) -> Option<&TracerProvider> {
        self.tracer_provider.as_ref()
    }

    pub fn meter_provider(&self) -> Option<&SdkMeterProvider> {
        self.meter_provider.as_ref()
    }

    pub fn shutdown(&self) {
        if let Some(provider) = &self.tracer_provider {
            let _ = provider.shutdown();
        }
        if let Some(provider) = &self.meter_provider {
            let _ = provider.shutdown();
        }
    }
}

impl Drop for OtlpExporter {
    fn drop(&mut self) {
        self.shutdown();
    }
}
