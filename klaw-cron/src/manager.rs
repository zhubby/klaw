use crate::{time::now_ms, CronError, ScheduleSpec};
use klaw_storage::{
    open_default_store, CronJob, CronScheduleKind, CronStorage, CronTaskRun, DbRow, DbValue,
    DefaultSessionStore, MemoryDb, NewCronJob, UpdateCronJobPatch,
};

#[derive(Debug, Clone, Copy)]
pub struct CronListQuery {
    pub limit: i64,
    pub offset: i64,
}

impl Default for CronListQuery {
    fn default() -> Self {
        Self {
            limit: 200,
            offset: 0,
        }
    }
}

pub struct SqliteCronManager {
    store: DefaultSessionStore,
}

impl SqliteCronManager {
    pub async fn open_default() -> Result<Self, CronError> {
        let store = open_default_store().await?;
        Ok(Self { store })
    }

    pub fn from_store(store: DefaultSessionStore) -> Self {
        Self { store }
    }

    pub async fn list_jobs(&self, query: CronListQuery) -> Result<Vec<CronJob>, CronError> {
        let limit = query.limit.max(1);
        let offset = query.offset.max(0);
        let sql = format!(
            "SELECT id, name, schedule_kind, schedule_expr, payload_json, enabled, timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms \
             FROM cron \
             ORDER BY updated_at_ms DESC \
             LIMIT {limit} OFFSET {offset}"
        );
        let rows = self.store.query(&sql, &[]).await?;
        rows.into_iter().map(row_to_cron_job).collect()
    }

    pub async fn list_runs(
        &self,
        cron_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<CronTaskRun>, CronError> {
        Ok(self.store.list_task_runs(cron_id, limit, offset).await?)
    }

    pub async fn create_job(&self, input: &NewCronJob) -> Result<CronJob, CronError> {
        Ok(self.store.create_cron(input).await?)
    }

    pub async fn update_job(
        &self,
        cron_id: &str,
        patch: &UpdateCronJobPatch,
    ) -> Result<CronJob, CronError> {
        Ok(self.store.update_cron(cron_id, patch).await?)
    }

    pub async fn set_enabled(&self, cron_id: &str, enabled: bool) -> Result<(), CronError> {
        Ok(self.store.set_enabled(cron_id, enabled).await?)
    }

    pub async fn delete_job(&self, cron_id: &str) -> Result<(), CronError> {
        Ok(self.store.delete_cron(cron_id).await?)
    }

    pub fn compute_next_run_at_ms(kind: CronScheduleKind, expr: &str) -> Result<i64, CronError> {
        let schedule = ScheduleSpec::from_kind_expr(kind, expr)?;
        schedule.next_run_after_ms(now_ms())
    }
}

fn row_to_cron_job(row: DbRow) -> Result<CronJob, CronError> {
    let schedule_kind_raw = row_text(&row, 2)?;
    let schedule_kind = CronScheduleKind::parse(&schedule_kind_raw).ok_or_else(|| {
        CronError::InvalidCronRow(format!("invalid schedule kind: {schedule_kind_raw}"))
    })?;

    Ok(CronJob {
        id: row_text(&row, 0)?,
        name: row_text(&row, 1)?,
        schedule_kind,
        schedule_expr: row_text(&row, 3)?,
        payload_json: row_text(&row, 4)?,
        enabled: row_i64(&row, 5)? != 0,
        timezone: row_text(&row, 6)?,
        next_run_at_ms: row_i64(&row, 7)?,
        last_run_at_ms: row_opt_i64(&row, 8)?,
        created_at_ms: row_i64(&row, 9)?,
        updated_at_ms: row_i64(&row, 10)?,
    })
}

fn row_text(row: &DbRow, index: usize) -> Result<String, CronError> {
    match row.get(index) {
        Some(DbValue::Text(v)) => Ok(v.clone()),
        Some(DbValue::Integer(v)) => Ok(v.to_string()),
        Some(DbValue::Null) | None => Err(CronError::InvalidCronRow(format!(
            "missing text at column {index}"
        ))),
        Some(other) => Err(CronError::InvalidCronRow(format!(
            "unexpected value at column {index}: {other:?}"
        ))),
    }
}

fn row_i64(row: &DbRow, index: usize) -> Result<i64, CronError> {
    match row.get(index) {
        Some(DbValue::Integer(v)) => Ok(*v),
        Some(DbValue::Text(v)) => v.parse::<i64>().map_err(|_| {
            CronError::InvalidCronRow(format!("invalid integer text at column {index}"))
        }),
        Some(DbValue::Null) | None => Err(CronError::InvalidCronRow(format!(
            "missing integer at column {index}"
        ))),
        Some(other) => Err(CronError::InvalidCronRow(format!(
            "unexpected value at column {index}: {other:?}"
        ))),
    }
}

fn row_opt_i64(row: &DbRow, index: usize) -> Result<Option<i64>, CronError> {
    match row.get(index) {
        Some(DbValue::Null) | None => Ok(None),
        Some(DbValue::Integer(v)) => Ok(Some(*v)),
        Some(DbValue::Text(v)) => v.parse::<i64>().map(Some).map_err(|_| {
            CronError::InvalidCronRow(format!("invalid integer text at column {index}"))
        }),
        Some(other) => Err(CronError::InvalidCronRow(format!(
            "unexpected value at column {index}: {other:?}"
        ))),
    }
}
