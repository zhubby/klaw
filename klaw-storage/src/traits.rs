use crate::{
    ApprovalRecord, ApprovalStatus, ChatRecord, CronJob, CronTaskRun, CronTaskStatus,
    NewApprovalRecord, NewCronJob, NewCronTaskRun, SessionIndex, StorageError, UpdateCronJobPatch,
};
use async_trait::async_trait;
use std::path::PathBuf;

#[async_trait]
pub trait SessionStorage: Send + Sync {
    async fn touch_session(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, StorageError>;

    async fn complete_turn(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, StorageError>;

    async fn append_chat_record(
        &self,
        session_key: &str,
        record: &ChatRecord,
    ) -> Result<(), StorageError>;

    async fn read_chat_records(&self, session_key: &str) -> Result<Vec<ChatRecord>, StorageError>;

    async fn get_session(&self, session_key: &str) -> Result<SessionIndex, StorageError>;

    async fn get_or_create_session_state(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        default_provider: &str,
        default_model: &str,
    ) -> Result<SessionIndex, StorageError>;

    async fn set_active_session(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        active_session_key: &str,
    ) -> Result<SessionIndex, StorageError>;

    async fn set_model_provider(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        model_provider: &str,
        model: &str,
    ) -> Result<SessionIndex, StorageError>;

    async fn set_model(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        model: &str,
    ) -> Result<SessionIndex, StorageError>;

    async fn list_sessions(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SessionIndex>, StorageError>;

    async fn create_approval(
        &self,
        input: &NewApprovalRecord,
    ) -> Result<ApprovalRecord, StorageError>;

    async fn get_approval(&self, approval_id: &str) -> Result<ApprovalRecord, StorageError>;

    async fn update_approval_status(
        &self,
        approval_id: &str,
        status: ApprovalStatus,
        approved_by: Option<&str>,
    ) -> Result<ApprovalRecord, StorageError>;

    async fn consume_approved_shell_command(
        &self,
        approval_id: &str,
        session_key: &str,
        command_hash: &str,
        now_ms: i64,
    ) -> Result<bool, StorageError>;

    async fn consume_latest_approved_shell_command(
        &self,
        session_key: &str,
        command_hash: &str,
        now_ms: i64,
    ) -> Result<bool, StorageError>;

    fn session_jsonl_path(&self, session_key: &str) -> PathBuf;
}

#[async_trait]
pub trait CronStorage: Send + Sync {
    async fn create_cron(&self, input: &NewCronJob) -> Result<CronJob, StorageError>;

    async fn update_cron(
        &self,
        cron_id: &str,
        patch: &UpdateCronJobPatch,
    ) -> Result<CronJob, StorageError>;

    async fn set_enabled(&self, cron_id: &str, enabled: bool) -> Result<(), StorageError>;

    async fn delete_cron(&self, cron_id: &str) -> Result<(), StorageError>;

    async fn get_cron(&self, cron_id: &str) -> Result<CronJob, StorageError>;

    async fn list_due_crons(&self, now_ms: i64, limit: i64) -> Result<Vec<CronJob>, StorageError>;

    async fn claim_next_run(
        &self,
        cron_id: &str,
        expected_next_run_at_ms: i64,
        new_next_run_at_ms: i64,
        now_ms: i64,
    ) -> Result<bool, StorageError>;

    async fn append_task_run(&self, input: &NewCronTaskRun) -> Result<CronTaskRun, StorageError>;

    async fn mark_task_running(&self, run_id: &str, started_at_ms: i64)
        -> Result<(), StorageError>;

    async fn mark_task_result(
        &self,
        run_id: &str,
        status: CronTaskStatus,
        finished_at_ms: i64,
        error_message: Option<&str>,
        published_message_id: Option<&str>,
    ) -> Result<(), StorageError>;

    async fn list_task_runs(
        &self,
        cron_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<CronTaskRun>, StorageError>;
}
