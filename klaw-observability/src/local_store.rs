use crate::config::LocalStoreConfig;
use async_trait::async_trait;
use klaw_core::observability::{
    ModelRequestRecord, ModelRequestStatus, ModelToolOutcomeRecord, ToolOutcomeStatus,
    TurnOutcomeRecord,
};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;
use thiserror::Error;
use time::OffsetDateTime;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolMetricEvent {
    pub occurred_at_unix_ms: i64,
    pub session_key: String,
    pub tool_name: String,
    pub status: ToolOutcomeStatus,
    pub error_code: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ToolTimeRange {
    LastHour,
    Last24Hours,
    Last7Days,
}

impl ToolTimeRange {
    pub fn label(self) -> &'static str {
        match self {
            Self::LastHour => "1h",
            Self::Last24Hours => "24h",
            Self::Last7Days => "7d",
        }
    }

    fn window_start_unix_ms(self, now_unix_ms: i64) -> i64 {
        now_unix_ms
            - match self {
                Self::LastHour => 60 * 60 * 1000,
                Self::Last24Hours => 24 * 60 * 60 * 1000,
                Self::Last7Days => 7 * 24 * 60 * 60 * 1000,
            }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ToolSampleBucket {
    OneMinute,
    OneHour,
}

impl ToolSampleBucket {
    pub fn label(self) -> &'static str {
        match self {
            Self::OneMinute => "1m",
            Self::OneHour => "1h",
        }
    }

    fn bucket_width_ms(self) -> i64 {
        match self {
            Self::OneMinute => 60 * 1000,
            Self::OneHour => 60 * 60 * 1000,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolStatsQuery {
    pub time_range: ToolTimeRange,
    pub bucket_width: ToolSampleBucket,
    pub limit: usize,
}

impl Default for ToolStatsQuery {
    fn default() -> Self {
        Self {
            time_range: ToolTimeRange::LastHour,
            bucket_width: ToolSampleBucket::OneMinute,
            limit: 10,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelStatsQuery {
    pub time_range: ToolTimeRange,
    pub bucket_width: ToolSampleBucket,
    pub limit: usize,
    pub provider: Option<String>,
    pub model: Option<String>,
}

impl Default for ModelStatsQuery {
    fn default() -> Self {
        Self {
            time_range: ToolTimeRange::LastHour,
            bucket_width: ToolSampleBucket::OneMinute,
            limit: 10,
            provider: None,
            model: None,
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ToolSummaryRow {
    pub total_calls: u64,
    pub successes: u64,
    pub failures: u64,
    pub success_rate: f64,
    pub avg_duration_ms: f64,
    pub max_duration_ms: u64,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ToolStatsRow {
    pub tool_name: String,
    pub calls: u64,
    pub successes: u64,
    pub failures: u64,
    pub success_rate: f64,
    pub avg_duration_ms: f64,
    pub max_duration_ms: u64,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ToolTimeseriesPoint {
    pub bucket_start_unix_ms: i64,
    pub calls: u64,
    pub successes: u64,
    pub failures: u64,
    pub success_rate: f64,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ToolErrorBreakdownRow {
    pub error_code: String,
    pub failures: u64,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ToolDashboardSnapshot {
    pub generated_at_unix_ms: i64,
    pub summary: ToolSummaryRow,
    pub top_by_calls: Vec<ToolStatsRow>,
    pub top_by_failure_rate: Vec<ToolStatsRow>,
    pub timeseries: Vec<ToolTimeseriesPoint>,
    pub error_breakdown: Vec<ToolErrorBreakdownRow>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ModelSummaryRow {
    pub total_requests: u64,
    pub successes: u64,
    pub failures: u64,
    pub request_success_rate: f64,
    pub request_failure_rate: f64,
    pub timeout_rate: f64,
    pub empty_response_rate: f64,
    pub avg_duration_ms: f64,
    pub max_duration_ms: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tokens: u64,
    pub avg_input_tokens: f64,
    pub avg_output_tokens: f64,
    pub avg_total_tokens: f64,
    pub input_output_ratio: f64,
    pub cached_input_tokens_ratio: f64,
    pub reasoning_tokens_ratio: f64,
    pub tokens_per_second: f64,
    pub tool_call_rate: f64,
    pub avg_tool_calls_per_request: f64,
    pub tool_success_rate_by_model: f64,
    pub approval_required_rate: f64,
    pub avg_requests_per_turn: f64,
    pub avg_tool_iterations_per_turn: f64,
    pub turn_completion_rate: f64,
    pub degraded_run_rate: f64,
    pub token_budget_exceeded_rate: f64,
    pub tool_loop_exhausted_rate: f64,
    pub estimated_cost_usd: Option<f64>,
    pub cost_per_successful_turn: Option<f64>,
    pub cost_per_tool_success: Option<f64>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ModelStatsRow {
    pub provider: String,
    pub model: String,
    pub wire_api: String,
    pub requests: u64,
    pub successes: u64,
    pub failures: u64,
    pub request_success_rate: f64,
    pub request_failure_rate: f64,
    pub timeout_rate: f64,
    pub empty_response_rate: f64,
    pub avg_duration_ms: f64,
    pub p95_duration_ms: f64,
    pub max_duration_ms: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tokens: u64,
    pub avg_input_tokens: f64,
    pub avg_output_tokens: f64,
    pub avg_total_tokens: f64,
    pub input_output_ratio: f64,
    pub cached_input_tokens_ratio: f64,
    pub reasoning_tokens_ratio: f64,
    pub tokens_per_second: f64,
    pub tool_call_rate: f64,
    pub avg_tool_calls_per_request: f64,
    pub tool_success_rate_by_model: f64,
    pub approval_required_rate: f64,
    pub avg_requests_per_turn: f64,
    pub avg_tool_iterations_per_turn: f64,
    pub turn_completion_rate: f64,
    pub degraded_run_rate: f64,
    pub token_budget_exceeded_rate: f64,
    pub tool_loop_exhausted_rate: f64,
    pub estimated_cost_usd: Option<f64>,
    pub cost_per_successful_turn: Option<f64>,
    pub cost_per_tool_success: Option<f64>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ModelLatencyPercentilesRow {
    pub p50_duration_ms: f64,
    pub p95_duration_ms: f64,
    pub p99_duration_ms: f64,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ModelTokenCompositionRow {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
    pub reasoning_tokens: u64,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ModelErrorBreakdownRow {
    pub error_code: String,
    pub failures: u64,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ModelToolBreakdownRow {
    pub tool_name: String,
    pub calls: u64,
    pub successes: u64,
    pub failures: u64,
    pub success_rate: f64,
    pub approval_required_rate: f64,
    pub avg_duration_ms: f64,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TurnEfficiencyRow {
    pub avg_requests_per_turn: f64,
    pub avg_tool_iterations_per_turn: f64,
    pub turn_completion_rate: f64,
    pub degraded_run_rate: f64,
    pub token_budget_exceeded_rate: f64,
    pub tool_loop_exhausted_rate: f64,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ModelTimeseriesPoint {
    pub bucket_start_unix_ms: i64,
    pub requests: u64,
    pub successes: u64,
    pub failures: u64,
    pub request_success_rate: f64,
    pub avg_duration_ms: f64,
    pub p95_duration_ms: f64,
    pub total_tokens: u64,
    pub tool_call_rate: f64,
    pub tool_success_rate: f64,
    pub avg_requests_per_turn: f64,
    pub avg_tool_iterations_per_turn: f64,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ModelDashboardSnapshot {
    pub generated_at_unix_ms: i64,
    pub providers: Vec<String>,
    pub models: Vec<String>,
    pub summary: ModelSummaryRow,
    pub latency_percentiles: ModelLatencyPercentilesRow,
    pub token_composition: ModelTokenCompositionRow,
    pub turn_efficiency: TurnEfficiencyRow,
    pub model_rows: Vec<ModelStatsRow>,
    pub timeseries: Vec<ModelTimeseriesPoint>,
    pub error_breakdown: Vec<ModelErrorBreakdownRow>,
    pub tool_breakdown: Vec<ModelToolBreakdownRow>,
}

#[derive(Debug, Error)]
pub enum LocalMetricsStoreError {
    #[error("failed to create observability data directory: {0}")]
    CreateDir(#[source] std::io::Error),
    #[error("failed to connect local metrics store: {0}")]
    Connect(#[source] sqlx::Error),
    #[error("failed to initialize local metrics store: {0}")]
    Init(#[source] sqlx::Error),
    #[error("local metrics query failed: {0}")]
    Query(#[source] sqlx::Error),
}

#[async_trait]
pub trait LocalMetricsStore: Send + Sync {
    async fn record_tool_outcome(
        &self,
        event: ToolMetricEvent,
    ) -> Result<(), LocalMetricsStoreError>;
    async fn record_model_request(
        &self,
        record: ModelRequestRecord,
    ) -> Result<(), LocalMetricsStoreError>;
    async fn record_model_tool_outcome(
        &self,
        record: ModelToolOutcomeRecord,
    ) -> Result<(), LocalMetricsStoreError>;
    async fn record_turn_outcome(
        &self,
        record: TurnOutcomeRecord,
    ) -> Result<(), LocalMetricsStoreError>;
    async fn query_tool_summary(
        &self,
        query: &ToolStatsQuery,
    ) -> Result<ToolSummaryRow, LocalMetricsStoreError>;
    async fn query_tool_stats(
        &self,
        query: &ToolStatsQuery,
    ) -> Result<Vec<ToolStatsRow>, LocalMetricsStoreError>;
    async fn query_tool_timeseries(
        &self,
        query: &ToolStatsQuery,
    ) -> Result<Vec<ToolTimeseriesPoint>, LocalMetricsStoreError>;
    async fn query_tool_error_breakdown(
        &self,
        query: &ToolStatsQuery,
        tool_name: Option<&str>,
    ) -> Result<Vec<ToolErrorBreakdownRow>, LocalMetricsStoreError>;
    async fn query_tool_dashboard_snapshot(
        &self,
        query: &ToolStatsQuery,
        tool_name: Option<&str>,
    ) -> Result<ToolDashboardSnapshot, LocalMetricsStoreError>;
    async fn query_model_dashboard_snapshot(
        &self,
        query: &ModelStatsQuery,
    ) -> Result<ModelDashboardSnapshot, LocalMetricsStoreError>;
}

pub struct SqliteLocalMetricsStore {
    pool: SqlitePool,
    retention_days: u16,
    maintenance_interval: Duration,
    last_maintenance_unix_ms: Mutex<i64>,
}

impl SqliteLocalMetricsStore {
    pub async fn open(
        path: impl AsRef<Path>,
        config: &LocalStoreConfig,
    ) -> Result<Self, LocalMetricsStoreError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(LocalMetricsStoreError::CreateDir)?;
        }
        let connect_options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(connect_options)
            .await
            .map_err(LocalMetricsStoreError::Connect)?;
        let store = Self {
            pool,
            retention_days: config.retention_days,
            maintenance_interval: Duration::from_secs(config.flush_interval_seconds),
            last_maintenance_unix_ms: Mutex::new(0),
        };
        store.init().await?;
        Ok(store)
    }

    async fn init(&self) -> Result<(), LocalMetricsStoreError> {
        for statement in [
            "CREATE TABLE IF NOT EXISTS tool_metric_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                occurred_at_unix_ms INTEGER NOT NULL,
                bucket_minute_unix_ms INTEGER NOT NULL,
                session_key TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                outcome TEXT NOT NULL,
                error_code TEXT,
                duration_ms INTEGER NOT NULL
            )",
            "CREATE INDEX IF NOT EXISTS idx_tool_metric_events_window
             ON tool_metric_events(occurred_at_unix_ms DESC, tool_name)",
            "CREATE TABLE IF NOT EXISTS tool_metric_minute_rollups (
                bucket_minute_unix_ms INTEGER NOT NULL,
                tool_name TEXT NOT NULL,
                calls INTEGER NOT NULL DEFAULT 0,
                successes INTEGER NOT NULL DEFAULT 0,
                failures INTEGER NOT NULL DEFAULT 0,
                total_duration_ms INTEGER NOT NULL DEFAULT 0,
                max_duration_ms INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (bucket_minute_unix_ms, tool_name)
            )",
            "CREATE INDEX IF NOT EXISTS idx_tool_metric_rollups_window
             ON tool_metric_minute_rollups(bucket_minute_unix_ms DESC, tool_name)",
            "CREATE TABLE IF NOT EXISTS llm_metric_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                occurred_at_unix_ms INTEGER NOT NULL,
                bucket_minute_unix_ms INTEGER NOT NULL,
                session_key TEXT NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                wire_api TEXT NOT NULL,
                status TEXT NOT NULL,
                error_code TEXT,
                duration_ms INTEGER NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                total_tokens INTEGER NOT NULL,
                cached_input_tokens INTEGER NOT NULL,
                reasoning_tokens INTEGER NOT NULL,
                tool_call_count INTEGER NOT NULL,
                has_tool_call INTEGER NOT NULL,
                empty_response INTEGER NOT NULL,
                is_timeout INTEGER NOT NULL,
                provider_request_id TEXT,
                provider_response_id TEXT
            )",
            "CREATE INDEX IF NOT EXISTS idx_llm_metric_events_window
             ON llm_metric_events(occurred_at_unix_ms DESC, provider, model)",
            "CREATE TABLE IF NOT EXISTS llm_metric_minute_rollups (
                bucket_minute_unix_ms INTEGER NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                wire_api TEXT NOT NULL,
                requests INTEGER NOT NULL DEFAULT 0,
                successes INTEGER NOT NULL DEFAULT 0,
                failures INTEGER NOT NULL DEFAULT 0,
                timeouts INTEGER NOT NULL DEFAULT 0,
                empty_responses INTEGER NOT NULL DEFAULT 0,
                total_duration_ms INTEGER NOT NULL DEFAULT 0,
                max_duration_ms INTEGER NOT NULL DEFAULT 0,
                total_input_tokens INTEGER NOT NULL DEFAULT 0,
                total_output_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                cached_input_tokens INTEGER NOT NULL DEFAULT 0,
                reasoning_tokens INTEGER NOT NULL DEFAULT 0,
                total_tool_calls INTEGER NOT NULL DEFAULT 0,
                requests_with_tool_calls INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (bucket_minute_unix_ms, provider, model)
            )",
            "CREATE INDEX IF NOT EXISTS idx_llm_metric_rollups_window
             ON llm_metric_minute_rollups(bucket_minute_unix_ms DESC, provider, model)",
            "CREATE TABLE IF NOT EXISTS model_tool_metric_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                occurred_at_unix_ms INTEGER NOT NULL,
                bucket_minute_unix_ms INTEGER NOT NULL,
                session_key TEXT NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                outcome TEXT NOT NULL,
                error_code TEXT,
                duration_ms INTEGER NOT NULL,
                approval_required INTEGER NOT NULL
            )",
            "CREATE INDEX IF NOT EXISTS idx_model_tool_metric_events_window
             ON model_tool_metric_events(occurred_at_unix_ms DESC, provider, model, tool_name)",
            "CREATE TABLE IF NOT EXISTS model_tool_metric_minute_rollups (
                bucket_minute_unix_ms INTEGER NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                calls INTEGER NOT NULL DEFAULT 0,
                successes INTEGER NOT NULL DEFAULT 0,
                failures INTEGER NOT NULL DEFAULT 0,
                approvals_required INTEGER NOT NULL DEFAULT 0,
                total_duration_ms INTEGER NOT NULL DEFAULT 0,
                max_duration_ms INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (bucket_minute_unix_ms, provider, model, tool_name)
            )",
            "CREATE INDEX IF NOT EXISTS idx_model_tool_metric_rollups_window
             ON model_tool_metric_minute_rollups(bucket_minute_unix_ms DESC, provider, model, tool_name)",
            "CREATE TABLE IF NOT EXISTS turn_metric_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                occurred_at_unix_ms INTEGER NOT NULL,
                bucket_minute_unix_ms INTEGER NOT NULL,
                session_key TEXT NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                requests_in_turn INTEGER NOT NULL,
                tool_iterations INTEGER NOT NULL,
                completed INTEGER NOT NULL,
                degraded INTEGER NOT NULL,
                token_budget_exceeded INTEGER NOT NULL,
                tool_loop_exhausted INTEGER NOT NULL
            )",
            "CREATE INDEX IF NOT EXISTS idx_turn_metric_events_window
             ON turn_metric_events(occurred_at_unix_ms DESC, provider, model)",
            "CREATE TABLE IF NOT EXISTS turn_metric_minute_rollups (
                bucket_minute_unix_ms INTEGER NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                turns INTEGER NOT NULL DEFAULT 0,
                total_requests_in_turn INTEGER NOT NULL DEFAULT 0,
                total_tool_iterations INTEGER NOT NULL DEFAULT 0,
                completed INTEGER NOT NULL DEFAULT 0,
                degraded INTEGER NOT NULL DEFAULT 0,
                token_budget_exceeded INTEGER NOT NULL DEFAULT 0,
                tool_loop_exhausted INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (bucket_minute_unix_ms, provider, model)
            )",
            "CREATE INDEX IF NOT EXISTS idx_turn_metric_rollups_window
             ON turn_metric_minute_rollups(bucket_minute_unix_ms DESC, provider, model)",
        ] {
            sqlx::query(statement)
                .execute(&self.pool)
                .await
                .map_err(LocalMetricsStoreError::Init)?;
        }
        Ok(())
    }

    async fn maybe_run_maintenance(&self, now_unix_ms: i64) -> Result<(), LocalMetricsStoreError> {
        let should_run = {
            let mut last = self
                .last_maintenance_unix_ms
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if now_unix_ms.saturating_sub(*last) < self.maintenance_interval.as_millis() as i64 {
                false
            } else {
                *last = now_unix_ms;
                true
            }
        };
        if !should_run {
            return Ok(());
        }

        let cutoff_unix_ms = now_unix_ms - (self.retention_days as i64) * 24 * 60 * 60 * 1000;
        let cutoff_bucket_unix_ms = floor_bucket_minute(cutoff_unix_ms);
        for (table, column) in [
            ("tool_metric_events", "occurred_at_unix_ms"),
            ("tool_metric_minute_rollups", "bucket_minute_unix_ms"),
            ("llm_metric_events", "occurred_at_unix_ms"),
            ("llm_metric_minute_rollups", "bucket_minute_unix_ms"),
            ("model_tool_metric_events", "occurred_at_unix_ms"),
            ("model_tool_metric_minute_rollups", "bucket_minute_unix_ms"),
            ("turn_metric_events", "occurred_at_unix_ms"),
            ("turn_metric_minute_rollups", "bucket_minute_unix_ms"),
        ] {
            let cutoff = if column == "occurred_at_unix_ms" {
                cutoff_unix_ms
            } else {
                cutoff_bucket_unix_ms
            };
            sqlx::query(&format!("DELETE FROM {table} WHERE {column} < ?1"))
                .bind(cutoff)
                .execute(&self.pool)
                .await
                .map_err(LocalMetricsStoreError::Query)?;
        }
        Ok(())
    }
}

#[async_trait]
impl LocalMetricsStore for SqliteLocalMetricsStore {
    async fn record_tool_outcome(
        &self,
        event: ToolMetricEvent,
    ) -> Result<(), LocalMetricsStoreError> {
        let bucket_minute_unix_ms = floor_bucket_minute(event.occurred_at_unix_ms);
        let success = u64::from(matches!(event.status, ToolOutcomeStatus::Success));
        let failure = u64::from(matches!(event.status, ToolOutcomeStatus::Failure));

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(LocalMetricsStoreError::Query)?;
        sqlx::query(
            "INSERT INTO tool_metric_events (
                occurred_at_unix_ms,
                bucket_minute_unix_ms,
                session_key,
                tool_name,
                outcome,
                error_code,
                duration_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(event.occurred_at_unix_ms)
        .bind(bucket_minute_unix_ms)
        .bind(&event.session_key)
        .bind(&event.tool_name)
        .bind(status_as_str(event.status))
        .bind(&event.error_code)
        .bind(event.duration_ms as i64)
        .execute(&mut *tx)
        .await
        .map_err(LocalMetricsStoreError::Query)?;
        sqlx::query(
            "INSERT INTO tool_metric_minute_rollups (
                bucket_minute_unix_ms,
                tool_name,
                calls,
                successes,
                failures,
                total_duration_ms,
                max_duration_ms
            ) VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6)
            ON CONFLICT(bucket_minute_unix_ms, tool_name) DO UPDATE SET
                calls = calls + 1,
                successes = successes + excluded.successes,
                failures = failures + excluded.failures,
                total_duration_ms = total_duration_ms + excluded.total_duration_ms,
                max_duration_ms = MAX(max_duration_ms, excluded.max_duration_ms)",
        )
        .bind(bucket_minute_unix_ms)
        .bind(&event.tool_name)
        .bind(success as i64)
        .bind(failure as i64)
        .bind(event.duration_ms as i64)
        .bind(event.duration_ms as i64)
        .execute(&mut *tx)
        .await
        .map_err(LocalMetricsStoreError::Query)?;
        tx.commit().await.map_err(LocalMetricsStoreError::Query)?;
        self.maybe_run_maintenance(event.occurred_at_unix_ms).await
    }

    async fn record_model_request(
        &self,
        record: ModelRequestRecord,
    ) -> Result<(), LocalMetricsStoreError> {
        let occurred_at_unix_ms = now_unix_ms();
        let bucket_minute_unix_ms = floor_bucket_minute(occurred_at_unix_ms);
        let success = u64::from(matches!(record.status, ModelRequestStatus::Success));
        let failure = u64::from(matches!(record.status, ModelRequestStatus::Failure));
        let timeout = u64::from(
            record
                .error_code
                .as_deref()
                .is_some_and(is_timeout_error_code),
        );
        let empty_response = u64::from(record.empty_response);
        let has_tool_call = u64::from(record.has_tool_call);

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(LocalMetricsStoreError::Query)?;
        sqlx::query(
            "INSERT INTO llm_metric_events (
                occurred_at_unix_ms,
                bucket_minute_unix_ms,
                session_key,
                provider,
                model,
                wire_api,
                status,
                error_code,
                duration_ms,
                input_tokens,
                output_tokens,
                total_tokens,
                cached_input_tokens,
                reasoning_tokens,
                tool_call_count,
                has_tool_call,
                empty_response,
                is_timeout,
                provider_request_id,
                provider_response_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        )
        .bind(occurred_at_unix_ms)
        .bind(bucket_minute_unix_ms)
        .bind(&record.session_key)
        .bind(&record.provider)
        .bind(&record.model)
        .bind(&record.wire_api)
        .bind(record.status.as_str())
        .bind(&record.error_code)
        .bind(record.duration.as_millis() as i64)
        .bind(record.input_tokens as i64)
        .bind(record.output_tokens as i64)
        .bind(record.total_tokens as i64)
        .bind(record.cached_input_tokens as i64)
        .bind(record.reasoning_tokens as i64)
        .bind(record.tool_call_count as i64)
        .bind(has_tool_call as i64)
        .bind(empty_response as i64)
        .bind(timeout as i64)
        .bind(&record.provider_request_id)
        .bind(&record.provider_response_id)
        .execute(&mut *tx)
        .await
        .map_err(LocalMetricsStoreError::Query)?;
        sqlx::query(
            "INSERT INTO llm_metric_minute_rollups (
                bucket_minute_unix_ms,
                provider,
                model,
                wire_api,
                requests,
                successes,
                failures,
                timeouts,
                empty_responses,
                total_duration_ms,
                max_duration_ms,
                total_input_tokens,
                total_output_tokens,
                total_tokens,
                cached_input_tokens,
                reasoning_tokens,
                total_tool_calls,
                requests_with_tool_calls
            ) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
            ON CONFLICT(bucket_minute_unix_ms, provider, model) DO UPDATE SET
                requests = requests + 1,
                successes = successes + excluded.successes,
                failures = failures + excluded.failures,
                timeouts = timeouts + excluded.timeouts,
                empty_responses = empty_responses + excluded.empty_responses,
                total_duration_ms = total_duration_ms + excluded.total_duration_ms,
                max_duration_ms = MAX(max_duration_ms, excluded.max_duration_ms),
                total_input_tokens = total_input_tokens + excluded.total_input_tokens,
                total_output_tokens = total_output_tokens + excluded.total_output_tokens,
                total_tokens = total_tokens + excluded.total_tokens,
                cached_input_tokens = cached_input_tokens + excluded.cached_input_tokens,
                reasoning_tokens = reasoning_tokens + excluded.reasoning_tokens,
                total_tool_calls = total_tool_calls + excluded.total_tool_calls,
                requests_with_tool_calls = requests_with_tool_calls + excluded.requests_with_tool_calls",
        )
        .bind(bucket_minute_unix_ms)
        .bind(&record.provider)
        .bind(&record.model)
        .bind(&record.wire_api)
        .bind(success as i64)
        .bind(failure as i64)
        .bind(timeout as i64)
        .bind(empty_response as i64)
        .bind(record.duration.as_millis() as i64)
        .bind(record.duration.as_millis() as i64)
        .bind(record.input_tokens as i64)
        .bind(record.output_tokens as i64)
        .bind(record.total_tokens as i64)
        .bind(record.cached_input_tokens as i64)
        .bind(record.reasoning_tokens as i64)
        .bind(record.tool_call_count as i64)
        .bind(has_tool_call as i64)
        .execute(&mut *tx)
        .await
        .map_err(LocalMetricsStoreError::Query)?;
        tx.commit().await.map_err(LocalMetricsStoreError::Query)?;
        self.maybe_run_maintenance(occurred_at_unix_ms).await
    }

    async fn record_model_tool_outcome(
        &self,
        record: ModelToolOutcomeRecord,
    ) -> Result<(), LocalMetricsStoreError> {
        let occurred_at_unix_ms = now_unix_ms();
        let bucket_minute_unix_ms = floor_bucket_minute(occurred_at_unix_ms);
        let success = u64::from(matches!(record.status, ToolOutcomeStatus::Success));
        let failure = u64::from(matches!(record.status, ToolOutcomeStatus::Failure));
        let approvals_required = u64::from(record.approval_required);

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(LocalMetricsStoreError::Query)?;
        sqlx::query(
            "INSERT INTO model_tool_metric_events (
                occurred_at_unix_ms,
                bucket_minute_unix_ms,
                session_key,
                provider,
                model,
                tool_name,
                outcome,
                error_code,
                duration_ms,
                approval_required
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )
        .bind(occurred_at_unix_ms)
        .bind(bucket_minute_unix_ms)
        .bind(&record.session_key)
        .bind(&record.provider)
        .bind(&record.model)
        .bind(&record.tool_name)
        .bind(status_as_str(record.status))
        .bind(&record.error_code)
        .bind(record.duration.as_millis() as i64)
        .bind(approvals_required as i64)
        .execute(&mut *tx)
        .await
        .map_err(LocalMetricsStoreError::Query)?;
        sqlx::query(
            "INSERT INTO model_tool_metric_minute_rollups (
                bucket_minute_unix_ms,
                provider,
                model,
                tool_name,
                calls,
                successes,
                failures,
                approvals_required,
                total_duration_ms,
                max_duration_ms
            ) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(bucket_minute_unix_ms, provider, model, tool_name) DO UPDATE SET
                calls = calls + 1,
                successes = successes + excluded.successes,
                failures = failures + excluded.failures,
                approvals_required = approvals_required + excluded.approvals_required,
                total_duration_ms = total_duration_ms + excluded.total_duration_ms,
                max_duration_ms = MAX(max_duration_ms, excluded.max_duration_ms)",
        )
        .bind(bucket_minute_unix_ms)
        .bind(&record.provider)
        .bind(&record.model)
        .bind(&record.tool_name)
        .bind(success as i64)
        .bind(failure as i64)
        .bind(approvals_required as i64)
        .bind(record.duration.as_millis() as i64)
        .bind(record.duration.as_millis() as i64)
        .execute(&mut *tx)
        .await
        .map_err(LocalMetricsStoreError::Query)?;
        tx.commit().await.map_err(LocalMetricsStoreError::Query)?;
        self.maybe_run_maintenance(occurred_at_unix_ms).await
    }

    async fn record_turn_outcome(
        &self,
        record: TurnOutcomeRecord,
    ) -> Result<(), LocalMetricsStoreError> {
        let occurred_at_unix_ms = now_unix_ms();
        let bucket_minute_unix_ms = floor_bucket_minute(occurred_at_unix_ms);
        let completed = u64::from(record.completed);
        let degraded = u64::from(record.degraded);
        let token_budget_exceeded = u64::from(record.token_budget_exceeded);
        let tool_loop_exhausted = u64::from(record.tool_loop_exhausted);

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(LocalMetricsStoreError::Query)?;
        sqlx::query(
            "INSERT INTO turn_metric_events (
                occurred_at_unix_ms,
                bucket_minute_unix_ms,
                session_key,
                provider,
                model,
                requests_in_turn,
                tool_iterations,
                completed,
                degraded,
                token_budget_exceeded,
                tool_loop_exhausted
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )
        .bind(occurred_at_unix_ms)
        .bind(bucket_minute_unix_ms)
        .bind(&record.session_key)
        .bind(&record.provider)
        .bind(&record.model)
        .bind(record.requests_in_turn as i64)
        .bind(record.tool_iterations as i64)
        .bind(completed as i64)
        .bind(degraded as i64)
        .bind(token_budget_exceeded as i64)
        .bind(tool_loop_exhausted as i64)
        .execute(&mut *tx)
        .await
        .map_err(LocalMetricsStoreError::Query)?;
        sqlx::query(
            "INSERT INTO turn_metric_minute_rollups (
                bucket_minute_unix_ms,
                provider,
                model,
                turns,
                total_requests_in_turn,
                total_tool_iterations,
                completed,
                degraded,
                token_budget_exceeded,
                tool_loop_exhausted
            ) VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(bucket_minute_unix_ms, provider, model) DO UPDATE SET
                turns = turns + 1,
                total_requests_in_turn = total_requests_in_turn + excluded.total_requests_in_turn,
                total_tool_iterations = total_tool_iterations + excluded.total_tool_iterations,
                completed = completed + excluded.completed,
                degraded = degraded + excluded.degraded,
                token_budget_exceeded = token_budget_exceeded + excluded.token_budget_exceeded,
                tool_loop_exhausted = tool_loop_exhausted + excluded.tool_loop_exhausted",
        )
        .bind(bucket_minute_unix_ms)
        .bind(&record.provider)
        .bind(&record.model)
        .bind(record.requests_in_turn as i64)
        .bind(record.tool_iterations as i64)
        .bind(completed as i64)
        .bind(degraded as i64)
        .bind(token_budget_exceeded as i64)
        .bind(tool_loop_exhausted as i64)
        .execute(&mut *tx)
        .await
        .map_err(LocalMetricsStoreError::Query)?;
        tx.commit().await.map_err(LocalMetricsStoreError::Query)?;
        self.maybe_run_maintenance(occurred_at_unix_ms).await
    }

    async fn query_tool_summary(
        &self,
        query: &ToolStatsQuery,
    ) -> Result<ToolSummaryRow, LocalMetricsStoreError> {
        let now_unix_ms = now_unix_ms();
        let from_unix_ms = query.time_range.window_start_unix_ms(now_unix_ms);
        let row = sqlx::query(
            "SELECT
                COALESCE(SUM(calls), 0) AS calls,
                COALESCE(SUM(successes), 0) AS successes,
                COALESCE(SUM(failures), 0) AS failures,
                COALESCE(SUM(total_duration_ms), 0) AS total_duration_ms,
                COALESCE(MAX(max_duration_ms), 0) AS max_duration_ms
             FROM tool_metric_minute_rollups
             WHERE bucket_minute_unix_ms >= ?1",
        )
        .bind(floor_bucket_minute(from_unix_ms))
        .fetch_one(&self.pool)
        .await
        .map_err(LocalMetricsStoreError::Query)?;
        let calls = row.get::<i64, _>("calls").max(0) as u64;
        let successes = row.get::<i64, _>("successes").max(0) as u64;
        let failures = row.get::<i64, _>("failures").max(0) as u64;
        let total_duration_ms = row.get::<i64, _>("total_duration_ms").max(0) as u64;
        let max_duration_ms = row.get::<i64, _>("max_duration_ms").max(0) as u64;

        Ok(ToolSummaryRow {
            total_calls: calls,
            successes,
            failures,
            success_rate: ratio(successes, calls),
            avg_duration_ms: average(total_duration_ms, calls),
            max_duration_ms,
        })
    }

    async fn query_tool_stats(
        &self,
        query: &ToolStatsQuery,
    ) -> Result<Vec<ToolStatsRow>, LocalMetricsStoreError> {
        let now_unix_ms = now_unix_ms();
        let from_unix_ms = query.time_range.window_start_unix_ms(now_unix_ms);
        let rows = sqlx::query(
            "SELECT
                tool_name,
                SUM(calls) AS calls,
                SUM(successes) AS successes,
                SUM(failures) AS failures,
                SUM(total_duration_ms) AS total_duration_ms,
                MAX(max_duration_ms) AS max_duration_ms
             FROM tool_metric_minute_rollups
             WHERE bucket_minute_unix_ms >= ?1
             GROUP BY tool_name",
        )
        .bind(floor_bucket_minute(from_unix_ms))
        .fetch_all(&self.pool)
        .await
        .map_err(LocalMetricsStoreError::Query)?;

        let mut stats = rows
            .into_iter()
            .map(|row| {
                let calls = row.get::<i64, _>("calls").max(0) as u64;
                let successes = row.get::<i64, _>("successes").max(0) as u64;
                let failures = row.get::<i64, _>("failures").max(0) as u64;
                let total_duration_ms = row.get::<i64, _>("total_duration_ms").max(0) as u64;
                ToolStatsRow {
                    tool_name: row.get::<String, _>("tool_name"),
                    calls,
                    successes,
                    failures,
                    success_rate: ratio(successes, calls),
                    avg_duration_ms: average(total_duration_ms, calls),
                    max_duration_ms: row.get::<i64, _>("max_duration_ms").max(0) as u64,
                }
            })
            .collect::<Vec<_>>();

        stats.sort_by(|left, right| {
            right
                .calls
                .cmp(&left.calls)
                .then_with(|| left.tool_name.cmp(&right.tool_name))
        });
        stats.truncate(query.limit);
        Ok(stats)
    }

    async fn query_tool_timeseries(
        &self,
        query: &ToolStatsQuery,
    ) -> Result<Vec<ToolTimeseriesPoint>, LocalMetricsStoreError> {
        let now_unix_ms = now_unix_ms();
        let from_unix_ms = query.time_range.window_start_unix_ms(now_unix_ms);
        let rows = sqlx::query(
            "SELECT
                bucket_minute_unix_ms,
                SUM(calls) AS calls,
                SUM(successes) AS successes,
                SUM(failures) AS failures
             FROM tool_metric_minute_rollups
             WHERE bucket_minute_unix_ms >= ?1
             GROUP BY bucket_minute_unix_ms
             ORDER BY bucket_minute_unix_ms ASC",
        )
        .bind(floor_bucket_minute(from_unix_ms))
        .fetch_all(&self.pool)
        .await
        .map_err(LocalMetricsStoreError::Query)?;

        let bucket_width_ms = query.bucket_width.bucket_width_ms();
        let mut points = Vec::<ToolTimeseriesPoint>::new();
        for row in rows {
            let source_bucket = row.get::<i64, _>("bucket_minute_unix_ms");
            let collapsed_bucket = floor_to_width(source_bucket, bucket_width_ms);
            let calls = row.get::<i64, _>("calls").max(0) as u64;
            let successes = row.get::<i64, _>("successes").max(0) as u64;
            let failures = row.get::<i64, _>("failures").max(0) as u64;

            if let Some(last) = points.last_mut() {
                if last.bucket_start_unix_ms == collapsed_bucket {
                    last.calls += calls;
                    last.successes += successes;
                    last.failures += failures;
                    last.success_rate = ratio(last.successes, last.calls);
                    continue;
                }
            }

            points.push(ToolTimeseriesPoint {
                bucket_start_unix_ms: collapsed_bucket,
                calls,
                successes,
                failures,
                success_rate: ratio(successes, calls),
            });
        }
        Ok(points)
    }

    async fn query_tool_error_breakdown(
        &self,
        query: &ToolStatsQuery,
        tool_name: Option<&str>,
    ) -> Result<Vec<ToolErrorBreakdownRow>, LocalMetricsStoreError> {
        let now_unix_ms = now_unix_ms();
        let from_unix_ms = query.time_range.window_start_unix_ms(now_unix_ms);
        let rows = if let Some(tool_name) = tool_name {
            sqlx::query(
                "SELECT
                    COALESCE(error_code, 'unknown') AS error_code,
                    COUNT(*) AS failures
                 FROM tool_metric_events
                 WHERE occurred_at_unix_ms >= ?1
                   AND outcome = 'failure'
                   AND tool_name = ?2
                 GROUP BY COALESCE(error_code, 'unknown')
                 ORDER BY failures DESC, error_code ASC
                 LIMIT ?3",
            )
            .bind(from_unix_ms)
            .bind(tool_name)
            .bind(query.limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(LocalMetricsStoreError::Query)?
        } else {
            sqlx::query(
                "SELECT
                    COALESCE(error_code, 'unknown') AS error_code,
                    COUNT(*) AS failures
                 FROM tool_metric_events
                 WHERE occurred_at_unix_ms >= ?1
                   AND outcome = 'failure'
                 GROUP BY COALESCE(error_code, 'unknown')
                 ORDER BY failures DESC, error_code ASC
                 LIMIT ?2",
            )
            .bind(from_unix_ms)
            .bind(query.limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(LocalMetricsStoreError::Query)?
        };

        Ok(rows
            .into_iter()
            .map(|row| ToolErrorBreakdownRow {
                error_code: row.get::<String, _>("error_code"),
                failures: row.get::<i64, _>("failures").max(0) as u64,
            })
            .collect())
    }

    async fn query_tool_dashboard_snapshot(
        &self,
        query: &ToolStatsQuery,
        tool_name: Option<&str>,
    ) -> Result<ToolDashboardSnapshot, LocalMetricsStoreError> {
        let summary = self.query_tool_summary(query).await?;
        let top_by_calls = self.query_tool_stats(query).await?;
        let mut top_by_failure_rate = top_by_calls.clone();
        top_by_failure_rate.sort_by(|left, right| {
            right
                .failures
                .cmp(&left.failures)
                .then_with(|| right.success_rate.total_cmp(&left.success_rate))
                .then_with(|| right.calls.cmp(&left.calls))
        });
        top_by_failure_rate.truncate(query.limit);
        let timeseries = self.query_tool_timeseries(query).await?;
        let breakdown_tool_name =
            tool_name.or_else(|| top_by_calls.first().map(|row| row.tool_name.as_str()));
        let error_breakdown = self
            .query_tool_error_breakdown(query, breakdown_tool_name)
            .await?;

        Ok(ToolDashboardSnapshot {
            generated_at_unix_ms: now_unix_ms(),
            summary,
            top_by_calls,
            top_by_failure_rate,
            timeseries,
            error_breakdown,
        })
    }

    async fn query_model_dashboard_snapshot(
        &self,
        query: &ModelStatsQuery,
    ) -> Result<ModelDashboardSnapshot, LocalMetricsStoreError> {
        let now_unix_ms = now_unix_ms();
        let from_unix_ms = query.time_range.window_start_unix_ms(now_unix_ms);
        let provider_filter = query.provider.as_deref().filter(|value| !value.is_empty());
        let model_filter = query.model.as_deref().filter(|value| !value.is_empty());

        let providers = sqlx::query(
            "SELECT DISTINCT provider
             FROM llm_metric_events
             WHERE occurred_at_unix_ms >= ?1
             ORDER BY provider ASC",
        )
        .bind(from_unix_ms)
        .fetch_all(&self.pool)
        .await
        .map_err(LocalMetricsStoreError::Query)?
        .into_iter()
        .map(|row| row.get::<String, _>("provider"))
        .collect::<Vec<_>>();

        let models = sqlx::query(
            "SELECT DISTINCT model
             FROM llm_metric_events
             WHERE occurred_at_unix_ms >= ?1
               AND (?2 IS NULL OR provider = ?2)
             ORDER BY model ASC",
        )
        .bind(from_unix_ms)
        .bind(provider_filter)
        .fetch_all(&self.pool)
        .await
        .map_err(LocalMetricsStoreError::Query)?
        .into_iter()
        .map(|row| row.get::<String, _>("model"))
        .collect::<Vec<_>>();

        let rows = sqlx::query(
            "SELECT
                provider,
                model,
                MIN(wire_api) AS wire_api,
                COUNT(*) AS requests,
                SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) AS successes,
                SUM(CASE WHEN status = 'failure' THEN 1 ELSE 0 END) AS failures,
                SUM(is_timeout) AS timeouts,
                SUM(empty_response) AS empty_responses,
                SUM(duration_ms) AS total_duration_ms,
                MAX(duration_ms) AS max_duration_ms,
                SUM(input_tokens) AS total_input_tokens,
                SUM(output_tokens) AS total_output_tokens,
                SUM(total_tokens) AS total_tokens,
                SUM(cached_input_tokens) AS cached_input_tokens,
                SUM(reasoning_tokens) AS reasoning_tokens,
                SUM(tool_call_count) AS total_tool_calls,
                SUM(has_tool_call) AS requests_with_tool_calls
             FROM llm_metric_events
             WHERE occurred_at_unix_ms >= ?1
               AND (?2 IS NULL OR provider = ?2)
               AND (?3 IS NULL OR model = ?3)
             GROUP BY provider, model
             ORDER BY requests DESC, provider ASC, model ASC",
        )
        .bind(from_unix_ms)
        .bind(provider_filter)
        .bind(model_filter)
        .fetch_all(&self.pool)
        .await
        .map_err(LocalMetricsStoreError::Query)?;

        let turn_rows = sqlx::query(
            "SELECT
                provider,
                model,
                SUM(turns) AS turns,
                SUM(total_requests_in_turn) AS total_requests_in_turn,
                SUM(total_tool_iterations) AS total_tool_iterations,
                SUM(completed) AS completed,
                SUM(degraded) AS degraded,
                SUM(token_budget_exceeded) AS token_budget_exceeded,
                SUM(tool_loop_exhausted) AS tool_loop_exhausted
             FROM turn_metric_minute_rollups
             WHERE bucket_minute_unix_ms >= ?1
               AND (?2 IS NULL OR provider = ?2)
               AND (?3 IS NULL OR model = ?3)
             GROUP BY provider, model",
        )
        .bind(floor_bucket_minute(from_unix_ms))
        .bind(provider_filter)
        .bind(model_filter)
        .fetch_all(&self.pool)
        .await
        .map_err(LocalMetricsStoreError::Query)?;

        let tool_rows = sqlx::query(
            "SELECT
                provider,
                model,
                SUM(calls) AS calls,
                SUM(successes) AS successes,
                SUM(failures) AS failures,
                SUM(approvals_required) AS approvals_required
             FROM model_tool_metric_minute_rollups
             WHERE bucket_minute_unix_ms >= ?1
               AND (?2 IS NULL OR provider = ?2)
               AND (?3 IS NULL OR model = ?3)
             GROUP BY provider, model",
        )
        .bind(floor_bucket_minute(from_unix_ms))
        .bind(provider_filter)
        .bind(model_filter)
        .fetch_all(&self.pool)
        .await
        .map_err(LocalMetricsStoreError::Query)?;

        let turn_map = turn_rows
            .into_iter()
            .map(|row| {
                (
                    (
                        row.get::<String, _>("provider"),
                        row.get::<String, _>("model"),
                    ),
                    TurnAgg {
                        turns: row.get::<i64, _>("turns").max(0) as u64,
                        total_requests_in_turn: row.get::<i64, _>("total_requests_in_turn").max(0)
                            as u64,
                        total_tool_iterations: row.get::<i64, _>("total_tool_iterations").max(0)
                            as u64,
                        completed: row.get::<i64, _>("completed").max(0) as u64,
                        degraded: row.get::<i64, _>("degraded").max(0) as u64,
                        token_budget_exceeded: row.get::<i64, _>("token_budget_exceeded").max(0)
                            as u64,
                        tool_loop_exhausted: row.get::<i64, _>("tool_loop_exhausted").max(0) as u64,
                    },
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();

        let tool_map = tool_rows
            .into_iter()
            .map(|row| {
                (
                    (
                        row.get::<String, _>("provider"),
                        row.get::<String, _>("model"),
                    ),
                    ModelToolAgg {
                        calls: row.get::<i64, _>("calls").max(0) as u64,
                        successes: row.get::<i64, _>("successes").max(0) as u64,
                        approvals_required: row.get::<i64, _>("approvals_required").max(0) as u64,
                    },
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let model_percentile_rows = sqlx::query(
            "SELECT provider, model, duration_ms
             FROM llm_metric_events
             WHERE occurred_at_unix_ms >= ?1
               AND (?2 IS NULL OR provider = ?2)
               AND (?3 IS NULL OR model = ?3)
             ORDER BY provider ASC, model ASC, duration_ms ASC",
        )
        .bind(from_unix_ms)
        .bind(provider_filter)
        .bind(model_filter)
        .fetch_all(&self.pool)
        .await
        .map_err(LocalMetricsStoreError::Query)?;
        let mut percentile_map = std::collections::BTreeMap::<(String, String), Vec<f64>>::new();
        for row in model_percentile_rows {
            percentile_map
                .entry((
                    row.get::<String, _>("provider"),
                    row.get::<String, _>("model"),
                ))
                .or_default()
                .push(row.get::<i64, _>("duration_ms").max(0) as f64);
        }

        let mut model_rows = rows
            .into_iter()
            .map(|row| {
                let provider = row.get::<String, _>("provider");
                let model = row.get::<String, _>("model");
                let requests = row.get::<i64, _>("requests").max(0) as u64;
                let successes = row.get::<i64, _>("successes").max(0) as u64;
                let failures = row.get::<i64, _>("failures").max(0) as u64;
                let timeouts = row.get::<i64, _>("timeouts").max(0) as u64;
                let empty_responses = row.get::<i64, _>("empty_responses").max(0) as u64;
                let total_duration_ms = row.get::<i64, _>("total_duration_ms").max(0) as u64;
                let total_input_tokens = row.get::<i64, _>("total_input_tokens").max(0) as u64;
                let total_output_tokens = row.get::<i64, _>("total_output_tokens").max(0) as u64;
                let total_tokens = row.get::<i64, _>("total_tokens").max(0) as u64;
                let cached_input_tokens = row.get::<i64, _>("cached_input_tokens").max(0) as u64;
                let reasoning_tokens = row.get::<i64, _>("reasoning_tokens").max(0) as u64;
                let total_tool_calls = row.get::<i64, _>("total_tool_calls").max(0) as u64;
                let requests_with_tool_calls =
                    row.get::<i64, _>("requests_with_tool_calls").max(0) as u64;
                let turn_agg = turn_map
                    .get(&(provider.clone(), model.clone()))
                    .cloned()
                    .unwrap_or_default();
                let tool_agg = tool_map
                    .get(&(provider.clone(), model.clone()))
                    .cloned()
                    .unwrap_or_default();
                let p95_duration_ms = percentile_map
                    .get(&(provider.clone(), model.clone()))
                    .map(|values| percentile(values, 0.95))
                    .unwrap_or(0.0);
                let estimated_cost_usd =
                    estimate_cost_usd(&provider, &model, total_input_tokens, total_output_tokens);
                let completed_turns = turn_agg.completed;
                let successful_tool_calls = tool_agg.successes;

                ModelStatsRow {
                    provider,
                    model,
                    wire_api: row.get::<String, _>("wire_api"),
                    requests,
                    successes,
                    failures,
                    request_success_rate: ratio(successes, requests),
                    request_failure_rate: ratio(failures, requests),
                    timeout_rate: ratio(timeouts, requests),
                    empty_response_rate: ratio(empty_responses, requests),
                    avg_duration_ms: average(total_duration_ms, requests),
                    p95_duration_ms,
                    max_duration_ms: row.get::<i64, _>("max_duration_ms").max(0) as u64,
                    total_input_tokens,
                    total_output_tokens,
                    total_tokens,
                    avg_input_tokens: average(total_input_tokens, requests),
                    avg_output_tokens: average(total_output_tokens, requests),
                    avg_total_tokens: average(total_tokens, requests),
                    input_output_ratio: ratio_f64(
                        total_input_tokens as f64,
                        total_output_tokens as f64,
                    ),
                    cached_input_tokens_ratio: ratio(cached_input_tokens, total_input_tokens),
                    reasoning_tokens_ratio: ratio(reasoning_tokens, total_tokens),
                    tokens_per_second: tokens_per_second(total_tokens, total_duration_ms),
                    tool_call_rate: ratio(requests_with_tool_calls, requests),
                    avg_tool_calls_per_request: average(total_tool_calls, requests),
                    tool_success_rate_by_model: ratio(tool_agg.successes, tool_agg.calls),
                    approval_required_rate: ratio(tool_agg.approvals_required, tool_agg.calls),
                    avg_requests_per_turn: average(turn_agg.total_requests_in_turn, turn_agg.turns),
                    avg_tool_iterations_per_turn: average(
                        turn_agg.total_tool_iterations,
                        turn_agg.turns,
                    ),
                    turn_completion_rate: ratio(turn_agg.completed, turn_agg.turns),
                    degraded_run_rate: ratio(turn_agg.degraded, turn_agg.turns),
                    token_budget_exceeded_rate: ratio(
                        turn_agg.token_budget_exceeded,
                        turn_agg.turns,
                    ),
                    tool_loop_exhausted_rate: ratio(turn_agg.tool_loop_exhausted, turn_agg.turns),
                    estimated_cost_usd,
                    cost_per_successful_turn: divide_option(estimated_cost_usd, completed_turns),
                    cost_per_tool_success: divide_option(estimated_cost_usd, successful_tool_calls),
                }
            })
            .collect::<Vec<_>>();
        if query.limit > 0 && model_rows.len() > query.limit {
            model_rows.truncate(query.limit);
        }

        let summary = summarize_model_rows(&model_rows);
        let latency_percentiles = query_model_latency_percentiles(
            &self.pool,
            from_unix_ms,
            provider_filter,
            model_filter,
        )
        .await?;
        let token_composition_row = sqlx::query(
            "SELECT
                COALESCE(SUM(cached_input_tokens), 0) AS cached_input_tokens,
                COALESCE(SUM(reasoning_tokens), 0) AS reasoning_tokens
             FROM llm_metric_events
             WHERE occurred_at_unix_ms >= ?1
               AND (?2 IS NULL OR provider = ?2)
               AND (?3 IS NULL OR model = ?3)",
        )
        .bind(from_unix_ms)
        .bind(provider_filter)
        .bind(model_filter)
        .fetch_one(&self.pool)
        .await
        .map_err(LocalMetricsStoreError::Query)?;
        let token_composition = ModelTokenCompositionRow {
            input_tokens: summary.total_input_tokens,
            output_tokens: summary.total_output_tokens,
            cached_input_tokens: token_composition_row
                .get::<i64, _>("cached_input_tokens")
                .max(0) as u64,
            reasoning_tokens: token_composition_row
                .get::<i64, _>("reasoning_tokens")
                .max(0) as u64,
        };
        let turn_efficiency = TurnEfficiencyRow {
            avg_requests_per_turn: summary.avg_requests_per_turn,
            avg_tool_iterations_per_turn: summary.avg_tool_iterations_per_turn,
            turn_completion_rate: summary.turn_completion_rate,
            degraded_run_rate: summary.degraded_run_rate,
            token_budget_exceeded_rate: summary.token_budget_exceeded_rate,
            tool_loop_exhausted_rate: summary.tool_loop_exhausted_rate,
        };
        let timeseries = query_model_timeseries(
            &self.pool,
            from_unix_ms,
            query.bucket_width.bucket_width_ms(),
            provider_filter,
            model_filter,
        )
        .await?;
        let error_breakdown = query_model_error_breakdown(
            &self.pool,
            from_unix_ms,
            provider_filter,
            model_filter,
            query.limit,
        )
        .await?;
        let tool_breakdown = query_model_tool_breakdown(
            &self.pool,
            from_unix_ms,
            provider_filter,
            model_filter,
            query.limit,
        )
        .await?;

        Ok(ModelDashboardSnapshot {
            generated_at_unix_ms: now_unix_ms,
            providers,
            models,
            summary,
            latency_percentiles,
            token_composition,
            turn_efficiency,
            model_rows,
            timeseries,
            error_breakdown,
            tool_breakdown,
        })
    }
}

#[derive(Debug, Clone, Default)]
struct TurnAgg {
    turns: u64,
    total_requests_in_turn: u64,
    total_tool_iterations: u64,
    completed: u64,
    degraded: u64,
    token_budget_exceeded: u64,
    tool_loop_exhausted: u64,
}

#[derive(Debug, Clone, Default)]
struct ModelToolAgg {
    calls: u64,
    successes: u64,
    approvals_required: u64,
}

async fn query_model_latency_percentiles(
    pool: &SqlitePool,
    from_unix_ms: i64,
    provider_filter: Option<&str>,
    model_filter: Option<&str>,
) -> Result<ModelLatencyPercentilesRow, LocalMetricsStoreError> {
    let mut values = sqlx::query(
        "SELECT duration_ms
         FROM llm_metric_events
         WHERE occurred_at_unix_ms >= ?1
           AND (?2 IS NULL OR provider = ?2)
           AND (?3 IS NULL OR model = ?3)
         ORDER BY duration_ms ASC",
    )
    .bind(from_unix_ms)
    .bind(provider_filter)
    .bind(model_filter)
    .fetch_all(pool)
    .await
    .map_err(LocalMetricsStoreError::Query)?
    .into_iter()
    .map(|row| row.get::<i64, _>("duration_ms").max(0) as f64)
    .collect::<Vec<_>>();
    if values.is_empty() {
        return Ok(ModelLatencyPercentilesRow::default());
    }
    values.sort_by(|left, right| left.total_cmp(right));
    Ok(ModelLatencyPercentilesRow {
        p50_duration_ms: percentile(&values, 0.50),
        p95_duration_ms: percentile(&values, 0.95),
        p99_duration_ms: percentile(&values, 0.99),
    })
}

async fn query_model_timeseries(
    pool: &SqlitePool,
    from_unix_ms: i64,
    bucket_width_ms: i64,
    provider_filter: Option<&str>,
    model_filter: Option<&str>,
) -> Result<Vec<ModelTimeseriesPoint>, LocalMetricsStoreError> {
    let llm_rows = sqlx::query(
        "SELECT
            bucket_minute_unix_ms,
            SUM(requests) AS requests,
            SUM(successes) AS successes,
            SUM(failures) AS failures,
            SUM(total_duration_ms) AS total_duration_ms,
            SUM(total_tokens) AS total_tokens,
            SUM(total_tool_calls) AS total_tool_calls,
            SUM(requests_with_tool_calls) AS requests_with_tool_calls
         FROM llm_metric_minute_rollups
         WHERE bucket_minute_unix_ms >= ?1
           AND (?2 IS NULL OR provider = ?2)
           AND (?3 IS NULL OR model = ?3)
         GROUP BY bucket_minute_unix_ms
         ORDER BY bucket_minute_unix_ms ASC",
    )
    .bind(floor_bucket_minute(from_unix_ms))
    .bind(provider_filter)
    .bind(model_filter)
    .fetch_all(pool)
    .await
    .map_err(LocalMetricsStoreError::Query)?;

    let turn_rows = sqlx::query(
        "SELECT
            bucket_minute_unix_ms,
            SUM(turns) AS turns,
            SUM(total_requests_in_turn) AS total_requests_in_turn,
            SUM(total_tool_iterations) AS total_tool_iterations
         FROM turn_metric_minute_rollups
         WHERE bucket_minute_unix_ms >= ?1
           AND (?2 IS NULL OR provider = ?2)
           AND (?3 IS NULL OR model = ?3)
         GROUP BY bucket_minute_unix_ms
         ORDER BY bucket_minute_unix_ms ASC",
    )
    .bind(floor_bucket_minute(from_unix_ms))
    .bind(provider_filter)
    .bind(model_filter)
    .fetch_all(pool)
    .await
    .map_err(LocalMetricsStoreError::Query)?;

    let tool_rows = sqlx::query(
        "SELECT
            bucket_minute_unix_ms,
            SUM(calls) AS calls,
            SUM(successes) AS successes
         FROM model_tool_metric_minute_rollups
         WHERE bucket_minute_unix_ms >= ?1
           AND (?2 IS NULL OR provider = ?2)
           AND (?3 IS NULL OR model = ?3)
         GROUP BY bucket_minute_unix_ms
         ORDER BY bucket_minute_unix_ms ASC",
    )
    .bind(floor_bucket_minute(from_unix_ms))
    .bind(provider_filter)
    .bind(model_filter)
    .fetch_all(pool)
    .await
    .map_err(LocalMetricsStoreError::Query)?;

    let mut by_bucket = std::collections::BTreeMap::<i64, ModelTimeseriesPoint>::new();
    for row in llm_rows {
        let bucket = floor_to_width(row.get::<i64, _>("bucket_minute_unix_ms"), bucket_width_ms);
        let entry = by_bucket.entry(bucket).or_default();
        entry.bucket_start_unix_ms = bucket;
        entry.requests += row.get::<i64, _>("requests").max(0) as u64;
        entry.successes += row.get::<i64, _>("successes").max(0) as u64;
        entry.failures += row.get::<i64, _>("failures").max(0) as u64;
        let total_duration_ms = row.get::<i64, _>("total_duration_ms").max(0) as u64;
        entry.avg_duration_ms += total_duration_ms as f64;
        entry.total_tokens += row.get::<i64, _>("total_tokens").max(0) as u64;
        let requests_with_tool_calls = row.get::<i64, _>("requests_with_tool_calls").max(0) as u64;
        let total_tool_calls = row.get::<i64, _>("total_tool_calls").max(0) as u64;
        entry.tool_call_rate += requests_with_tool_calls as f64;
        entry.p95_duration_ms = 0.0;
        if entry.requests > 0 {
            entry.avg_duration_ms /= entry.requests as f64;
        }
        entry.tool_call_rate = if entry.requests > 0 {
            total_tool_calls as f64 / entry.requests as f64
        } else {
            0.0
        };
    }

    let p95_rows = sqlx::query(
        "SELECT bucket_minute_unix_ms, duration_ms
         FROM llm_metric_events
         WHERE occurred_at_unix_ms >= ?1
           AND (?2 IS NULL OR provider = ?2)
           AND (?3 IS NULL OR model = ?3)
         ORDER BY bucket_minute_unix_ms ASC, duration_ms ASC",
    )
    .bind(from_unix_ms)
    .bind(provider_filter)
    .bind(model_filter)
    .fetch_all(pool)
    .await
    .map_err(LocalMetricsStoreError::Query)?;
    let mut bucket_durations = std::collections::BTreeMap::<i64, Vec<f64>>::new();
    for row in p95_rows {
        let bucket = floor_to_width(row.get::<i64, _>("bucket_minute_unix_ms"), bucket_width_ms);
        bucket_durations
            .entry(bucket)
            .or_default()
            .push(row.get::<i64, _>("duration_ms").max(0) as f64);
    }
    for (bucket, mut durations) in bucket_durations {
        durations.sort_by(|left, right| left.total_cmp(right));
        if let Some(point) = by_bucket.get_mut(&bucket) {
            point.p95_duration_ms = percentile(&durations, 0.95);
        }
    }

    for row in turn_rows {
        let bucket = floor_to_width(row.get::<i64, _>("bucket_minute_unix_ms"), bucket_width_ms);
        let entry = by_bucket.entry(bucket).or_default();
        entry.bucket_start_unix_ms = bucket;
        let turns = row.get::<i64, _>("turns").max(0) as u64;
        let total_requests_in_turn = row.get::<i64, _>("total_requests_in_turn").max(0) as u64;
        let total_tool_iterations = row.get::<i64, _>("total_tool_iterations").max(0) as u64;
        entry.avg_requests_per_turn = average(total_requests_in_turn, turns);
        entry.avg_tool_iterations_per_turn = average(total_tool_iterations, turns);
    }

    for row in tool_rows {
        let bucket = floor_to_width(row.get::<i64, _>("bucket_minute_unix_ms"), bucket_width_ms);
        let entry = by_bucket.entry(bucket).or_default();
        entry.bucket_start_unix_ms = bucket;
        let calls = row.get::<i64, _>("calls").max(0) as u64;
        let successes = row.get::<i64, _>("successes").max(0) as u64;
        entry.tool_success_rate = ratio(successes, calls);
    }

    let mut points = by_bucket.into_values().collect::<Vec<_>>();
    for point in &mut points {
        point.request_success_rate = ratio(point.successes, point.requests);
    }
    Ok(points)
}

async fn query_model_error_breakdown(
    pool: &SqlitePool,
    from_unix_ms: i64,
    provider_filter: Option<&str>,
    model_filter: Option<&str>,
    limit: usize,
) -> Result<Vec<ModelErrorBreakdownRow>, LocalMetricsStoreError> {
    let rows = sqlx::query(
        "SELECT
            COALESCE(error_code, 'unknown') AS error_code,
            COUNT(*) AS failures
         FROM llm_metric_events
         WHERE occurred_at_unix_ms >= ?1
           AND status = 'failure'
           AND (?2 IS NULL OR provider = ?2)
           AND (?3 IS NULL OR model = ?3)
         GROUP BY COALESCE(error_code, 'unknown')
         ORDER BY failures DESC, error_code ASC
         LIMIT ?4",
    )
    .bind(from_unix_ms)
    .bind(provider_filter)
    .bind(model_filter)
    .bind(limit.max(1) as i64)
    .fetch_all(pool)
    .await
    .map_err(LocalMetricsStoreError::Query)?;
    Ok(rows
        .into_iter()
        .map(|row| ModelErrorBreakdownRow {
            error_code: row.get::<String, _>("error_code"),
            failures: row.get::<i64, _>("failures").max(0) as u64,
        })
        .collect())
}

async fn query_model_tool_breakdown(
    pool: &SqlitePool,
    from_unix_ms: i64,
    provider_filter: Option<&str>,
    model_filter: Option<&str>,
    limit: usize,
) -> Result<Vec<ModelToolBreakdownRow>, LocalMetricsStoreError> {
    let rows = sqlx::query(
        "SELECT
            tool_name,
            SUM(calls) AS calls,
            SUM(successes) AS successes,
            SUM(failures) AS failures,
            SUM(approvals_required) AS approvals_required,
            SUM(total_duration_ms) AS total_duration_ms
         FROM model_tool_metric_minute_rollups
         WHERE bucket_minute_unix_ms >= ?1
           AND (?2 IS NULL OR provider = ?2)
           AND (?3 IS NULL OR model = ?3)
         GROUP BY tool_name
         ORDER BY calls DESC, tool_name ASC
         LIMIT ?4",
    )
    .bind(floor_bucket_minute(from_unix_ms))
    .bind(provider_filter)
    .bind(model_filter)
    .bind(limit.max(1) as i64)
    .fetch_all(pool)
    .await
    .map_err(LocalMetricsStoreError::Query)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let calls = row.get::<i64, _>("calls").max(0) as u64;
            let successes = row.get::<i64, _>("successes").max(0) as u64;
            let approvals_required = row.get::<i64, _>("approvals_required").max(0) as u64;
            let total_duration_ms = row.get::<i64, _>("total_duration_ms").max(0) as u64;
            ModelToolBreakdownRow {
                tool_name: row.get::<String, _>("tool_name"),
                calls,
                successes,
                failures: row.get::<i64, _>("failures").max(0) as u64,
                success_rate: ratio(successes, calls),
                approval_required_rate: ratio(approvals_required, calls),
                avg_duration_ms: average(total_duration_ms, calls),
            }
        })
        .collect())
}

fn summarize_model_rows(rows: &[ModelStatsRow]) -> ModelSummaryRow {
    let total_requests = sum_rows(rows, |row| row.requests);
    let successes = sum_rows(rows, |row| row.successes);
    let failures = sum_rows(rows, |row| row.failures);
    let total_input_tokens = sum_rows(rows, |row| row.total_input_tokens);
    let total_output_tokens = sum_rows(rows, |row| row.total_output_tokens);
    let total_tokens = sum_rows(rows, |row| row.total_tokens);
    let avg_duration_ms = weighted_average(rows, |row| row.avg_duration_ms, |row| row.requests);
    let estimated_cost_usd = rows.iter().try_fold(0.0, |acc, row| {
        row.estimated_cost_usd.map(|cost| acc + cost)
    });
    let tool_successes = sum_rows(rows, |row| {
        ((row.tool_success_rate_by_model * row.requests as f64).round() as u64).min(row.requests)
    });
    let completed_turns = rows
        .iter()
        .map(|row| (row.turn_completion_rate * row.avg_requests_per_turn.max(1.0)).round() as u64)
        .sum::<u64>();

    ModelSummaryRow {
        total_requests,
        successes,
        failures,
        request_success_rate: ratio(successes, total_requests),
        request_failure_rate: ratio(failures, total_requests),
        timeout_rate: weighted_average(rows, |row| row.timeout_rate, |row| row.requests),
        empty_response_rate: weighted_average(
            rows,
            |row| row.empty_response_rate,
            |row| row.requests,
        ),
        avg_duration_ms,
        max_duration_ms: rows
            .iter()
            .map(|row| row.max_duration_ms)
            .max()
            .unwrap_or(0),
        total_input_tokens,
        total_output_tokens,
        total_tokens,
        avg_input_tokens: average(total_input_tokens, total_requests),
        avg_output_tokens: average(total_output_tokens, total_requests),
        avg_total_tokens: average(total_tokens, total_requests),
        input_output_ratio: ratio_f64(total_input_tokens as f64, total_output_tokens as f64),
        cached_input_tokens_ratio: weighted_average(
            rows,
            |row| row.cached_input_tokens_ratio,
            |row| row.total_input_tokens,
        ),
        reasoning_tokens_ratio: weighted_average(
            rows,
            |row| row.reasoning_tokens_ratio,
            |row| row.total_tokens,
        ),
        tokens_per_second: tokens_per_second(
            total_tokens,
            (avg_duration_ms * total_requests as f64) as u64,
        ),
        tool_call_rate: weighted_average(rows, |row| row.tool_call_rate, |row| row.requests),
        avg_tool_calls_per_request: weighted_average(
            rows,
            |row| row.avg_tool_calls_per_request,
            |row| row.requests,
        ),
        tool_success_rate_by_model: weighted_average(
            rows,
            |row| row.tool_success_rate_by_model,
            |row| row.requests,
        ),
        approval_required_rate: weighted_average(
            rows,
            |row| row.approval_required_rate,
            |row| row.requests,
        ),
        avg_requests_per_turn: weighted_average(
            rows,
            |row| row.avg_requests_per_turn,
            |row| row.requests,
        ),
        avg_tool_iterations_per_turn: weighted_average(
            rows,
            |row| row.avg_tool_iterations_per_turn,
            |row| row.requests,
        ),
        turn_completion_rate: weighted_average(
            rows,
            |row| row.turn_completion_rate,
            |row| row.requests,
        ),
        degraded_run_rate: weighted_average(rows, |row| row.degraded_run_rate, |row| row.requests),
        token_budget_exceeded_rate: weighted_average(
            rows,
            |row| row.token_budget_exceeded_rate,
            |row| row.requests,
        ),
        tool_loop_exhausted_rate: weighted_average(
            rows,
            |row| row.tool_loop_exhausted_rate,
            |row| row.requests,
        ),
        estimated_cost_usd,
        cost_per_successful_turn: divide_option(estimated_cost_usd, completed_turns),
        cost_per_tool_success: divide_option(estimated_cost_usd, tool_successes),
    }
}

fn now_unix_ms() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp_nanos() as i64 / 1_000_000
}

fn floor_bucket_minute(unix_ms: i64) -> i64 {
    floor_to_width(unix_ms, 60 * 1000)
}

fn floor_to_width(unix_ms: i64, width_ms: i64) -> i64 {
    unix_ms - unix_ms.rem_euclid(width_ms)
}

fn ratio(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 / total as f64
    }
}

fn ratio_f64(left: f64, right: f64) -> f64 {
    if right <= 0.0 { 0.0 } else { left / right }
}

fn average(total: u64, count: u64) -> f64 {
    if count == 0 {
        0.0
    } else {
        total as f64 / count as f64
    }
}

fn weighted_average<T>(rows: &[T], value: impl Fn(&T) -> f64, weight: impl Fn(&T) -> u64) -> f64 {
    let total_weight = rows.iter().map(&weight).sum::<u64>();
    if total_weight == 0 {
        return 0.0;
    }
    rows.iter()
        .map(|row| value(row) * weight(row) as f64)
        .sum::<f64>()
        / total_weight as f64
}

fn tokens_per_second(total_tokens: u64, total_duration_ms: u64) -> f64 {
    if total_duration_ms == 0 {
        0.0
    } else {
        total_tokens as f64 / (total_duration_ms as f64 / 1000.0)
    }
}

fn status_as_str(status: ToolOutcomeStatus) -> &'static str {
    match status {
        ToolOutcomeStatus::Success => "success",
        ToolOutcomeStatus::Failure => "failure",
    }
}

fn is_timeout_error_code(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized.contains("timeout")
}

fn percentile(sorted_values: &[f64], pct: f64) -> f64 {
    if sorted_values.is_empty() {
        return 0.0;
    }
    let index = ((sorted_values.len() - 1) as f64 * pct).round() as usize;
    sorted_values[index.min(sorted_values.len() - 1)]
}

fn divide_option(cost: Option<f64>, denominator: u64) -> Option<f64> {
    if denominator == 0 {
        None
    } else {
        cost.map(|value| value / denominator as f64)
    }
}

fn sum_rows<T>(rows: &[T], value: impl Fn(&T) -> u64) -> u64 {
    rows.iter().map(value).sum()
}

fn estimate_cost_usd(
    provider: &str,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> Option<f64> {
    let (input_rate, output_rate) = match (provider, model) {
        ("openai", "gpt-4.1") => (2.0, 8.0),
        ("openai", "gpt-4.1-mini") => (0.4, 1.6),
        ("openai", "gpt-4o") => (2.5, 10.0),
        ("openai", "gpt-4o-mini") => (0.15, 0.6),
        ("anthropic", "claude-3-7-sonnet") => (3.0, 15.0),
        ("anthropic", "claude-sonnet-4") => (3.0, 15.0),
        ("anthropic", "claude-opus-4") => (15.0, 75.0),
        _ => return None,
    };
    Some(
        (input_tokens as f64 / 1_000_000.0) * input_rate
            + (output_tokens as f64 / 1_000_000.0) * output_rate,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn create_store() -> SqliteLocalMetricsStore {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let db_path = std::env::temp_dir().join(format!("klaw-observability-{suffix}.db"));
        SqliteLocalMetricsStore::open(
            &db_path,
            &LocalStoreConfig {
                enabled: true,
                retention_days: 7,
                flush_interval_seconds: 1,
            },
        )
        .await
        .expect("store should open")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn record_tool_outcome_updates_summary_and_stats() {
        let store = create_store().await;
        let now = now_unix_ms();
        store
            .record_tool_outcome(ToolMetricEvent {
                occurred_at_unix_ms: now,
                session_key: "stdio:test".to_string(),
                tool_name: "shell".to_string(),
                status: ToolOutcomeStatus::Success,
                error_code: None,
                duration_ms: 40,
            })
            .await
            .expect("success event should be recorded");
        store
            .record_tool_outcome(ToolMetricEvent {
                occurred_at_unix_ms: now,
                session_key: "stdio:test".to_string(),
                tool_name: "shell".to_string(),
                status: ToolOutcomeStatus::Failure,
                error_code: Some("timeout".to_string()),
                duration_ms: 60,
            })
            .await
            .expect("failure event should be recorded");

        let query = ToolStatsQuery::default();
        let summary = store
            .query_tool_summary(&query)
            .await
            .expect("summary query should succeed");
        assert_eq!(summary.total_calls, 2);
        assert_eq!(summary.successes, 1);
        assert_eq!(summary.failures, 1);

        let stats = store
            .query_tool_stats(&query)
            .await
            .expect("stats query should succeed");
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].tool_name, "shell");
        assert_eq!(stats[0].calls, 2);
        assert_eq!(stats[0].failures, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn query_tool_error_breakdown_filters_by_tool_name() {
        let store = create_store().await;
        let now = now_unix_ms();
        for tool_name in ["shell", "web_fetch"] {
            store
                .record_tool_outcome(ToolMetricEvent {
                    occurred_at_unix_ms: now,
                    session_key: "stdio:test".to_string(),
                    tool_name: tool_name.to_string(),
                    status: ToolOutcomeStatus::Failure,
                    error_code: Some("timeout".to_string()),
                    duration_ms: 20,
                })
                .await
                .expect("failure event should be recorded");
        }

        let rows = store
            .query_tool_error_breakdown(&ToolStatsQuery::default(), Some("shell"))
            .await
            .expect("breakdown query should succeed");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].error_code, "timeout");
        assert_eq!(rows[0].failures, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn record_model_metrics_populates_dashboard_snapshot() {
        let store = create_store().await;
        store
            .record_model_request(ModelRequestRecord {
                session_key: "stdio:model".to_string(),
                provider: "openai".to_string(),
                model: "gpt-4o-mini".to_string(),
                wire_api: "responses".to_string(),
                status: ModelRequestStatus::Success,
                error_code: None,
                duration: Duration::from_millis(500),
                input_tokens: 100,
                output_tokens: 40,
                total_tokens: 140,
                cached_input_tokens: 20,
                reasoning_tokens: 10,
                provider_request_id: None,
                provider_response_id: Some("resp-1".to_string()),
                tool_call_count: 1,
                has_tool_call: true,
                empty_response: false,
            })
            .await
            .expect("llm success should persist");
        store
            .record_model_request(ModelRequestRecord {
                session_key: "stdio:model".to_string(),
                provider: "openai".to_string(),
                model: "gpt-4o-mini".to_string(),
                wire_api: "responses".to_string(),
                status: ModelRequestStatus::Failure,
                error_code: Some("timeout".to_string()),
                duration: Duration::from_millis(900),
                input_tokens: 80,
                output_tokens: 0,
                total_tokens: 80,
                cached_input_tokens: 0,
                reasoning_tokens: 0,
                provider_request_id: None,
                provider_response_id: None,
                tool_call_count: 0,
                has_tool_call: false,
                empty_response: true,
            })
            .await
            .expect("llm failure should persist");
        store
            .record_model_tool_outcome(ModelToolOutcomeRecord {
                session_key: "stdio:model".to_string(),
                provider: "openai".to_string(),
                model: "gpt-4o-mini".to_string(),
                tool_name: "shell".to_string(),
                status: ToolOutcomeStatus::Success,
                error_code: None,
                duration: Duration::from_millis(200),
                approval_required: false,
            })
            .await
            .expect("tool outcome should persist");
        store
            .record_turn_outcome(TurnOutcomeRecord {
                session_key: "stdio:model".to_string(),
                provider: "openai".to_string(),
                model: "gpt-4o-mini".to_string(),
                requests_in_turn: 2,
                tool_iterations: 1,
                completed: true,
                degraded: false,
                token_budget_exceeded: false,
                tool_loop_exhausted: false,
            })
            .await
            .expect("turn outcome should persist");

        let snapshot = store
            .query_model_dashboard_snapshot(&ModelStatsQuery::default())
            .await
            .expect("model snapshot should load");
        assert_eq!(snapshot.summary.total_requests, 2);
        assert_eq!(snapshot.summary.failures, 1);
        assert_eq!(snapshot.model_rows.len(), 1);
        assert_eq!(snapshot.model_rows[0].provider, "openai");
        assert_eq!(snapshot.model_rows[0].model, "gpt-4o-mini");
        assert_eq!(snapshot.error_breakdown[0].error_code, "timeout");
        assert_eq!(snapshot.tool_breakdown[0].tool_name, "shell");
    }
}
