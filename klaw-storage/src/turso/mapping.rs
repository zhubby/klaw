use crate::{
    ApprovalRecord, ApprovalStatus, CronJob, CronScheduleKind, CronTaskRun, CronTaskStatus,
    HeartbeatJob, HeartbeatTaskRun, HeartbeatTaskStatus, LlmAuditRecord, LlmAuditStatus,
    LlmAuditSummaryRecord, LlmUsageRecord, LlmUsageSource, LlmUsageSummary, PendingQuestionRecord,
    PendingQuestionStatus, SessionIndex, StorageError, ToolAuditRecord, ToolAuditStatus,
    WebhookAgentRecord, WebhookEventRecord, WebhookEventStatus, database_executor::DbValue,
};
use turso::{Connection, Row, value::Value};

pub(crate) fn escape_sql_text(input: &str) -> String {
    input.replace('\'', "''")
}

pub(crate) fn opt_string_sql(value: Option<&str>) -> String {
    value
        .map(|inner| format!("'{}'", escape_sql_text(inner)))
        .unwrap_or_else(|| "NULL".to_string())
}

pub(crate) fn opt_i64_sql(value: Option<i64>) -> String {
    value
        .map(|inner| inner.to_string())
        .unwrap_or_else(|| "NULL".to_string())
}

pub(crate) fn to_turso_params(values: &[DbValue]) -> Vec<Value> {
    values
        .iter()
        .map(|value| match value {
            DbValue::Null => Value::Null,
            DbValue::Integer(v) => Value::Integer(*v),
            DbValue::Real(v) => Value::Real(*v),
            DbValue::Text(v) => Value::Text(v.clone()),
            DbValue::Blob(v) => Value::Blob(v.clone()),
        })
        .collect()
}

pub(crate) fn from_turso_value(value: Value) -> DbValue {
    match value {
        Value::Null => DbValue::Null,
        Value::Integer(v) => DbValue::Integer(v),
        Value::Real(v) => DbValue::Real(v),
        Value::Text(v) => DbValue::Text(v),
        Value::Blob(v) => DbValue::Blob(v),
    }
}

pub(crate) fn row_to_session_index(row: &Row) -> Result<SessionIndex, StorageError> {
    Ok(SessionIndex {
        session_key: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        chat_id: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        channel: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        title: value_to_opt_string(row.get_value(3).map_err(StorageError::backend)?),
        active_session_key: value_to_opt_string(row.get_value(4).map_err(StorageError::backend)?),
        model_provider: value_to_opt_string(row.get_value(5).map_err(StorageError::backend)?),
        model_provider_explicit: value_to_i64(row.get_value(6).map_err(StorageError::backend)?)?
            != 0,
        model: value_to_opt_string(row.get_value(7).map_err(StorageError::backend)?),
        model_explicit: value_to_i64(row.get_value(8).map_err(StorageError::backend)?)? != 0,
        delivery_metadata_json: value_to_opt_string(
            row.get_value(9).map_err(StorageError::backend)?,
        ),
        is_active: value_to_i64(row.get_value(10).map_err(StorageError::backend)?)? != 0,
        created_at_ms: value_to_i64(row.get_value(11).map_err(StorageError::backend)?)?,
        updated_at_ms: value_to_i64(row.get_value(12).map_err(StorageError::backend)?)?,
        last_message_at_ms: value_to_i64(row.get_value(13).map_err(StorageError::backend)?)?,
        turn_count: value_to_i64(row.get_value(14).map_err(StorageError::backend)?)?,
        jsonl_path: value_to_string(row.get_value(15).map_err(StorageError::backend)?)?,
    })
}

pub(crate) fn row_to_cron_job(row: &Row) -> Result<CronJob, StorageError> {
    let kind_raw = value_to_string(row.get_value(2).map_err(StorageError::backend)?)?;
    let schedule_kind = CronScheduleKind::parse(&kind_raw)
        .ok_or_else(|| StorageError::backend(format!("invalid cron schedule kind: {kind_raw}")))?;
    Ok(CronJob {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        name: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        schedule_kind,
        schedule_expr: value_to_string(row.get_value(3).map_err(StorageError::backend)?)?,
        payload_json: value_to_string(row.get_value(4).map_err(StorageError::backend)?)?,
        enabled: value_to_i64(row.get_value(5).map_err(StorageError::backend)?)? != 0,
        timezone: value_to_string(row.get_value(6).map_err(StorageError::backend)?)?,
        next_run_at_ms: value_to_i64(row.get_value(7).map_err(StorageError::backend)?)?,
        last_run_at_ms: value_to_opt_i64(row.get_value(8).map_err(StorageError::backend)?)?,
        created_at_ms: value_to_i64(row.get_value(9).map_err(StorageError::backend)?)?,
        updated_at_ms: value_to_i64(row.get_value(10).map_err(StorageError::backend)?)?,
    })
}

pub(crate) fn row_to_cron_task_run(row: &Row) -> Result<CronTaskRun, StorageError> {
    let status_raw = value_to_string(row.get_value(5).map_err(StorageError::backend)?)?;
    let status = CronTaskStatus::parse(&status_raw)
        .ok_or_else(|| StorageError::backend(format!("invalid cron task status: {status_raw}")))?;
    Ok(CronTaskRun {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        cron_id: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        scheduled_at_ms: value_to_i64(row.get_value(2).map_err(StorageError::backend)?)?,
        started_at_ms: value_to_opt_i64(row.get_value(3).map_err(StorageError::backend)?)?,
        finished_at_ms: value_to_opt_i64(row.get_value(4).map_err(StorageError::backend)?)?,
        status,
        attempt: value_to_i64(row.get_value(6).map_err(StorageError::backend)?)?,
        error_message: value_to_opt_string(row.get_value(7).map_err(StorageError::backend)?),
        published_message_id: value_to_opt_string(row.get_value(8).map_err(StorageError::backend)?),
        created_at_ms: value_to_i64(row.get_value(9).map_err(StorageError::backend)?)?,
    })
}

pub(crate) fn row_to_heartbeat_job(row: &Row) -> Result<HeartbeatJob, StorageError> {
    Ok(HeartbeatJob {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        session_key: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        channel: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        chat_id: value_to_string(row.get_value(3).map_err(StorageError::backend)?)?,
        enabled: value_to_i64(row.get_value(4).map_err(StorageError::backend)?)? != 0,
        every: value_to_string(row.get_value(5).map_err(StorageError::backend)?)?,
        prompt: value_to_string(row.get_value(6).map_err(StorageError::backend)?)?,
        silent_ack_token: value_to_string(row.get_value(7).map_err(StorageError::backend)?)?,
        recent_messages_limit: value_to_i64(row.get_value(8).map_err(StorageError::backend)?)?,
        timezone: value_to_string(row.get_value(9).map_err(StorageError::backend)?)?,
        next_run_at_ms: value_to_i64(row.get_value(10).map_err(StorageError::backend)?)?,
        last_run_at_ms: value_to_opt_i64(row.get_value(11).map_err(StorageError::backend)?)?,
        created_at_ms: value_to_i64(row.get_value(12).map_err(StorageError::backend)?)?,
        updated_at_ms: value_to_i64(row.get_value(13).map_err(StorageError::backend)?)?,
    })
}

pub(crate) fn row_to_heartbeat_task_run(row: &Row) -> Result<HeartbeatTaskRun, StorageError> {
    let status_raw = value_to_string(row.get_value(5).map_err(StorageError::backend)?)?;
    let status = HeartbeatTaskStatus::parse(&status_raw).ok_or_else(|| {
        StorageError::backend(format!("invalid heartbeat task status: {status_raw}"))
    })?;
    Ok(HeartbeatTaskRun {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        heartbeat_id: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        scheduled_at_ms: value_to_i64(row.get_value(2).map_err(StorageError::backend)?)?,
        started_at_ms: value_to_opt_i64(row.get_value(3).map_err(StorageError::backend)?)?,
        finished_at_ms: value_to_opt_i64(row.get_value(4).map_err(StorageError::backend)?)?,
        status,
        attempt: value_to_i64(row.get_value(6).map_err(StorageError::backend)?)?,
        error_message: value_to_opt_string(row.get_value(7).map_err(StorageError::backend)?),
        published_message_id: value_to_opt_string(row.get_value(8).map_err(StorageError::backend)?),
        created_at_ms: value_to_i64(row.get_value(9).map_err(StorageError::backend)?)?,
    })
}

pub(crate) fn row_to_approval(row: &Row) -> Result<ApprovalRecord, StorageError> {
    let status_raw = value_to_string(row.get_value(6).map_err(StorageError::backend)?)?;
    let status = ApprovalStatus::parse(&status_raw)
        .ok_or_else(|| StorageError::backend(format!("invalid approval status: {status_raw}")))?;
    Ok(ApprovalRecord {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        session_key: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        tool_name: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        command_hash: value_to_string(row.get_value(3).map_err(StorageError::backend)?)?,
        command_preview: value_to_string(row.get_value(4).map_err(StorageError::backend)?)?,
        command_text: value_to_string(row.get_value(7).map_err(StorageError::backend)?)?,
        risk_level: value_to_string(row.get_value(5).map_err(StorageError::backend)?)?,
        status,
        requested_by: value_to_string(row.get_value(8).map_err(StorageError::backend)?)?,
        approved_by: value_to_opt_string(row.get_value(9).map_err(StorageError::backend)?),
        justification: value_to_opt_string(row.get_value(10).map_err(StorageError::backend)?),
        expires_at_ms: value_to_i64(row.get_value(11).map_err(StorageError::backend)?)?,
        created_at_ms: value_to_i64(row.get_value(12).map_err(StorageError::backend)?)?,
        updated_at_ms: value_to_i64(row.get_value(13).map_err(StorageError::backend)?)?,
        consumed_at_ms: value_to_opt_i64(row.get_value(14).map_err(StorageError::backend)?)?,
    })
}

pub(crate) fn row_to_pending_question(row: &Row) -> Result<PendingQuestionRecord, StorageError> {
    let status_raw = value_to_string(row.get_value(7).map_err(StorageError::backend)?)?;
    let status = PendingQuestionStatus::parse(&status_raw).ok_or_else(|| {
        StorageError::backend(format!("invalid pending question status: {status_raw}"))
    })?;
    Ok(PendingQuestionRecord {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        session_key: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        channel: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        chat_id: value_to_string(row.get_value(3).map_err(StorageError::backend)?)?,
        title: value_to_opt_string(row.get_value(4).map_err(StorageError::backend)?),
        question_text: value_to_string(row.get_value(5).map_err(StorageError::backend)?)?,
        options_json: value_to_string(row.get_value(6).map_err(StorageError::backend)?)?,
        status,
        selected_option_id: value_to_opt_string(row.get_value(8).map_err(StorageError::backend)?),
        answered_by: value_to_opt_string(row.get_value(9).map_err(StorageError::backend)?),
        expires_at_ms: value_to_i64(row.get_value(10).map_err(StorageError::backend)?)?,
        created_at_ms: value_to_i64(row.get_value(11).map_err(StorageError::backend)?)?,
        updated_at_ms: value_to_i64(row.get_value(12).map_err(StorageError::backend)?)?,
        answered_at_ms: value_to_opt_i64(row.get_value(13).map_err(StorageError::backend)?)?,
    })
}

pub(crate) fn row_to_llm_usage(row: &Row) -> Result<LlmUsageRecord, StorageError> {
    let source_raw = value_to_string(row.get_value(13).map_err(StorageError::backend)?)?;
    let source = LlmUsageSource::parse(&source_raw)
        .ok_or_else(|| StorageError::backend(format!("invalid llm usage source: {source_raw}")))?;
    Ok(LlmUsageRecord {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        session_key: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        chat_id: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        turn_index: value_to_i64(row.get_value(3).map_err(StorageError::backend)?)?,
        request_seq: value_to_i64(row.get_value(4).map_err(StorageError::backend)?)?,
        provider: value_to_string(row.get_value(5).map_err(StorageError::backend)?)?,
        model: value_to_string(row.get_value(6).map_err(StorageError::backend)?)?,
        wire_api: value_to_string(row.get_value(7).map_err(StorageError::backend)?)?,
        input_tokens: value_to_i64(row.get_value(8).map_err(StorageError::backend)?)?,
        output_tokens: value_to_i64(row.get_value(9).map_err(StorageError::backend)?)?,
        total_tokens: value_to_i64(row.get_value(10).map_err(StorageError::backend)?)?,
        cached_input_tokens: value_to_opt_i64(row.get_value(11).map_err(StorageError::backend)?)?,
        reasoning_tokens: value_to_opt_i64(row.get_value(12).map_err(StorageError::backend)?)?,
        source,
        provider_request_id: value_to_opt_string(row.get_value(14).map_err(StorageError::backend)?),
        provider_response_id: value_to_opt_string(
            row.get_value(15).map_err(StorageError::backend)?,
        ),
        created_at_ms: value_to_i64(row.get_value(16).map_err(StorageError::backend)?)?,
    })
}

pub(crate) fn row_to_llm_usage_summary(row: &Row) -> Result<LlmUsageSummary, StorageError> {
    Ok(LlmUsageSummary {
        request_count: value_to_i64(row.get_value(0).map_err(StorageError::backend)?)?,
        input_tokens: value_to_i64(row.get_value(1).map_err(StorageError::backend)?)?,
        output_tokens: value_to_i64(row.get_value(2).map_err(StorageError::backend)?)?,
        total_tokens: value_to_i64(row.get_value(3).map_err(StorageError::backend)?)?,
        cached_input_tokens: value_to_i64(row.get_value(4).map_err(StorageError::backend)?)?,
        reasoning_tokens: value_to_i64(row.get_value(5).map_err(StorageError::backend)?)?,
    })
}

pub(crate) fn row_to_llm_audit(row: &Row) -> Result<LlmAuditRecord, StorageError> {
    let status_raw = value_to_string(row.get_value(8).map_err(StorageError::backend)?)?;
    let status = LlmAuditStatus::parse(&status_raw)
        .ok_or_else(|| StorageError::backend(format!("invalid llm audit status: {status_raw}")))?;
    Ok(LlmAuditRecord {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        session_key: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        chat_id: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        turn_index: value_to_i64(row.get_value(3).map_err(StorageError::backend)?)?,
        request_seq: value_to_i64(row.get_value(4).map_err(StorageError::backend)?)?,
        provider: value_to_string(row.get_value(5).map_err(StorageError::backend)?)?,
        model: value_to_string(row.get_value(6).map_err(StorageError::backend)?)?,
        wire_api: value_to_string(row.get_value(7).map_err(StorageError::backend)?)?,
        status,
        error_code: value_to_opt_string(row.get_value(9).map_err(StorageError::backend)?),
        error_message: value_to_opt_string(row.get_value(10).map_err(StorageError::backend)?),
        provider_request_id: value_to_opt_string(row.get_value(11).map_err(StorageError::backend)?),
        provider_response_id: value_to_opt_string(
            row.get_value(12).map_err(StorageError::backend)?,
        ),
        request_body_json: value_to_string(row.get_value(13).map_err(StorageError::backend)?)?,
        response_body_json: value_to_opt_string(row.get_value(14).map_err(StorageError::backend)?),
        metadata_json: value_to_opt_string(row.get_value(15).map_err(StorageError::backend)?),
        requested_at_ms: value_to_i64(row.get_value(16).map_err(StorageError::backend)?)?,
        responded_at_ms: value_to_opt_i64(row.get_value(17).map_err(StorageError::backend)?)?,
        created_at_ms: value_to_i64(row.get_value(18).map_err(StorageError::backend)?)?,
    })
}

pub(crate) fn row_to_llm_audit_summary(row: &Row) -> Result<LlmAuditSummaryRecord, StorageError> {
    let status_raw = value_to_string(row.get_value(8).map_err(StorageError::backend)?)?;
    let status = LlmAuditStatus::parse(&status_raw)
        .ok_or_else(|| StorageError::backend(format!("invalid llm audit status: {status_raw}")))?;
    Ok(LlmAuditSummaryRecord {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        session_key: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        chat_id: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        turn_index: value_to_i64(row.get_value(3).map_err(StorageError::backend)?)?,
        request_seq: value_to_i64(row.get_value(4).map_err(StorageError::backend)?)?,
        provider: value_to_string(row.get_value(5).map_err(StorageError::backend)?)?,
        model: value_to_string(row.get_value(6).map_err(StorageError::backend)?)?,
        wire_api: value_to_string(row.get_value(7).map_err(StorageError::backend)?)?,
        status,
        error_code: value_to_opt_string(row.get_value(9).map_err(StorageError::backend)?),
        error_message: value_to_opt_string(row.get_value(10).map_err(StorageError::backend)?),
        provider_request_id: value_to_opt_string(row.get_value(11).map_err(StorageError::backend)?),
        provider_response_id: value_to_opt_string(
            row.get_value(12).map_err(StorageError::backend)?,
        ),
        requested_at_ms: value_to_i64(row.get_value(13).map_err(StorageError::backend)?)?,
        responded_at_ms: value_to_opt_i64(row.get_value(14).map_err(StorageError::backend)?)?,
        created_at_ms: value_to_i64(row.get_value(15).map_err(StorageError::backend)?)?,
    })
}

pub(crate) fn row_to_tool_audit(row: &Row) -> Result<ToolAuditRecord, StorageError> {
    let status_raw = value_to_string(row.get_value(7).map_err(StorageError::backend)?)?;
    let status = ToolAuditStatus::parse(&status_raw)
        .ok_or_else(|| StorageError::backend(format!("invalid tool audit status: {status_raw}")))?;
    Ok(ToolAuditRecord {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        session_key: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        chat_id: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        turn_index: value_to_i64(row.get_value(3).map_err(StorageError::backend)?)?,
        request_seq: value_to_i64(row.get_value(4).map_err(StorageError::backend)?)?,
        tool_call_seq: value_to_i64(row.get_value(5).map_err(StorageError::backend)?)?,
        tool_name: value_to_string(row.get_value(6).map_err(StorageError::backend)?)?,
        status,
        error_code: value_to_opt_string(row.get_value(8).map_err(StorageError::backend)?),
        error_message: value_to_opt_string(row.get_value(9).map_err(StorageError::backend)?),
        retryable: value_to_opt_i64(row.get_value(10).map_err(StorageError::backend)?)?
            .map(|value| value != 0),
        approval_required: value_to_i64(row.get_value(11).map_err(StorageError::backend)?)? != 0,
        arguments_json: value_to_string(row.get_value(12).map_err(StorageError::backend)?)?,
        result_content: value_to_string(row.get_value(13).map_err(StorageError::backend)?)?,
        error_details_json: value_to_opt_string(row.get_value(14).map_err(StorageError::backend)?),
        signals_json: value_to_opt_string(row.get_value(15).map_err(StorageError::backend)?),
        metadata_json: value_to_opt_string(row.get_value(16).map_err(StorageError::backend)?),
        started_at_ms: value_to_i64(row.get_value(17).map_err(StorageError::backend)?)?,
        finished_at_ms: value_to_i64(row.get_value(18).map_err(StorageError::backend)?)?,
        created_at_ms: value_to_i64(row.get_value(19).map_err(StorageError::backend)?)?,
    })
}

pub(crate) fn row_to_webhook_event(row: &Row) -> Result<WebhookEventRecord, StorageError> {
    let status_raw = value_to_string(row.get_value(9).map_err(StorageError::backend)?)?;
    let status = WebhookEventStatus::parse(&status_raw).ok_or_else(|| {
        StorageError::backend(format!("invalid webhook event status: {status_raw}"))
    })?;
    Ok(WebhookEventRecord {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        source: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        event_type: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        session_key: value_to_string(row.get_value(3).map_err(StorageError::backend)?)?,
        chat_id: value_to_string(row.get_value(4).map_err(StorageError::backend)?)?,
        sender_id: value_to_string(row.get_value(5).map_err(StorageError::backend)?)?,
        content: value_to_string(row.get_value(6).map_err(StorageError::backend)?)?,
        payload_json: value_to_opt_string(row.get_value(7).map_err(StorageError::backend)?),
        metadata_json: value_to_opt_string(row.get_value(8).map_err(StorageError::backend)?),
        status,
        error_message: value_to_opt_string(row.get_value(10).map_err(StorageError::backend)?),
        response_summary: value_to_opt_string(row.get_value(11).map_err(StorageError::backend)?),
        received_at_ms: value_to_i64(row.get_value(12).map_err(StorageError::backend)?)?,
        processed_at_ms: value_to_opt_i64(row.get_value(13).map_err(StorageError::backend)?)?,
        remote_addr: value_to_opt_string(row.get_value(14).map_err(StorageError::backend)?),
        created_at_ms: value_to_i64(row.get_value(15).map_err(StorageError::backend)?)?,
    })
}

pub(crate) fn row_to_webhook_agent(row: &Row) -> Result<WebhookAgentRecord, StorageError> {
    let status_raw = value_to_string(row.get_value(8).map_err(StorageError::backend)?)?;
    let status = WebhookEventStatus::parse(&status_raw).ok_or_else(|| {
        StorageError::backend(format!("invalid webhook agent status: {status_raw}"))
    })?;
    Ok(WebhookAgentRecord {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        hook_id: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        session_key: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        chat_id: value_to_string(row.get_value(3).map_err(StorageError::backend)?)?,
        sender_id: value_to_string(row.get_value(4).map_err(StorageError::backend)?)?,
        content: value_to_string(row.get_value(5).map_err(StorageError::backend)?)?,
        payload_json: value_to_opt_string(row.get_value(6).map_err(StorageError::backend)?),
        metadata_json: value_to_opt_string(row.get_value(7).map_err(StorageError::backend)?),
        status,
        error_message: value_to_opt_string(row.get_value(9).map_err(StorageError::backend)?),
        response_summary: value_to_opt_string(row.get_value(10).map_err(StorageError::backend)?),
        received_at_ms: value_to_i64(row.get_value(11).map_err(StorageError::backend)?)?,
        processed_at_ms: value_to_opt_i64(row.get_value(12).map_err(StorageError::backend)?)?,
        remote_addr: value_to_opt_string(row.get_value(13).map_err(StorageError::backend)?),
        created_at_ms: value_to_i64(row.get_value(14).map_err(StorageError::backend)?)?,
    })
}

pub(crate) fn value_to_string(value: Value) -> Result<String, StorageError> {
    match value {
        Value::Text(v) => Ok(v),
        Value::Integer(v) => Ok(v.to_string()),
        Value::Real(v) => Ok(v.to_string()),
        Value::Null => Ok(String::new()),
        Value::Blob(_) => Err(StorageError::backend("unexpected blob value")),
    }
}

pub(crate) fn value_to_i64(value: Value) -> Result<i64, StorageError> {
    match value {
        Value::Integer(v) => Ok(v),
        Value::Text(v) => v
            .parse::<i64>()
            .map_err(|err| StorageError::backend(format!("invalid integer text: {err}"))),
        Value::Real(v) => Ok(v as i64),
        Value::Null => Ok(0),
        Value::Blob(_) => Err(StorageError::backend("unexpected blob value")),
    }
}

pub(crate) fn value_to_opt_i64(value: Value) -> Result<Option<i64>, StorageError> {
    match value {
        Value::Null => Ok(None),
        other => value_to_i64(other).map(Some),
    }
}

pub(crate) fn llm_audit_requested_range_where(
    requested_from_ms: Option<i64>,
    requested_to_ms: Option<i64>,
) -> String {
    let mut conditions = Vec::new();
    if let Some(from_ms) = requested_from_ms {
        conditions.push(format!("requested_at_ms >= {from_ms}"));
    }
    if let Some(to_ms) = requested_to_ms {
        conditions.push(format!("requested_at_ms <= {to_ms}"));
    }
    if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    }
}

pub(crate) fn tool_audit_started_range_where(
    started_from_ms: Option<i64>,
    started_to_ms: Option<i64>,
) -> String {
    let mut conditions = Vec::new();
    if let Some(from_ms) = started_from_ms {
        conditions.push(format!("started_at_ms >= {from_ms}"));
    }
    if let Some(to_ms) = started_to_ms {
        conditions.push(format!("started_at_ms <= {to_ms}"));
    }
    if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    }
}

pub(crate) async fn collect_string_column(
    conn: &Connection,
    sql: &str,
) -> Result<Vec<String>, StorageError> {
    let mut rows = conn.query(sql, ()).await.map_err(StorageError::backend)?;
    let mut values = Vec::new();
    while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
        values.push(value_to_string(
            row.get_value(0).map_err(StorageError::backend)?,
        )?);
    }
    Ok(values)
}

pub(crate) fn opt_sql_text(value: Option<&str>) -> String {
    value
        .map(|value| format!("'{}'", escape_sql_text(value)))
        .unwrap_or_else(|| "NULL".to_string())
}

pub(crate) fn opt_sql_i64(value: Option<i64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "NULL".to_string())
}

pub(crate) fn value_to_opt_string(value: Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Text(v) => Some(v),
        Value::Integer(v) => Some(v.to_string()),
        Value::Real(v) => Some(v.to_string()),
        Value::Blob(_) => None,
    }
}
