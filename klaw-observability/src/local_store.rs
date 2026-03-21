use crate::config::LocalStoreConfig;
use async_trait::async_trait;
use klaw_core::observability::ToolOutcomeStatus;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row, SqlitePool,
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
        sqlx::query(
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
        )
        .execute(&self.pool)
        .await
        .map_err(LocalMetricsStoreError::Init)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_tool_metric_events_window
             ON tool_metric_events(occurred_at_unix_ms DESC, tool_name)",
        )
        .execute(&self.pool)
        .await
        .map_err(LocalMetricsStoreError::Init)?;
        sqlx::query(
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
        )
        .execute(&self.pool)
        .await
        .map_err(LocalMetricsStoreError::Init)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_tool_metric_rollups_window
             ON tool_metric_minute_rollups(bucket_minute_unix_ms DESC, tool_name)",
        )
        .execute(&self.pool)
        .await
        .map_err(LocalMetricsStoreError::Init)?;
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
        sqlx::query("DELETE FROM tool_metric_events WHERE occurred_at_unix_ms < ?1")
            .bind(cutoff_unix_ms)
            .execute(&self.pool)
            .await
            .map_err(LocalMetricsStoreError::Query)?;
        sqlx::query("DELETE FROM tool_metric_minute_rollups WHERE bucket_minute_unix_ms < ?1")
            .bind(cutoff_bucket_unix_ms)
            .execute(&self.pool)
            .await
            .map_err(LocalMetricsStoreError::Query)?;
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

fn average(total: u64, count: u64) -> f64 {
    if count == 0 {
        0.0
    } else {
        total as f64 / count as f64
    }
}

fn status_as_str(status: ToolOutcomeStatus) -> &'static str {
    match status {
        ToolOutcomeStatus::Success => "success",
        ToolOutcomeStatus::Failure => "failure",
    }
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
}
