use super::{
    core::TursoSessionStore,
    mapping::{escape_sql_text, row_to_cron_job, row_to_cron_task_run},
};
use crate::{
    CronJob, CronStorage, CronTaskRun, CronTaskStatus, NewCronJob, NewCronTaskRun, StorageError,
    UpdateCronJobPatch, util::now_ms,
};
use async_trait::async_trait;

#[async_trait]
impl CronStorage for TursoSessionStore {
    async fn create_cron(&self, input: &NewCronJob) -> Result<CronJob, StorageError> {
        let now = now_ms();
        let sql = format!(
            "INSERT INTO cron (
                id, name, schedule_kind, schedule_expr, payload_json, enabled, timezone,
                next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
            ) VALUES ('{}', '{}', '{}', '{}', '{}', {}, '{}', {}, NULL, {}, {})",
            escape_sql_text(&input.id),
            escape_sql_text(&input.name),
            input.schedule_kind.as_str(),
            escape_sql_text(&input.schedule_expr),
            escape_sql_text(&input.payload_json),
            if input.enabled { 1 } else { 0 },
            escape_sql_text(&input.timezone),
            input.next_run_at_ms,
            now,
            now
        );
        {
            let conn = self.connection().await?;
            conn.execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
        }
        self.get_cron(&input.id).await
    }

    async fn update_cron(
        &self,
        cron_id: &str,
        patch: &UpdateCronJobPatch,
    ) -> Result<CronJob, StorageError> {
        let current = self.get_cron(cron_id).await?;
        let schedule_kind = patch
            .schedule_kind
            .unwrap_or(current.schedule_kind)
            .as_str();
        let sql = format!(
            "UPDATE cron
             SET name = '{}',
                 schedule_kind = '{}',
                 schedule_expr = '{}',
                 payload_json = '{}',
                 timezone = '{}',
                 next_run_at_ms = {},
                 updated_at_ms = {}
             WHERE id = '{}'",
            escape_sql_text(patch.name.as_deref().unwrap_or(&current.name)),
            schedule_kind,
            escape_sql_text(
                patch
                    .schedule_expr
                    .as_deref()
                    .unwrap_or(&current.schedule_expr)
            ),
            escape_sql_text(
                patch
                    .payload_json
                    .as_deref()
                    .unwrap_or(&current.payload_json)
            ),
            escape_sql_text(patch.timezone.as_deref().unwrap_or(&current.timezone)),
            patch.next_run_at_ms.unwrap_or(current.next_run_at_ms),
            now_ms(),
            escape_sql_text(cron_id)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                return Err(StorageError::backend(format!(
                    "cron job '{cron_id}' not found when updating"
                )));
            }
        }
        self.get_cron(cron_id).await
    }

    async fn set_enabled(&self, cron_id: &str, enabled: bool) -> Result<(), StorageError> {
        let sql = format!(
            "UPDATE cron
             SET enabled = {}, updated_at_ms = {}
             WHERE id = '{}'",
            if enabled { 1 } else { 0 },
            now_ms(),
            escape_sql_text(cron_id)
        );
        let conn = self.connection().await?;
        let affected = conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        if affected == 0 {
            return Err(StorageError::backend(format!(
                "cron job '{cron_id}' not found when setting enabled"
            )));
        }
        Ok(())
    }

    async fn delete_cron(&self, cron_id: &str) -> Result<(), StorageError> {
        let sql = format!("DELETE FROM cron WHERE id = '{}'", escape_sql_text(cron_id));
        let conn = self.connection().await?;
        let affected = conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        if affected == 0 {
            return Err(StorageError::backend(format!(
                "cron job '{cron_id}' not found when deleting"
            )));
        }
        Ok(())
    }

    async fn get_cron(&self, cron_id: &str) -> Result<CronJob, StorageError> {
        let sql = format!(
            "SELECT id, name, schedule_kind, schedule_expr, payload_json, enabled, timezone,
                    next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM cron
             WHERE id = '{}'
             LIMIT 1",
            escape_sql_text(cron_id)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("cron job not found"))?;
        row_to_cron_job(&row)
    }

    async fn list_crons(&self, limit: i64, offset: i64) -> Result<Vec<CronJob>, StorageError> {
        let sql = format!(
            "SELECT id, name, schedule_kind, schedule_expr, payload_json, enabled, timezone,
                    next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM cron
             ORDER BY updated_at_ms DESC
             LIMIT {}
             OFFSET {}",
            limit.max(1),
            offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_cron_job(&row)?);
        }
        Ok(out)
    }

    async fn list_due_crons(&self, now_ms: i64, limit: i64) -> Result<Vec<CronJob>, StorageError> {
        let sql = format!(
            "SELECT id, name, schedule_kind, schedule_expr, payload_json, enabled, timezone,
                    next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM cron
             WHERE enabled = 1 AND next_run_at_ms <= {}
             ORDER BY next_run_at_ms ASC
             LIMIT {}",
            now_ms,
            limit.max(1)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_cron_job(&row)?);
        }
        Ok(out)
    }

    async fn claim_next_run(
        &self,
        cron_id: &str,
        expected_next_run_at_ms: i64,
        new_next_run_at_ms: i64,
        now_ms: i64,
    ) -> Result<bool, StorageError> {
        let sql = format!(
            "UPDATE cron
             SET next_run_at_ms = {},
                 last_run_at_ms = {},
                 updated_at_ms = {}
             WHERE id = '{}' AND enabled = 1 AND next_run_at_ms = {}",
            new_next_run_at_ms,
            expected_next_run_at_ms,
            now_ms,
            escape_sql_text(cron_id),
            expected_next_run_at_ms
        );
        let conn = self.connection().await?;
        let affected = conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        Ok(affected == 1)
    }

    async fn append_task_run(&self, input: &NewCronTaskRun) -> Result<CronTaskRun, StorageError> {
        let sql = format!(
            "INSERT INTO cron_task (
                id, cron_id, scheduled_at_ms, started_at_ms, finished_at_ms,
                status, attempt, error_message, published_message_id, created_at_ms
            ) VALUES ('{}', '{}', {}, NULL, NULL, '{}', {}, NULL, NULL, {})",
            escape_sql_text(&input.id),
            escape_sql_text(&input.cron_id),
            input.scheduled_at_ms,
            input.status.as_str(),
            input.attempt,
            input.created_at_ms
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        let mut rows = conn
            .query(
                &format!(
                    "SELECT id, cron_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                            attempt, error_message, published_message_id, created_at_ms
                     FROM cron_task
                     WHERE id = '{}'
                     LIMIT 1",
                    escape_sql_text(&input.id)
                ),
                (),
            )
            .await
            .map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("cron task not found"))?;
        row_to_cron_task_run(&row)
    }

    async fn mark_task_running(
        &self,
        run_id: &str,
        started_at_ms: i64,
    ) -> Result<(), StorageError> {
        let sql = format!(
            "UPDATE cron_task
             SET status = '{}', started_at_ms = {}
             WHERE id = '{}'",
            CronTaskStatus::Running.as_str(),
            started_at_ms,
            escape_sql_text(run_id)
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn mark_task_result(
        &self,
        run_id: &str,
        status: CronTaskStatus,
        finished_at_ms: i64,
        error_message: Option<&str>,
        published_message_id: Option<&str>,
    ) -> Result<(), StorageError> {
        let error_sql = error_message
            .map(|value| format!("'{}'", escape_sql_text(value)))
            .unwrap_or_else(|| "NULL".to_string());
        let publish_sql = published_message_id
            .map(|value| format!("'{}'", escape_sql_text(value)))
            .unwrap_or_else(|| "NULL".to_string());
        let sql = format!(
            "UPDATE cron_task
             SET status = '{}',
                 finished_at_ms = {},
                 error_message = {},
                 published_message_id = {}
             WHERE id = '{}'",
            status.as_str(),
            finished_at_ms,
            error_sql,
            publish_sql,
            escape_sql_text(run_id)
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn list_task_runs(
        &self,
        cron_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<CronTaskRun>, StorageError> {
        let sql = format!(
            "SELECT id, cron_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                    attempt, error_message, published_message_id, created_at_ms
             FROM cron_task
             WHERE cron_id = '{}'
             ORDER BY created_at_ms DESC
             LIMIT {} OFFSET {}",
            escape_sql_text(cron_id),
            limit.max(1),
            offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_cron_task_run(&row)?);
        }
        Ok(out)
    }
}
