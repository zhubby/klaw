use super::{
    core::SqlxSessionStore,
    rows::{CronJobRow, CronTaskRunRow},
};
use crate::{
    CronJob, CronStorage, CronTaskRun, CronTaskStatus, NewCronJob, NewCronTaskRun, StorageError,
    UpdateCronJobPatch, util::now_ms,
};
use async_trait::async_trait;

#[async_trait]
impl CronStorage for SqlxSessionStore {
    async fn create_cron(&self, input: &NewCronJob) -> Result<CronJob, StorageError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO cron (
                id, name, schedule_kind, schedule_expr, payload_json, enabled, timezone,
                next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, ?9, ?10)",
        )
        .bind(&input.id)
        .bind(&input.name)
        .bind(input.schedule_kind.as_str())
        .bind(&input.schedule_expr)
        .bind(&input.payload_json)
        .bind(if input.enabled { 1_i64 } else { 0_i64 })
        .bind(&input.timezone)
        .bind(input.next_run_at_ms)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.get_cron(&input.id).await
    }

    async fn update_cron(
        &self,
        cron_id: &str,
        patch: &UpdateCronJobPatch,
    ) -> Result<CronJob, StorageError> {
        let current = self.get_cron(cron_id).await?;
        let now = now_ms();
        let updated = sqlx::query(
            "UPDATE cron
             SET name = ?1,
                 schedule_kind = ?2,
                 schedule_expr = ?3,
                 payload_json = ?4,
                 timezone = ?5,
                 next_run_at_ms = ?6,
                 updated_at_ms = ?7
             WHERE id = ?8",
        )
        .bind(patch.name.as_ref().unwrap_or(&current.name))
        .bind(
            patch
                .schedule_kind
                .unwrap_or(current.schedule_kind)
                .as_str(),
        )
        .bind(
            patch
                .schedule_expr
                .as_ref()
                .unwrap_or(&current.schedule_expr),
        )
        .bind(patch.payload_json.as_ref().unwrap_or(&current.payload_json))
        .bind(patch.timezone.as_ref().unwrap_or(&current.timezone))
        .bind(patch.next_run_at_ms.unwrap_or(current.next_run_at_ms))
        .bind(now)
        .bind(cron_id)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        if updated.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "cron job '{cron_id}' not found when updating"
            )));
        }
        self.get_cron(cron_id).await
    }

    async fn set_enabled(&self, cron_id: &str, enabled: bool) -> Result<(), StorageError> {
        let updated = sqlx::query("UPDATE cron SET enabled = ?1, updated_at_ms = ?2 WHERE id = ?3")
            .bind(if enabled { 1_i64 } else { 0_i64 })
            .bind(now_ms())
            .bind(cron_id)
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        if updated.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "cron job '{cron_id}' not found when setting enabled"
            )));
        }
        Ok(())
    }

    async fn delete_cron(&self, cron_id: &str) -> Result<(), StorageError> {
        let deleted = sqlx::query("DELETE FROM cron WHERE id = ?1")
            .bind(cron_id)
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        if deleted.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "cron job '{cron_id}' not found when deleting"
            )));
        }
        Ok(())
    }

    async fn get_cron(&self, cron_id: &str) -> Result<CronJob, StorageError> {
        let row = sqlx::query_as::<_, CronJobRow>(
            "SELECT id, name, schedule_kind, schedule_expr, payload_json, enabled, timezone,
                    next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM cron
             WHERE id = ?1",
        )
        .bind(cron_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        let Some(row) = row else {
            return Err(StorageError::backend(format!(
                "cron job not found: {cron_id}"
            )));
        };
        row.try_into()
    }

    async fn list_crons(&self, limit: i64, offset: i64) -> Result<Vec<CronJob>, StorageError> {
        let rows = sqlx::query_as::<_, CronJobRow>(
            "SELECT id, name, schedule_kind, schedule_expr, payload_json, enabled, timezone,
                    next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM cron
             ORDER BY updated_at_ms DESC
             LIMIT ?1 OFFSET ?2",
        )
        .bind(limit.max(1))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn list_due_crons(&self, now_ms: i64, limit: i64) -> Result<Vec<CronJob>, StorageError> {
        let rows = sqlx::query_as::<_, CronJobRow>(
            "SELECT id, name, schedule_kind, schedule_expr, payload_json, enabled, timezone,
                    next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM cron
             WHERE enabled = 1 AND next_run_at_ms <= ?1
             ORDER BY next_run_at_ms ASC
             LIMIT ?2",
        )
        .bind(now_ms)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn claim_next_run(
        &self,
        cron_id: &str,
        expected_next_run_at_ms: i64,
        new_next_run_at_ms: i64,
        now_ms: i64,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            "UPDATE cron
             SET next_run_at_ms = ?1,
                 last_run_at_ms = ?2,
                 updated_at_ms = ?3
             WHERE id = ?4 AND enabled = 1 AND next_run_at_ms = ?5",
        )
        .bind(new_next_run_at_ms)
        .bind(expected_next_run_at_ms)
        .bind(now_ms)
        .bind(cron_id)
        .bind(expected_next_run_at_ms)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(result.rows_affected() == 1)
    }

    async fn append_task_run(&self, input: &NewCronTaskRun) -> Result<CronTaskRun, StorageError> {
        sqlx::query(
            "INSERT INTO cron_task (
                id, cron_id, scheduled_at_ms, started_at_ms, finished_at_ms,
                status, attempt, error_message, published_message_id, created_at_ms
            ) VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?5, NULL, NULL, ?6)",
        )
        .bind(&input.id)
        .bind(&input.cron_id)
        .bind(input.scheduled_at_ms)
        .bind(input.status.as_str())
        .bind(input.attempt)
        .bind(input.created_at_ms)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;

        let row = sqlx::query_as::<_, CronTaskRunRow>(
            "SELECT id, cron_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                    attempt, error_message, published_message_id, created_at_ms
             FROM cron_task
             WHERE id = ?1",
        )
        .bind(&input.id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        row.try_into()
    }

    async fn mark_task_running(
        &self,
        run_id: &str,
        started_at_ms: i64,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE cron_task
             SET status = ?1, started_at_ms = ?2
             WHERE id = ?3",
        )
        .bind(CronTaskStatus::Running.as_str())
        .bind(started_at_ms)
        .bind(run_id)
        .execute(&self.pool)
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
        sqlx::query(
            "UPDATE cron_task
             SET status = ?1,
                 finished_at_ms = ?2,
                 error_message = ?3,
                 published_message_id = ?4
             WHERE id = ?5",
        )
        .bind(status.as_str())
        .bind(finished_at_ms)
        .bind(error_message)
        .bind(published_message_id)
        .bind(run_id)
        .execute(&self.pool)
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
        let rows = sqlx::query_as::<_, CronTaskRunRow>(
            "SELECT id, cron_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                    attempt, error_message, published_message_id, created_at_ms
             FROM cron_task
             WHERE cron_id = ?1
             ORDER BY created_at_ms DESC
             LIMIT ?2 OFFSET ?3",
        )
        .bind(cron_id)
        .bind(limit.max(1))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }
}
