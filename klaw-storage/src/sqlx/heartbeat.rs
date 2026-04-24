use super::{
    core::SqlxSessionStore,
    rows::{HeartbeatJobRow, HeartbeatTaskRunRow},
};
use crate::{
    HeartbeatJob, HeartbeatStorage, HeartbeatTaskRun, HeartbeatTaskStatus, NewHeartbeatJob,
    NewHeartbeatTaskRun, StorageError, UpdateHeartbeatJobPatch, util::now_ms,
};
use async_trait::async_trait;

#[async_trait]
impl HeartbeatStorage for SqlxSessionStore {
    async fn create_heartbeat(
        &self,
        input: &NewHeartbeatJob,
    ) -> Result<HeartbeatJob, StorageError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO heartbeat (
                id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                recent_messages_limit, timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, ?12, ?13)",
        )
        .bind(&input.id)
        .bind(&input.session_key)
        .bind(&input.channel)
        .bind(&input.chat_id)
        .bind(if input.enabled { 1_i64 } else { 0_i64 })
        .bind(&input.every)
        .bind(&input.prompt)
        .bind(&input.silent_ack_token)
        .bind(input.recent_messages_limit)
        .bind(&input.timezone)
        .bind(input.next_run_at_ms)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.get_heartbeat(&input.id).await
    }

    async fn update_heartbeat(
        &self,
        heartbeat_id: &str,
        patch: &UpdateHeartbeatJobPatch,
    ) -> Result<HeartbeatJob, StorageError> {
        let current = self.get_heartbeat(heartbeat_id).await?;
        sqlx::query(
            "UPDATE heartbeat
             SET session_key = ?1,
                 channel = ?2,
                 chat_id = ?3,
                 every = ?4,
                 prompt = ?5,
                 silent_ack_token = ?6,
                 recent_messages_limit = ?7,
                 timezone = ?8,
                 next_run_at_ms = ?9,
                 updated_at_ms = ?10
             WHERE id = ?11",
        )
        .bind(patch.session_key.as_ref().unwrap_or(&current.session_key))
        .bind(patch.channel.as_ref().unwrap_or(&current.channel))
        .bind(patch.chat_id.as_ref().unwrap_or(&current.chat_id))
        .bind(patch.every.as_ref().unwrap_or(&current.every))
        .bind(patch.prompt.as_ref().unwrap_or(&current.prompt))
        .bind(
            patch
                .silent_ack_token
                .as_ref()
                .unwrap_or(&current.silent_ack_token),
        )
        .bind(
            patch
                .recent_messages_limit
                .unwrap_or(current.recent_messages_limit),
        )
        .bind(patch.timezone.as_ref().unwrap_or(&current.timezone))
        .bind(patch.next_run_at_ms.unwrap_or(current.next_run_at_ms))
        .bind(now_ms())
        .bind(heartbeat_id)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.get_heartbeat(heartbeat_id).await
    }

    async fn set_heartbeat_enabled(
        &self,
        heartbeat_id: &str,
        enabled: bool,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE heartbeat
             SET enabled = ?1, updated_at_ms = ?2
             WHERE id = ?3",
        )
        .bind(if enabled { 1_i64 } else { 0_i64 })
        .bind(now_ms())
        .bind(heartbeat_id)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn delete_heartbeat(&self, heartbeat_id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM heartbeat WHERE id = ?1")
            .bind(heartbeat_id)
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn get_heartbeat(&self, heartbeat_id: &str) -> Result<HeartbeatJob, StorageError> {
        let row = sqlx::query_as::<_, HeartbeatJobRow>(
            "SELECT id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                    recent_messages_limit, timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM heartbeat
             WHERE id = ?1",
        )
        .bind(heartbeat_id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(row.into())
    }

    async fn get_heartbeat_by_session_key(
        &self,
        session_key: &str,
    ) -> Result<HeartbeatJob, StorageError> {
        let row = sqlx::query_as::<_, HeartbeatJobRow>(
            "SELECT id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                    recent_messages_limit, timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM heartbeat
             WHERE session_key = ?1",
        )
        .bind(session_key)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(row.into())
    }

    async fn list_heartbeats(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<HeartbeatJob>, StorageError> {
        let rows = sqlx::query_as::<_, HeartbeatJobRow>(
            "SELECT id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                    recent_messages_limit, timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM heartbeat
             ORDER BY updated_at_ms DESC
             LIMIT ?1 OFFSET ?2",
        )
        .bind(limit.max(1))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_due_heartbeats(
        &self,
        now_ms: i64,
        limit: i64,
    ) -> Result<Vec<HeartbeatJob>, StorageError> {
        let rows = sqlx::query_as::<_, HeartbeatJobRow>(
            "SELECT id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                    recent_messages_limit, timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM heartbeat
             WHERE enabled = 1 AND next_run_at_ms <= ?1
             ORDER BY next_run_at_ms ASC
             LIMIT ?2",
        )
        .bind(now_ms)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn claim_next_heartbeat_run(
        &self,
        heartbeat_id: &str,
        expected_next_run_at_ms: i64,
        new_next_run_at_ms: i64,
        now_ms: i64,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            "UPDATE heartbeat
             SET next_run_at_ms = ?1,
                 last_run_at_ms = ?2,
                 updated_at_ms = ?3
             WHERE id = ?4 AND enabled = 1 AND next_run_at_ms = ?5",
        )
        .bind(new_next_run_at_ms)
        .bind(expected_next_run_at_ms)
        .bind(now_ms)
        .bind(heartbeat_id)
        .bind(expected_next_run_at_ms)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(result.rows_affected() == 1)
    }

    async fn append_heartbeat_task_run(
        &self,
        input: &NewHeartbeatTaskRun,
    ) -> Result<HeartbeatTaskRun, StorageError> {
        sqlx::query(
            "INSERT INTO heartbeat_task (
                id, heartbeat_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                attempt, error_message, published_message_id, created_at_ms
            ) VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?5, NULL, NULL, ?6)",
        )
        .bind(&input.id)
        .bind(&input.heartbeat_id)
        .bind(input.scheduled_at_ms)
        .bind(input.status.as_str())
        .bind(input.attempt)
        .bind(input.created_at_ms)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;

        let row = sqlx::query_as::<_, HeartbeatTaskRunRow>(
            "SELECT id, heartbeat_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                    attempt, error_message, published_message_id, created_at_ms
             FROM heartbeat_task
             WHERE id = ?1",
        )
        .bind(&input.id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        row.try_into()
    }

    async fn mark_heartbeat_task_running(
        &self,
        run_id: &str,
        started_at_ms: i64,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE heartbeat_task
             SET status = ?1, started_at_ms = ?2
             WHERE id = ?3",
        )
        .bind(HeartbeatTaskStatus::Running.as_str())
        .bind(started_at_ms)
        .bind(run_id)
        .execute(&self.pool)
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
        sqlx::query(
            "UPDATE heartbeat_task
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

    async fn list_heartbeat_task_runs(
        &self,
        heartbeat_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<HeartbeatTaskRun>, StorageError> {
        let rows = sqlx::query_as::<_, HeartbeatTaskRunRow>(
            "SELECT id, heartbeat_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                    attempt, error_message, published_message_id, created_at_ms
             FROM heartbeat_task
             WHERE heartbeat_id = ?1
             ORDER BY created_at_ms DESC
             LIMIT ?2 OFFSET ?3",
        )
        .bind(heartbeat_id)
        .bind(limit.max(1))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }
}
