use crate::{
    ApprovalRecord, ApprovalStatus, CronJob, CronScheduleKind, CronTaskRun, CronTaskStatus,
    HeartbeatJob, HeartbeatTaskRun, HeartbeatTaskStatus, LlmAuditRecord, LlmAuditStatus,
    LlmAuditSummaryRecord, LlmUsageRecord, LlmUsageSource, LlmUsageSummary, PendingQuestionRecord,
    PendingQuestionStatus, SessionIndex, StorageError, ToolAuditRecord, ToolAuditStatus,
    WebhookAgentRecord, WebhookEventRecord, WebhookEventStatus,
};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub(crate) struct SessionIndexRow {
    session_key: String,
    chat_id: String,
    channel: String,
    title: Option<String>,
    active_session_key: Option<String>,
    model_provider: Option<String>,
    model_provider_explicit: i64,
    model: Option<String>,
    model_explicit: i64,
    delivery_metadata_json: Option<String>,
    is_active: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    last_message_at_ms: i64,
    turn_count: i64,
    jsonl_path: String,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct CronJobRow {
    id: String,
    name: String,
    schedule_kind: String,
    schedule_expr: String,
    payload_json: String,
    enabled: i64,
    timezone: String,
    next_run_at_ms: i64,
    last_run_at_ms: Option<i64>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct CronTaskRunRow {
    id: String,
    cron_id: String,
    scheduled_at_ms: i64,
    started_at_ms: Option<i64>,
    finished_at_ms: Option<i64>,
    status: String,
    attempt: i64,
    error_message: Option<String>,
    published_message_id: Option<String>,
    created_at_ms: i64,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct HeartbeatJobRow {
    id: String,
    session_key: String,
    channel: String,
    chat_id: String,
    enabled: i64,
    every: String,
    prompt: String,
    silent_ack_token: String,
    recent_messages_limit: i64,
    timezone: String,
    next_run_at_ms: i64,
    last_run_at_ms: Option<i64>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct HeartbeatTaskRunRow {
    id: String,
    heartbeat_id: String,
    scheduled_at_ms: i64,
    started_at_ms: Option<i64>,
    finished_at_ms: Option<i64>,
    status: String,
    attempt: i64,
    error_message: Option<String>,
    published_message_id: Option<String>,
    created_at_ms: i64,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct ApprovalRow {
    id: String,
    session_key: String,
    tool_name: String,
    command_hash: String,
    command_preview: String,
    command_text: String,
    risk_level: String,
    status: String,
    requested_by: String,
    approved_by: Option<String>,
    justification: Option<String>,
    expires_at_ms: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    consumed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct PendingQuestionRow {
    id: String,
    session_key: String,
    channel: String,
    chat_id: String,
    title: Option<String>,
    question_text: String,
    options_json: String,
    status: String,
    selected_option_id: Option<String>,
    answered_by: Option<String>,
    expires_at_ms: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    answered_at_ms: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct LlmUsageRow {
    id: String,
    session_key: String,
    chat_id: String,
    turn_index: i64,
    request_seq: i64,
    provider: String,
    model: String,
    wire_api: String,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    cached_input_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
    source: String,
    provider_request_id: Option<String>,
    provider_response_id: Option<String>,
    created_at_ms: i64,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct LlmUsageSummaryRow {
    request_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    cached_input_tokens: i64,
    reasoning_tokens: i64,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct LlmAuditRow {
    id: String,
    session_key: String,
    chat_id: String,
    turn_index: i64,
    request_seq: i64,
    provider: String,
    model: String,
    wire_api: String,
    status: String,
    error_code: Option<String>,
    error_message: Option<String>,
    provider_request_id: Option<String>,
    provider_response_id: Option<String>,
    request_body_json: String,
    response_body_json: Option<String>,
    metadata_json: Option<String>,
    requested_at_ms: i64,
    responded_at_ms: Option<i64>,
    created_at_ms: i64,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct LlmAuditSummaryRow {
    id: String,
    session_key: String,
    chat_id: String,
    turn_index: i64,
    request_seq: i64,
    provider: String,
    model: String,
    wire_api: String,
    status: String,
    error_code: Option<String>,
    error_message: Option<String>,
    provider_request_id: Option<String>,
    provider_response_id: Option<String>,
    requested_at_ms: i64,
    responded_at_ms: Option<i64>,
    created_at_ms: i64,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct ToolAuditRow {
    id: String,
    session_key: String,
    chat_id: String,
    turn_index: i64,
    request_seq: i64,
    tool_call_seq: i64,
    tool_name: String,
    status: String,
    error_code: Option<String>,
    error_message: Option<String>,
    retryable: Option<i64>,
    approval_required: i64,
    arguments_json: String,
    result_content: String,
    error_details_json: Option<String>,
    signals_json: Option<String>,
    metadata_json: Option<String>,
    started_at_ms: i64,
    finished_at_ms: i64,
    created_at_ms: i64,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct WebhookEventRow {
    id: String,
    source: String,
    event_type: String,
    session_key: String,
    chat_id: String,
    sender_id: String,
    content: String,
    payload_json: Option<String>,
    metadata_json: Option<String>,
    status: String,
    error_message: Option<String>,
    response_summary: Option<String>,
    received_at_ms: i64,
    processed_at_ms: Option<i64>,
    remote_addr: Option<String>,
    created_at_ms: i64,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct WebhookAgentRow {
    id: String,
    hook_id: String,
    session_key: String,
    chat_id: String,
    sender_id: String,
    content: String,
    payload_json: Option<String>,
    metadata_json: Option<String>,
    status: String,
    error_message: Option<String>,
    response_summary: Option<String>,
    received_at_ms: i64,
    processed_at_ms: Option<i64>,
    remote_addr: Option<String>,
    created_at_ms: i64,
}

impl From<SessionIndexRow> for SessionIndex {
    fn from(value: SessionIndexRow) -> Self {
        Self {
            session_key: value.session_key,
            chat_id: value.chat_id,
            channel: value.channel,
            title: value.title,
            active_session_key: value.active_session_key,
            model_provider: value.model_provider,
            model_provider_explicit: value.model_provider_explicit != 0,
            model: value.model,
            model_explicit: value.model_explicit != 0,
            delivery_metadata_json: value.delivery_metadata_json,
            is_active: value.is_active != 0,
            created_at_ms: value.created_at_ms,
            updated_at_ms: value.updated_at_ms,
            last_message_at_ms: value.last_message_at_ms,
            turn_count: value.turn_count,
            jsonl_path: value.jsonl_path,
        }
    }
}

impl TryFrom<CronJobRow> for CronJob {
    type Error = StorageError;

    fn try_from(value: CronJobRow) -> Result<Self, Self::Error> {
        let schedule_kind = CronScheduleKind::parse(&value.schedule_kind).ok_or_else(|| {
            StorageError::backend(format!(
                "invalid cron schedule kind: {}",
                value.schedule_kind
            ))
        })?;
        Ok(Self {
            id: value.id,
            name: value.name,
            schedule_kind,
            schedule_expr: value.schedule_expr,
            payload_json: value.payload_json,
            enabled: value.enabled != 0,
            timezone: value.timezone,
            next_run_at_ms: value.next_run_at_ms,
            last_run_at_ms: value.last_run_at_ms,
            created_at_ms: value.created_at_ms,
            updated_at_ms: value.updated_at_ms,
        })
    }
}

impl TryFrom<CronTaskRunRow> for CronTaskRun {
    type Error = StorageError;

    fn try_from(value: CronTaskRunRow) -> Result<Self, Self::Error> {
        let status = CronTaskStatus::parse(&value.status).ok_or_else(|| {
            StorageError::backend(format!("invalid cron task status: {}", value.status))
        })?;
        Ok(Self {
            id: value.id,
            cron_id: value.cron_id,
            scheduled_at_ms: value.scheduled_at_ms,
            started_at_ms: value.started_at_ms,
            finished_at_ms: value.finished_at_ms,
            status,
            attempt: value.attempt,
            error_message: value.error_message,
            published_message_id: value.published_message_id,
            created_at_ms: value.created_at_ms,
        })
    }
}

impl From<HeartbeatJobRow> for HeartbeatJob {
    fn from(value: HeartbeatJobRow) -> Self {
        Self {
            id: value.id,
            session_key: value.session_key,
            channel: value.channel,
            chat_id: value.chat_id,
            enabled: value.enabled != 0,
            every: value.every,
            prompt: value.prompt,
            silent_ack_token: value.silent_ack_token,
            recent_messages_limit: value.recent_messages_limit,
            timezone: value.timezone,
            next_run_at_ms: value.next_run_at_ms,
            last_run_at_ms: value.last_run_at_ms,
            created_at_ms: value.created_at_ms,
            updated_at_ms: value.updated_at_ms,
        }
    }
}

impl TryFrom<HeartbeatTaskRunRow> for HeartbeatTaskRun {
    type Error = StorageError;

    fn try_from(value: HeartbeatTaskRunRow) -> Result<Self, Self::Error> {
        let status = HeartbeatTaskStatus::parse(&value.status)
            .ok_or_else(|| StorageError::backend("invalid heartbeat task status"))?;
        Ok(Self {
            id: value.id,
            heartbeat_id: value.heartbeat_id,
            scheduled_at_ms: value.scheduled_at_ms,
            started_at_ms: value.started_at_ms,
            finished_at_ms: value.finished_at_ms,
            status,
            attempt: value.attempt,
            error_message: value.error_message,
            published_message_id: value.published_message_id,
            created_at_ms: value.created_at_ms,
        })
    }
}

impl TryFrom<ApprovalRow> for ApprovalRecord {
    type Error = StorageError;

    fn try_from(value: ApprovalRow) -> Result<Self, Self::Error> {
        let status = ApprovalStatus::parse(&value.status).ok_or_else(|| {
            StorageError::backend(format!("invalid approval status: {}", value.status))
        })?;
        Ok(Self {
            id: value.id,
            session_key: value.session_key,
            tool_name: value.tool_name,
            command_hash: value.command_hash,
            command_preview: value.command_preview,
            command_text: value.command_text,
            risk_level: value.risk_level,
            status,
            requested_by: value.requested_by,
            approved_by: value.approved_by,
            justification: value.justification,
            expires_at_ms: value.expires_at_ms,
            created_at_ms: value.created_at_ms,
            updated_at_ms: value.updated_at_ms,
            consumed_at_ms: value.consumed_at_ms,
        })
    }
}

impl TryFrom<PendingQuestionRow> for PendingQuestionRecord {
    type Error = StorageError;

    fn try_from(value: PendingQuestionRow) -> Result<Self, Self::Error> {
        let status = PendingQuestionStatus::parse(&value.status).ok_or_else(|| {
            StorageError::backend(format!("invalid pending question status: {}", value.status))
        })?;
        Ok(Self {
            id: value.id,
            session_key: value.session_key,
            channel: value.channel,
            chat_id: value.chat_id,
            title: value.title,
            question_text: value.question_text,
            options_json: value.options_json,
            status,
            selected_option_id: value.selected_option_id,
            answered_by: value.answered_by,
            expires_at_ms: value.expires_at_ms,
            created_at_ms: value.created_at_ms,
            updated_at_ms: value.updated_at_ms,
            answered_at_ms: value.answered_at_ms,
        })
    }
}

impl TryFrom<LlmUsageRow> for LlmUsageRecord {
    type Error = StorageError;

    fn try_from(value: LlmUsageRow) -> Result<Self, Self::Error> {
        let source = LlmUsageSource::parse(&value.source).ok_or_else(|| {
            StorageError::backend(format!("invalid llm usage source: {}", value.source))
        })?;
        Ok(Self {
            id: value.id,
            session_key: value.session_key,
            chat_id: value.chat_id,
            turn_index: value.turn_index,
            request_seq: value.request_seq,
            provider: value.provider,
            model: value.model,
            wire_api: value.wire_api,
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            total_tokens: value.total_tokens,
            cached_input_tokens: value.cached_input_tokens,
            reasoning_tokens: value.reasoning_tokens,
            source,
            provider_request_id: value.provider_request_id,
            provider_response_id: value.provider_response_id,
            created_at_ms: value.created_at_ms,
        })
    }
}

impl From<LlmUsageSummaryRow> for LlmUsageSummary {
    fn from(value: LlmUsageSummaryRow) -> Self {
        Self {
            request_count: value.request_count,
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            total_tokens: value.total_tokens,
            cached_input_tokens: value.cached_input_tokens,
            reasoning_tokens: value.reasoning_tokens,
        }
    }
}

impl TryFrom<LlmAuditRow> for LlmAuditRecord {
    type Error = StorageError;

    fn try_from(value: LlmAuditRow) -> Result<Self, Self::Error> {
        let status = LlmAuditStatus::parse(&value.status).ok_or_else(|| {
            StorageError::backend(format!("invalid llm audit status: {}", value.status))
        })?;
        Ok(Self {
            id: value.id,
            session_key: value.session_key,
            chat_id: value.chat_id,
            turn_index: value.turn_index,
            request_seq: value.request_seq,
            provider: value.provider,
            model: value.model,
            wire_api: value.wire_api,
            status,
            error_code: value.error_code,
            error_message: value.error_message,
            provider_request_id: value.provider_request_id,
            provider_response_id: value.provider_response_id,
            request_body_json: value.request_body_json,
            response_body_json: value.response_body_json,
            metadata_json: value.metadata_json,
            requested_at_ms: value.requested_at_ms,
            responded_at_ms: value.responded_at_ms,
            created_at_ms: value.created_at_ms,
        })
    }
}

impl TryFrom<LlmAuditSummaryRow> for LlmAuditSummaryRecord {
    type Error = StorageError;

    fn try_from(value: LlmAuditSummaryRow) -> Result<Self, Self::Error> {
        let status = LlmAuditStatus::parse(&value.status).ok_or_else(|| {
            StorageError::backend(format!("invalid llm audit status: {}", value.status))
        })?;
        Ok(Self {
            id: value.id,
            session_key: value.session_key,
            chat_id: value.chat_id,
            turn_index: value.turn_index,
            request_seq: value.request_seq,
            provider: value.provider,
            model: value.model,
            wire_api: value.wire_api,
            status,
            error_code: value.error_code,
            error_message: value.error_message,
            provider_request_id: value.provider_request_id,
            provider_response_id: value.provider_response_id,
            requested_at_ms: value.requested_at_ms,
            responded_at_ms: value.responded_at_ms,
            created_at_ms: value.created_at_ms,
        })
    }
}

impl TryFrom<ToolAuditRow> for ToolAuditRecord {
    type Error = StorageError;

    fn try_from(value: ToolAuditRow) -> Result<Self, Self::Error> {
        let status = ToolAuditStatus::parse(&value.status).ok_or_else(|| {
            StorageError::backend(format!("invalid tool audit status: {}", value.status))
        })?;
        Ok(Self {
            id: value.id,
            session_key: value.session_key,
            chat_id: value.chat_id,
            turn_index: value.turn_index,
            request_seq: value.request_seq,
            tool_call_seq: value.tool_call_seq,
            tool_name: value.tool_name,
            status,
            error_code: value.error_code,
            error_message: value.error_message,
            retryable: value.retryable.map(|flag| flag != 0),
            approval_required: value.approval_required != 0,
            arguments_json: value.arguments_json,
            result_content: value.result_content,
            error_details_json: value.error_details_json,
            signals_json: value.signals_json,
            metadata_json: value.metadata_json,
            started_at_ms: value.started_at_ms,
            finished_at_ms: value.finished_at_ms,
            created_at_ms: value.created_at_ms,
        })
    }
}

impl TryFrom<WebhookEventRow> for WebhookEventRecord {
    type Error = StorageError;

    fn try_from(value: WebhookEventRow) -> Result<Self, Self::Error> {
        let status = WebhookEventStatus::parse(&value.status).ok_or_else(|| {
            StorageError::backend(format!("invalid webhook event status: {}", value.status))
        })?;
        Ok(Self {
            id: value.id,
            source: value.source,
            event_type: value.event_type,
            session_key: value.session_key,
            chat_id: value.chat_id,
            sender_id: value.sender_id,
            content: value.content,
            payload_json: value.payload_json,
            metadata_json: value.metadata_json,
            status,
            error_message: value.error_message,
            response_summary: value.response_summary,
            received_at_ms: value.received_at_ms,
            processed_at_ms: value.processed_at_ms,
            remote_addr: value.remote_addr,
            created_at_ms: value.created_at_ms,
        })
    }
}

impl TryFrom<WebhookAgentRow> for WebhookAgentRecord {
    type Error = StorageError;

    fn try_from(value: WebhookAgentRow) -> Result<Self, Self::Error> {
        let status = WebhookEventStatus::parse(&value.status).ok_or_else(|| {
            StorageError::backend(format!("invalid webhook agent status: {}", value.status))
        })?;
        Ok(Self {
            id: value.id,
            hook_id: value.hook_id,
            session_key: value.session_key,
            chat_id: value.chat_id,
            sender_id: value.sender_id,
            content: value.content,
            payload_json: value.payload_json,
            metadata_json: value.metadata_json,
            status,
            error_message: value.error_message,
            response_summary: value.response_summary,
            received_at_ms: value.received_at_ms,
            processed_at_ms: value.processed_at_ms,
            remote_addr: value.remote_addr,
            created_at_ms: value.created_at_ms,
        })
    }
}
