use super::{
    core::TursoSessionStore,
    mapping::{escape_sql_text, row_to_heartbeat_job, row_to_heartbeat_task_run},
};
use crate::{
    HeartbeatJob, HeartbeatStorage, HeartbeatTaskRun, HeartbeatTaskStatus, NewHeartbeatJob,
    NewHeartbeatTaskRun, StorageError, UpdateHeartbeatJobPatch, util::now_ms,
};
use async_trait::async_trait;

#[async_trait]
impl HeartbeatStorage for TursoSessionStore {
    async fn create_heartbeat(
        &self,
        input: &NewHeartbeatJob,
    ) -> Result<HeartbeatJob, StorageError> {
        let now = now_ms();
        let sql = format!(
            "INSERT INTO heartbeat (
                id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                recent_messages_limit, timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
            ) VALUES ('{}', '{}', '{}', '{}', {}, '{}', '{}', '{}', {}, '{}', {}, NULL, {}, {})",
            escape_sql_text(&input.id),
            escape_sql_text(&input.session_key),
            escape_sql_text(&input.channel),
            escape_sql_text(&input.chat_id),
            if input.enabled { 1 } else { 0 },
            escape_sql_text(&input.every),
            escape_sql_text(&input.prompt),
            escape_sql_text(&input.silent_ack_token),
            input.recent_messages_limit,
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
        self.get_heartbeat(&input.id).await
    }

    async fn update_heartbeat(
        &self,
        heartbeat_id: &str,
        patch: &UpdateHeartbeatJobPatch,
    ) -> Result<HeartbeatJob, StorageError> {
        let current = self.get_heartbeat(heartbeat_id).await?;
        let sql = format!(
            "UPDATE heartbeat
             SET session_key = '{}',
                 channel = '{}',
                 chat_id = '{}',
                 every = '{}',
                 prompt = '{}',
                 silent_ack_token = '{}',
                 recent_messages_limit = {},
                 timezone = '{}',
                 next_run_at_ms = {},
                 updated_at_ms = {}
             WHERE id = '{}'",
            escape_sql_text(patch.session_key.as_deref().unwrap_or(&current.session_key)),
            escape_sql_text(patch.channel.as_deref().unwrap_or(&current.channel)),
            escape_sql_text(patch.chat_id.as_deref().unwrap_or(&current.chat_id)),
            escape_sql_text(patch.every.as_deref().unwrap_or(&current.every)),
            escape_sql_text(patch.prompt.as_deref().unwrap_or(&current.prompt)),
            escape_sql_text(
                patch
                    .silent_ack_token
                    .as_deref()
                    .unwrap_or(&current.silent_ack_token)
            ),
            patch
                .recent_messages_limit
                .unwrap_or(current.recent_messages_limit),
            escape_sql_text(patch.timezone.as_deref().unwrap_or(&current.timezone)),
            patch.next_run_at_ms.unwrap_or(current.next_run_at_ms),
            now_ms(),
            escape_sql_text(heartbeat_id)
        );
        {
            let conn = self.connection().await?;
            conn.execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
        }
        self.get_heartbeat(heartbeat_id).await
    }

    async fn set_heartbeat_enabled(
        &self,
        heartbeat_id: &str,
        enabled: bool,
    ) -> Result<(), StorageError> {
        let sql = format!(
            "UPDATE heartbeat
             SET enabled = {}, updated_at_ms = {}
             WHERE id = '{}'",
            if enabled { 1 } else { 0 },
            now_ms(),
            escape_sql_text(heartbeat_id)
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn delete_heartbeat(&self, heartbeat_id: &str) -> Result<(), StorageError> {
        let sql = format!(
            "DELETE FROM heartbeat WHERE id = '{}'",
            escape_sql_text(heartbeat_id)
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn get_heartbeat(&self, heartbeat_id: &str) -> Result<HeartbeatJob, StorageError> {
        let sql = format!(
            "SELECT id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                    recent_messages_limit, timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM heartbeat
             WHERE id = '{}'
             LIMIT 1",
            escape_sql_text(heartbeat_id)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("heartbeat job not found"))?;
        row_to_heartbeat_job(&row)
    }

    async fn get_heartbeat_by_session_key(
        &self,
        session_key: &str,
    ) -> Result<HeartbeatJob, StorageError> {
        let sql = format!(
            "SELECT id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                    recent_messages_limit, timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM heartbeat
             WHERE session_key = '{}'
             LIMIT 1",
            escape_sql_text(session_key)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("heartbeat job not found"))?;
        row_to_heartbeat_job(&row)
    }

    async fn list_heartbeats(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<HeartbeatJob>, StorageError> {
        let sql = format!(
            "SELECT id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                    recent_messages_limit, timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM heartbeat
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
            out.push(row_to_heartbeat_job(&row)?);
        }
        Ok(out)
    }

    async fn list_due_heartbeats(
        &self,
        now_ms: i64,
        limit: i64,
    ) -> Result<Vec<HeartbeatJob>, StorageError> {
        let sql = format!(
            "SELECT id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                    recent_messages_limit, timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM heartbeat
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
            out.push(row_to_heartbeat_job(&row)?);
        }
        Ok(out)
    }

    async fn claim_next_heartbeat_run(
        &self,
        heartbeat_id: &str,
        expected_next_run_at_ms: i64,
        new_next_run_at_ms: i64,
        now_ms: i64,
    ) -> Result<bool, StorageError> {
        let sql = format!(
            "UPDATE heartbeat
             SET next_run_at_ms = {},
                 last_run_at_ms = {},
                 updated_at_ms = {}
             WHERE id = '{}' AND enabled = 1 AND next_run_at_ms = {}",
            new_next_run_at_ms,
            expected_next_run_at_ms,
            now_ms,
            escape_sql_text(heartbeat_id),
            expected_next_run_at_ms
        );
        let conn = self.connection().await?;
        let affected = conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        Ok(affected == 1)
    }

    async fn append_heartbeat_task_run(
        &self,
        input: &NewHeartbeatTaskRun,
    ) -> Result<HeartbeatTaskRun, StorageError> {
        let sql = format!(
            "INSERT INTO heartbeat_task (
                id, heartbeat_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                attempt, error_message, published_message_id, created_at_ms
            ) VALUES ('{}', '{}', {}, NULL, NULL, '{}', {}, NULL, NULL, {})",
            escape_sql_text(&input.id),
            escape_sql_text(&input.heartbeat_id),
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
                    "SELECT id, heartbeat_id, scheduled_at_ms, started_at_ms, finished_at_ms,
                            status, attempt, error_message, published_message_id, created_at_ms
                     FROM heartbeat_task
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
            .ok_or_else(|| StorageError::backend("heartbeat task not found"))?;
        row_to_heartbeat_task_run(&row)
    }

    async fn mark_heartbeat_task_running(
        &self,
        run_id: &str,
        started_at_ms: i64,
    ) -> Result<(), StorageError> {
        let sql = format!(
            "UPDATE heartbeat_task
             SET status = '{}', started_at_ms = {}
             WHERE id = '{}'",
            HeartbeatTaskStatus::Running.as_str(),
            started_at_ms,
            escape_sql_text(run_id)
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn mark_heartbeat_task_result(
        &self,
        run_id: &str,
        status: HeartbeatTaskStatus,
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
            "UPDATE heartbeat_task
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

    async fn list_heartbeat_task_runs(
        &self,
        heartbeat_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<HeartbeatTaskRun>, StorageError> {
        let sql = format!(
            "SELECT id, heartbeat_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                    attempt, error_message, published_message_id, created_at_ms
             FROM heartbeat_task
             WHERE heartbeat_id = '{}'
             ORDER BY created_at_ms DESC
             LIMIT {} OFFSET {}",
            escape_sql_text(heartbeat_id),
            limit.max(1),
            offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_heartbeat_task_run(&row)?);
        }
        Ok(out)
    }
}
