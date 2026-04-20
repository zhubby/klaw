use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceEntry {
    pub input_rate: f64,
    pub output_rate: f64,
}

pub type PriceTable = BTreeMap<String, BTreeMap<String, PriceEntry>>;

pub fn default_price_table() -> PriceTable {
    let mut openai = BTreeMap::new();
    openai.insert(
        "gpt-4.1".to_string(),
        PriceEntry {
            input_rate: 2.0,
            output_rate: 8.0,
        },
    );
    openai.insert(
        "gpt-4.1-mini".to_string(),
        PriceEntry {
            input_rate: 0.4,
            output_rate: 1.6,
        },
    );
    openai.insert(
        "gpt-4o".to_string(),
        PriceEntry {
            input_rate: 2.5,
            output_rate: 10.0,
        },
    );
    openai.insert(
        "gpt-4o-mini".to_string(),
        PriceEntry {
            input_rate: 0.15,
            output_rate: 0.6,
        },
    );
    let mut anthropic = BTreeMap::new();
    anthropic.insert(
        "claude-3-7-sonnet".to_string(),
        PriceEntry {
            input_rate: 3.0,
            output_rate: 15.0,
        },
    );
    anthropic.insert(
        "claude-sonnet-4".to_string(),
        PriceEntry {
            input_rate: 3.0,
            output_rate: 15.0,
        },
    );
    anthropic.insert(
        "claude-opus-4".to_string(),
        PriceEntry {
            input_rate: 15.0,
            output_rate: 75.0,
        },
    );
    let mut table = BTreeMap::new();
    table.insert("openai".to_string(), openai);
    table.insert("anthropic".to_string(), anthropic);
    table
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_service_name")]
    pub service_name: String,
    #[serde(default = "default_service_version")]
    pub service_version: String,
    #[serde(default)]
    pub metrics: MetricsConfig,
    #[serde(default)]
    pub traces: TracesConfig,
    #[serde(default)]
    pub otlp: OtlpConfig,
    #[serde(default)]
    pub prometheus: PrometheusConfig,
    #[serde(default)]
    pub audit: AuditConfig,
    #[serde(default)]
    pub local_store: LocalStoreConfig,
    #[serde(default = "default_price_table")]
    pub price: PriceTable,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            service_name: default_service_name(),
            service_version: default_service_version(),
            metrics: MetricsConfig::default(),
            traces: TracesConfig::default(),
            otlp: OtlpConfig::default(),
            prometheus: PrometheusConfig::default(),
            audit: AuditConfig::default(),
            local_store: LocalStoreConfig::default(),
            price: default_price_table(),
        }
    }
}

fn default_enabled() -> bool {
    false
}

fn default_service_name() -> String {
    "klaw".to_string()
}

fn default_service_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    #[serde(default = "default_metrics_enabled")]
    pub enabled: bool,
    #[serde(default = "default_export_interval_seconds")]
    pub export_interval_seconds: u64,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: default_metrics_enabled(),
            export_interval_seconds: default_export_interval_seconds(),
        }
    }
}

fn default_metrics_enabled() -> bool {
    true
}

fn default_export_interval_seconds() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracesConfig {
    #[serde(default = "default_traces_enabled")]
    pub enabled: bool,
    #[serde(default = "default_sample_rate")]
    pub sample_rate: f64,
}

impl Default for TracesConfig {
    fn default() -> Self {
        Self {
            enabled: default_traces_enabled(),
            sample_rate: default_sample_rate(),
        }
    }
}

fn default_traces_enabled() -> bool {
    true
}

fn default_sample_rate() -> f64 {
    0.1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpConfig {
    #[serde(default = "default_otlp_enabled")]
    pub enabled: bool,
    #[serde(default = "default_otlp_endpoint")]
    pub endpoint: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

impl Default for OtlpConfig {
    fn default() -> Self {
        Self {
            enabled: default_otlp_enabled(),
            endpoint: default_otlp_endpoint(),
            headers: BTreeMap::new(),
        }
    }
}

fn default_otlp_enabled() -> bool {
    false
}

fn default_otlp_endpoint() -> String {
    "http://localhost:4317".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrometheusConfig {
    #[serde(default = "default_prometheus_enabled")]
    pub enabled: bool,
    #[serde(default = "default_prometheus_listen_port")]
    pub listen_port: u16,
    #[serde(default = "default_prometheus_path")]
    pub path: String,
}

impl Default for PrometheusConfig {
    fn default() -> Self {
        Self {
            enabled: default_prometheus_enabled(),
            listen_port: default_prometheus_listen_port(),
            path: default_prometheus_path(),
        }
    }
}

fn default_prometheus_enabled() -> bool {
    false
}

fn default_prometheus_listen_port() -> u16 {
    9090
}

fn default_prometheus_path() -> String {
    "/metrics".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    #[serde(default = "default_audit_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub output_path: Option<String>,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: default_audit_enabled(),
            output_path: None,
        }
    }
}

fn default_audit_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalStoreConfig {
    #[serde(default = "default_local_store_enabled")]
    pub enabled: bool,
    #[serde(default = "default_local_store_retention_days")]
    pub retention_days: u16,
    #[serde(default = "default_local_store_flush_interval_seconds")]
    pub flush_interval_seconds: u64,
}

impl Default for LocalStoreConfig {
    fn default() -> Self {
        Self {
            enabled: default_local_store_enabled(),
            retention_days: default_local_store_retention_days(),
            flush_interval_seconds: default_local_store_flush_interval_seconds(),
        }
    }
}

fn default_local_store_enabled() -> bool {
    true
}

fn default_local_store_retention_days() -> u16 {
    7
}

fn default_local_store_flush_interval_seconds() -> u64 {
    5
}
