use crate::{
    ApprovalRecord, ApprovalStatus, ChatRecord, CronJob, CronTaskRun, CronTaskStatus, HeartbeatJob,
    HeartbeatTaskRun, HeartbeatTaskStatus, LlmAuditFilterOptions, LlmAuditFilterOptionsQuery,
    LlmAuditQuery, LlmAuditRecord, LlmAuditSummaryRecord, LlmUsageRecord, LlmUsageSummary,
    NewApprovalRecord, NewCronJob, NewCronTaskRun, NewHeartbeatJob, NewHeartbeatTaskRun,
    NewLlmAuditRecord, NewLlmUsageRecord, NewPendingQuestionRecord, NewToolAuditRecord,
    NewWebhookAgentRecord, NewWebhookEventRecord, PendingQuestionRecord, PendingQuestionStatus,
    SessionCompressionState, SessionIndex, SessionSortOrder, StorageError, ToolAuditFilterOptions,
    ToolAuditFilterOptionsQuery, ToolAuditQuery, ToolAuditRecord, UpdateCronJobPatch,
    UpdateHeartbeatJobPatch, UpdateWebhookAgentResult, UpdateWebhookEventResult, WebhookAgentQuery,
    WebhookAgentRecord, WebhookEventQuery, WebhookEventRecord,
};
use async_trait::async_trait;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ChatRecordPage {
    pub records: Vec<ChatRecord>,
    pub has_more: bool,
    pub oldest_message_id: Option<String>,
}

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

    async fn read_chat_records_page(
        &self,
        session_key: &str,
        before_message_id: Option<&str>,
        limit: usize,
    ) -> Result<ChatRecordPage, StorageError>;

    async fn get_session(&self, session_key: &str) -> Result<SessionIndex, StorageError>;

    async fn set_session_title(
        &self,
        session_key: &str,
        title: Option<&str>,
    ) -> Result<SessionIndex, StorageError>;

    async fn delete_session(&self, session_key: &str) -> Result<bool, StorageError>;

    async fn get_session_by_active_session_key(
        &self,
        active_session_key: &str,
    ) -> Result<SessionIndex, StorageError>;

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

    async fn set_delivery_metadata(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        delivery_metadata_json: Option<&str>,
    ) -> Result<SessionIndex, StorageError>;

    async fn clear_model_routing_override(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, StorageError>;

    async fn get_session_compression_state(
        &self,
        session_key: &str,
    ) -> Result<Option<SessionCompressionState>, StorageError>;

    async fn set_session_compression_state(
        &self,
        session_key: &str,
        state: &SessionCompressionState,
    ) -> Result<(), StorageError>;

    async fn list_sessions(
        &self,
        limit: Option<i64>,
        offset: i64,
        updated_from_ms: Option<i64>,
        updated_to_ms: Option<i64>,
        channel: Option<&str>,
        session_key_prefix: Option<&str>,
        sort_order: SessionSortOrder,
    ) -> Result<Vec<SessionIndex>, StorageError>;

    async fn list_session_channels(&self) -> Result<Vec<String>, StorageError>;

    async fn append_llm_usage(
        &self,
        input: &NewLlmUsageRecord,
    ) -> Result<LlmUsageRecord, StorageError>;

    async fn list_llm_usage(
        &self,
        session_key: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<LlmUsageRecord>, StorageError>;

    async fn sum_llm_usage_by_session(
        &self,
        session_key: &str,
    ) -> Result<LlmUsageSummary, StorageError>;

    async fn sum_llm_usage_by_turn(
        &self,
        session_key: &str,
        turn_index: i64,
    ) -> Result<LlmUsageSummary, StorageError>;

    async fn append_llm_audit(
        &self,
        input: &NewLlmAuditRecord,
    ) -> Result<LlmAuditRecord, StorageError>;

    async fn list_llm_audit(
        &self,
        query: &LlmAuditQuery,
    ) -> Result<Vec<LlmAuditRecord>, StorageError>;

    async fn get_llm_audit(&self, audit_id: &str) -> Result<LlmAuditRecord, StorageError>;

    async fn list_llm_audit_summaries(
        &self,
        query: &LlmAuditQuery,
    ) -> Result<Vec<LlmAuditSummaryRecord>, StorageError>;

    async fn list_llm_audit_filter_options(
        &self,
        query: &LlmAuditFilterOptionsQuery,
    ) -> Result<LlmAuditFilterOptions, StorageError>;

    async fn append_tool_audit(
        &self,
        input: &NewToolAuditRecord,
    ) -> Result<ToolAuditRecord, StorageError>;

    async fn list_tool_audit(
        &self,
        query: &ToolAuditQuery,
    ) -> Result<Vec<ToolAuditRecord>, StorageError>;

    async fn list_tool_audit_filter_options(
        &self,
        query: &ToolAuditFilterOptionsQuery,
    ) -> Result<ToolAuditFilterOptions, StorageError>;

    async fn append_webhook_event(
        &self,
        input: &NewWebhookEventRecord,
    ) -> Result<WebhookEventRecord, StorageError>;

    async fn update_webhook_event_status(
        &self,
        event_id: &str,
        update: &UpdateWebhookEventResult,
    ) -> Result<WebhookEventRecord, StorageError>;

    async fn list_webhook_events(
        &self,
        query: &WebhookEventQuery,
    ) -> Result<Vec<WebhookEventRecord>, StorageError>;

    async fn append_webhook_agent(
        &self,
        input: &NewWebhookAgentRecord,
    ) -> Result<WebhookAgentRecord, StorageError>;

    async fn update_webhook_agent_status(
        &self,
        event_id: &str,
        update: &UpdateWebhookAgentResult,
    ) -> Result<WebhookAgentRecord, StorageError>;

    async fn list_webhook_agents(
        &self,
        query: &WebhookAgentQuery,
    ) -> Result<Vec<WebhookAgentRecord>, StorageError>;

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

    async fn consume_approved_tool_command(
        &self,
        approval_id: &str,
        tool_name: &str,
        session_key: &str,
        command_hash: &str,
        now_ms: i64,
    ) -> Result<bool, StorageError>;

    async fn consume_latest_approved_tool_command(
        &self,
        tool_name: &str,
        session_key: &str,
        command_hash: &str,
        now_ms: i64,
    ) -> Result<bool, StorageError>;

    async fn consume_approved_shell_command(
        &self,
        approval_id: &str,
        session_key: &str,
        command_hash: &str,
        now_ms: i64,
    ) -> Result<bool, StorageError> {
        self.consume_approved_tool_command(approval_id, "shell", session_key, command_hash, now_ms)
            .await
    }

    async fn consume_latest_approved_shell_command(
        &self,
        session_key: &str,
        command_hash: &str,
        now_ms: i64,
    ) -> Result<bool, StorageError> {
        self.consume_latest_approved_tool_command("shell", session_key, command_hash, now_ms)
            .await
    }

    async fn create_pending_question(
        &self,
        input: &NewPendingQuestionRecord,
    ) -> Result<PendingQuestionRecord, StorageError>;

    async fn get_pending_question(
        &self,
        question_id: &str,
    ) -> Result<PendingQuestionRecord, StorageError>;

    async fn update_pending_question_answer(
        &self,
        question_id: &str,
        status: PendingQuestionStatus,
        selected_option_id: Option<&str>,
        answered_by: Option<&str>,
        answered_at_ms: Option<i64>,
    ) -> Result<PendingQuestionRecord, StorageError>;

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

    async fn list_crons(&self, limit: i64, offset: i64) -> Result<Vec<CronJob>, StorageError>;

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

#[async_trait]
pub trait HeartbeatStorage: Send + Sync {
    async fn create_heartbeat(&self, input: &NewHeartbeatJob)
    -> Result<HeartbeatJob, StorageError>;

    async fn update_heartbeat(
        &self,
        heartbeat_id: &str,
        patch: &UpdateHeartbeatJobPatch,
    ) -> Result<HeartbeatJob, StorageError>;

    async fn set_heartbeat_enabled(
        &self,
        heartbeat_id: &str,
        enabled: bool,
    ) -> Result<(), StorageError>;

    async fn delete_heartbeat(&self, heartbeat_id: &str) -> Result<(), StorageError>;

    async fn get_heartbeat(&self, heartbeat_id: &str) -> Result<HeartbeatJob, StorageError>;

    async fn get_heartbeat_by_session_key(
        &self,
        session_key: &str,
    ) -> Result<HeartbeatJob, StorageError>;

    async fn list_heartbeats(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<HeartbeatJob>, StorageError>;

    async fn list_due_heartbeats(
        &self,
        now_ms: i64,
        limit: i64,
    ) -> Result<Vec<HeartbeatJob>, StorageError>;

    async fn claim_next_heartbeat_run(
        &self,
        heartbeat_id: &str,
        expected_next_run_at_ms: i64,
        new_next_run_at_ms: i64,
        now_ms: i64,
    ) -> Result<bool, StorageError>;

    async fn append_heartbeat_task_run(
        &self,
        input: &NewHeartbeatTaskRun,
    ) -> Result<HeartbeatTaskRun, StorageError>;

    async fn mark_heartbeat_task_running(
        &self,
        run_id: &str,
        started_at_ms: i64,
    ) -> Result<(), StorageError>;

    async fn mark_heartbeat_task_result(
        &self,
        run_id: &str,
        status: HeartbeatTaskStatus,
        finished_at_ms: i64,
        error_message: Option<&str>,
        published_message_id: Option<&str>,
    ) -> Result<(), StorageError>;

    async fn list_heartbeat_task_runs(
        &self,
        heartbeat_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<HeartbeatTaskRun>, StorageError>;
}
