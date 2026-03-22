use crate::{time::now_ms, CronWorker, CronWorkerConfig, ScheduleSpec};
use async_trait::async_trait;
use klaw_core::InMemoryTransport;
use klaw_storage::{
    ApprovalRecord, ApprovalStatus, ChatRecord, CronJob, CronScheduleKind, CronStorage,
    CronTaskRun, CronTaskStatus, LlmAuditQuery, LlmAuditRecord, LlmUsageRecord, LlmUsageSummary,
    NewApprovalRecord, NewCronJob, NewCronTaskRun, NewLlmAuditRecord, NewLlmUsageRecord,
    NewWebhookEventRecord, SessionCompressionState, SessionIndex, SessionStorage, StorageError,
    UpdateCronJobPatch, UpdateWebhookEventResult, WebhookEventQuery, WebhookEventRecord,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct FakeStorage {
    jobs: Mutex<Vec<CronJob>>,
    runs: Mutex<Vec<CronTaskRun>>,
    sessions: Mutex<Vec<SessionIndex>>,
}

#[async_trait]
impl CronStorage for FakeStorage {
    async fn create_cron(&self, input: &NewCronJob) -> Result<CronJob, StorageError> {
        let now = now_ms();
        let job = CronJob {
            id: input.id.clone(),
            name: input.name.clone(),
            schedule_kind: input.schedule_kind,
            schedule_expr: input.schedule_expr.clone(),
            payload_json: input.payload_json.clone(),
            enabled: input.enabled,
            timezone: input.timezone.clone(),
            next_run_at_ms: input.next_run_at_ms,
            last_run_at_ms: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        self.jobs.lock().expect("lock").push(job.clone());
        Ok(job)
    }

    async fn update_cron(
        &self,
        _cron_id: &str,
        _patch: &UpdateCronJobPatch,
    ) -> Result<CronJob, StorageError> {
        Err(StorageError::backend("not implemented for test"))
    }

    async fn set_enabled(&self, _cron_id: &str, _enabled: bool) -> Result<(), StorageError> {
        Ok(())
    }

    async fn delete_cron(&self, _cron_id: &str) -> Result<(), StorageError> {
        Ok(())
    }

    async fn get_cron(&self, cron_id: &str) -> Result<CronJob, StorageError> {
        self.jobs
            .lock()
            .expect("lock")
            .iter()
            .find(|job| job.id == cron_id)
            .cloned()
            .ok_or_else(|| StorageError::backend("not found"))
    }

    async fn list_crons(&self, limit: i64, offset: i64) -> Result<Vec<CronJob>, StorageError> {
        let mut jobs: Vec<CronJob> = self.jobs.lock().expect("lock").iter().cloned().collect();
        jobs.sort_by_key(|job| std::cmp::Reverse(job.updated_at_ms));
        let skip = offset.max(0) as usize;
        let take = limit.max(1) as usize;
        Ok(jobs.into_iter().skip(skip).take(take).collect())
    }

    async fn list_due_crons(&self, now_ms: i64, limit: i64) -> Result<Vec<CronJob>, StorageError> {
        let mut jobs: Vec<CronJob> = self
            .jobs
            .lock()
            .expect("lock")
            .iter()
            .filter(|job| job.enabled && job.next_run_at_ms <= now_ms)
            .cloned()
            .collect();
        jobs.sort_by_key(|job| job.next_run_at_ms);
        jobs.truncate(limit.max(1) as usize);
        Ok(jobs)
    }

    async fn claim_next_run(
        &self,
        cron_id: &str,
        expected_next_run_at_ms: i64,
        new_next_run_at_ms: i64,
        _now_ms: i64,
    ) -> Result<bool, StorageError> {
        let mut jobs = self.jobs.lock().expect("lock");
        if let Some(job) = jobs
            .iter_mut()
            .find(|job| job.id == cron_id && job.next_run_at_ms == expected_next_run_at_ms)
        {
            job.last_run_at_ms = Some(expected_next_run_at_ms);
            job.next_run_at_ms = new_next_run_at_ms;
            return Ok(true);
        }
        Ok(false)
    }

    async fn append_task_run(&self, input: &NewCronTaskRun) -> Result<CronTaskRun, StorageError> {
        let run = CronTaskRun {
            id: input.id.clone(),
            cron_id: input.cron_id.clone(),
            scheduled_at_ms: input.scheduled_at_ms,
            started_at_ms: None,
            finished_at_ms: None,
            status: input.status,
            attempt: input.attempt,
            error_message: None,
            published_message_id: None,
            created_at_ms: input.created_at_ms,
        };
        self.runs.lock().expect("lock").push(run.clone());
        Ok(run)
    }

    async fn mark_task_running(
        &self,
        run_id: &str,
        started_at_ms: i64,
    ) -> Result<(), StorageError> {
        if let Some(run) = self
            .runs
            .lock()
            .expect("lock")
            .iter_mut()
            .find(|run| run.id == run_id)
        {
            run.status = CronTaskStatus::Running;
            run.started_at_ms = Some(started_at_ms);
        }
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
        if let Some(run) = self
            .runs
            .lock()
            .expect("lock")
            .iter_mut()
            .find(|run| run.id == run_id)
        {
            run.status = status;
            run.finished_at_ms = Some(finished_at_ms);
            run.error_message = error_message.map(ToString::to_string);
            run.published_message_id = published_message_id.map(ToString::to_string);
        }
        Ok(())
    }

    async fn list_task_runs(
        &self,
        cron_id: &str,
        _limit: i64,
        _offset: i64,
    ) -> Result<Vec<CronTaskRun>, StorageError> {
        Ok(self
            .runs
            .lock()
            .expect("lock")
            .iter()
            .filter(|run| run.cron_id == cron_id)
            .cloned()
            .collect())
    }
}

#[async_trait]
impl SessionStorage for FakeStorage {
    async fn touch_session(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, StorageError> {
        let mut sessions = self.sessions.lock().expect("lock");
        if let Some(session) = sessions
            .iter_mut()
            .find(|item| item.session_key == session_key)
        {
            session.chat_id = chat_id.to_string();
            session.channel = channel.to_string();
            return Ok(session.clone());
        }

        let session = SessionIndex {
            session_key: session_key.to_string(),
            chat_id: chat_id.to_string(),
            channel: channel.to_string(),
            active_session_key: Some(session_key.to_string()),
            model_provider: None,
            model: None,
            created_at_ms: now_ms(),
            updated_at_ms: now_ms(),
            last_message_at_ms: now_ms(),
            turn_count: 0,
            jsonl_path: String::new(),
        };
        sessions.push(session.clone());
        Ok(session)
    }

    async fn complete_turn(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, StorageError> {
        self.touch_session(session_key, chat_id, channel).await
    }

    async fn append_chat_record(
        &self,
        _session_key: &str,
        _record: &ChatRecord,
    ) -> Result<(), StorageError> {
        Ok(())
    }

    async fn read_chat_records(&self, _session_key: &str) -> Result<Vec<ChatRecord>, StorageError> {
        Ok(Vec::new())
    }

    async fn get_session(&self, session_key: &str) -> Result<SessionIndex, StorageError> {
        self.sessions
            .lock()
            .expect("lock")
            .iter()
            .find(|session| session.session_key == session_key)
            .cloned()
            .ok_or_else(|| StorageError::backend("not found"))
    }

    async fn get_or_create_session_state(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        _default_provider: &str,
        _default_model: &str,
    ) -> Result<SessionIndex, StorageError> {
        self.touch_session(session_key, chat_id, channel).await
    }

    async fn set_active_session(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        active_session_key: &str,
    ) -> Result<SessionIndex, StorageError> {
        let mut session = self.touch_session(session_key, chat_id, channel).await?;
        session.active_session_key = Some(active_session_key.to_string());
        let mut sessions = self.sessions.lock().expect("lock");
        if let Some(item) = sessions
            .iter_mut()
            .find(|item| item.session_key == session_key)
        {
            *item = session.clone();
        }
        Ok(session)
    }

    async fn set_model_provider(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        model_provider: &str,
        model: &str,
    ) -> Result<SessionIndex, StorageError> {
        let mut session = self.touch_session(session_key, chat_id, channel).await?;
        session.model_provider = Some(model_provider.to_string());
        session.model = Some(model.to_string());
        let mut sessions = self.sessions.lock().expect("lock");
        if let Some(item) = sessions
            .iter_mut()
            .find(|item| item.session_key == session_key)
        {
            *item = session.clone();
        }
        Ok(session)
    }

    async fn set_model(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        model: &str,
    ) -> Result<SessionIndex, StorageError> {
        let mut session = self.touch_session(session_key, chat_id, channel).await?;
        session.model = Some(model.to_string());
        let mut sessions = self.sessions.lock().expect("lock");
        if let Some(item) = sessions
            .iter_mut()
            .find(|item| item.session_key == session_key)
        {
            *item = session.clone();
        }
        Ok(session)
    }

    async fn get_session_compression_state(
        &self,
        _session_key: &str,
    ) -> Result<Option<SessionCompressionState>, StorageError> {
        Ok(None)
    }

    async fn set_session_compression_state(
        &self,
        _session_key: &str,
        _state: &SessionCompressionState,
    ) -> Result<(), StorageError> {
        Ok(())
    }

    async fn list_sessions(
        &self,
        _limit: i64,
        _offset: i64,
        _updated_from_ms: Option<i64>,
        _updated_to_ms: Option<i64>,
    ) -> Result<Vec<SessionIndex>, StorageError> {
        Ok(self.sessions.lock().expect("lock").clone())
    }

    async fn append_llm_usage(
        &self,
        input: &NewLlmUsageRecord,
    ) -> Result<LlmUsageRecord, StorageError> {
        Ok(LlmUsageRecord {
            id: input.id.clone(),
            session_key: input.session_key.clone(),
            chat_id: input.chat_id.clone(),
            turn_index: input.turn_index,
            request_seq: input.request_seq,
            provider: input.provider.clone(),
            model: input.model.clone(),
            wire_api: input.wire_api.clone(),
            input_tokens: input.input_tokens,
            output_tokens: input.output_tokens,
            total_tokens: input.total_tokens,
            cached_input_tokens: input.cached_input_tokens,
            reasoning_tokens: input.reasoning_tokens,
            source: input.source,
            provider_request_id: input.provider_request_id.clone(),
            provider_response_id: input.provider_response_id.clone(),
            created_at_ms: now_ms(),
        })
    }

    async fn list_llm_usage(
        &self,
        _session_key: &str,
        _limit: i64,
        _offset: i64,
    ) -> Result<Vec<LlmUsageRecord>, StorageError> {
        Ok(Vec::new())
    }

    async fn sum_llm_usage_by_session(
        &self,
        _session_key: &str,
    ) -> Result<LlmUsageSummary, StorageError> {
        Ok(LlmUsageSummary::default())
    }

    async fn sum_llm_usage_by_turn(
        &self,
        _session_key: &str,
        _turn_index: i64,
    ) -> Result<LlmUsageSummary, StorageError> {
        Ok(LlmUsageSummary::default())
    }

    async fn append_llm_audit(
        &self,
        input: &NewLlmAuditRecord,
    ) -> Result<LlmAuditRecord, StorageError> {
        Ok(LlmAuditRecord {
            id: input.id.clone(),
            session_key: input.session_key.clone(),
            chat_id: input.chat_id.clone(),
            turn_index: input.turn_index,
            request_seq: input.request_seq,
            provider: input.provider.clone(),
            model: input.model.clone(),
            wire_api: input.wire_api.clone(),
            status: input.status,
            error_code: input.error_code.clone(),
            error_message: input.error_message.clone(),
            provider_request_id: input.provider_request_id.clone(),
            provider_response_id: input.provider_response_id.clone(),
            request_body_json: input.request_body_json.clone(),
            response_body_json: input.response_body_json.clone(),
            requested_at_ms: input.requested_at_ms,
            responded_at_ms: input.responded_at_ms,
            created_at_ms: now_ms(),
        })
    }

    async fn list_llm_audit(
        &self,
        _query: &LlmAuditQuery,
    ) -> Result<Vec<LlmAuditRecord>, StorageError> {
        Ok(Vec::new())
    }

    async fn append_webhook_event(
        &self,
        input: &NewWebhookEventRecord,
    ) -> Result<WebhookEventRecord, StorageError> {
        Ok(WebhookEventRecord {
            id: input.id.clone(),
            source: input.source.clone(),
            event_type: input.event_type.clone(),
            session_key: input.session_key.clone(),
            chat_id: input.chat_id.clone(),
            sender_id: input.sender_id.clone(),
            content: input.content.clone(),
            payload_json: input.payload_json.clone(),
            metadata_json: input.metadata_json.clone(),
            status: input.status,
            error_message: input.error_message.clone(),
            response_summary: input.response_summary.clone(),
            received_at_ms: input.received_at_ms,
            processed_at_ms: input.processed_at_ms,
            remote_addr: input.remote_addr.clone(),
            created_at_ms: now_ms(),
        })
    }

    async fn update_webhook_event_status(
        &self,
        event_id: &str,
        update: &UpdateWebhookEventResult,
    ) -> Result<WebhookEventRecord, StorageError> {
        Ok(WebhookEventRecord {
            id: event_id.to_string(),
            source: String::new(),
            event_type: String::new(),
            session_key: String::new(),
            chat_id: String::new(),
            sender_id: String::new(),
            content: String::new(),
            payload_json: None,
            metadata_json: None,
            status: update.status,
            error_message: update.error_message.clone(),
            response_summary: update.response_summary.clone(),
            received_at_ms: now_ms(),
            processed_at_ms: update.processed_at_ms,
            remote_addr: None,
            created_at_ms: now_ms(),
        })
    }

    async fn list_webhook_events(
        &self,
        _query: &WebhookEventQuery,
    ) -> Result<Vec<WebhookEventRecord>, StorageError> {
        Ok(Vec::new())
    }

    async fn create_approval(
        &self,
        _input: &NewApprovalRecord,
    ) -> Result<ApprovalRecord, StorageError> {
        Err(StorageError::backend("not implemented for test"))
    }

    async fn get_approval(&self, _approval_id: &str) -> Result<ApprovalRecord, StorageError> {
        Err(StorageError::backend("not implemented for test"))
    }

    async fn update_approval_status(
        &self,
        _approval_id: &str,
        _status: ApprovalStatus,
        _approved_by: Option<&str>,
    ) -> Result<ApprovalRecord, StorageError> {
        Err(StorageError::backend("not implemented for test"))
    }

    async fn consume_approved_shell_command(
        &self,
        _approval_id: &str,
        _session_key: &str,
        _command_hash: &str,
        _now_ms: i64,
    ) -> Result<bool, StorageError> {
        Ok(false)
    }

    async fn consume_latest_approved_shell_command(
        &self,
        _session_key: &str,
        _command_hash: &str,
        _now_ms: i64,
    ) -> Result<bool, StorageError> {
        Ok(false)
    }

    fn session_jsonl_path(&self, _session_key: &str) -> PathBuf {
        PathBuf::new()
    }
}

#[test]
fn parse_every_schedule() {
    let spec = ScheduleSpec::from_kind_expr(CronScheduleKind::Every, "45s").expect("parse");
    let next = spec.next_run_after_ms(1_000).expect("next");
    assert_eq!(next, 46_000);
}

#[test]
fn parse_cron_schedule() {
    let spec =
        ScheduleSpec::from_kind_expr(CronScheduleKind::Cron, "0 */2 * * * *").expect("parse");
    let next = spec.next_run_after_ms(0).expect("next");
    assert_eq!(next, 120_000);
}

#[tokio::test(flavor = "current_thread")]
async fn run_tick_publishes_inbound_and_marks_success() {
    let storage = Arc::new(FakeStorage::default());
    let transport = Arc::new(InMemoryTransport::new());
    let now = now_ms();
    storage
        .create_cron(&NewCronJob {
            id: "job-1".to_string(),
            name: "job".to_string(),
            schedule_kind: CronScheduleKind::Every,
            schedule_expr: "5s".to_string(),
            payload_json: "{\"channel\":\"cron\",\"sender_id\":\"system\",\"chat_id\":\"chat1\",\"session_key\":\"cron:chat1\",\"content\":\"hello\",\"metadata\":{}}".to_string(),
            enabled: true,
            timezone: "UTC".to_string(),
            next_run_at_ms: now.saturating_sub(1_000),
        })
        .await
        .expect("create job");

    let worker = CronWorker::new(
        storage.clone(),
        transport.clone(),
        CronWorkerConfig::default(),
    );
    let executed = worker.run_tick().await.expect("tick");
    assert_eq!(executed, 1);

    let messages = transport.published_messages().await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].payload.channel, "cron");
    assert_eq!(
        messages[0]
            .payload
            .metadata
            .get("cron_id")
            .and_then(|v| v.as_str()),
        Some("job-1")
    );

    let runs = storage.list_task_runs("job-1", 10, 0).await.expect("runs");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, CronTaskStatus::Success);
}

#[tokio::test(flavor = "current_thread")]
async fn run_tick_resolves_dingtalk_active_session_from_metadata() {
    let storage = Arc::new(FakeStorage::default());
    let transport = Arc::new(InMemoryTransport::new());
    let now = now_ms();
    storage
        .set_active_session(
            "dingtalk:acc:chat1",
            "chat1",
            "dingtalk",
            "dingtalk:acc:chat1:child",
        )
        .await
        .expect("session route should exist");
    storage
        .create_cron(&NewCronJob {
            id: "job-dingtalk".to_string(),
            name: "job".to_string(),
            schedule_kind: CronScheduleKind::Every,
            schedule_expr: "5s".to_string(),
            payload_json: "{\"channel\":\"dingtalk\",\"sender_id\":\"system\",\"chat_id\":\"chat1\",\"session_key\":\"cron:job-dingtalk\",\"content\":\"hello\",\"metadata\":{\"cron.base_session_key\":\"dingtalk:acc:chat1\"}}".to_string(),
            enabled: true,
            timezone: "UTC".to_string(),
            next_run_at_ms: now.saturating_sub(1_000),
        })
        .await
        .expect("create job");

    let worker = CronWorker::new(
        storage.clone(),
        transport.clone(),
        CronWorkerConfig::default(),
    );
    worker.run_tick().await.expect("tick");

    let messages = transport.published_messages().await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].header.session_key, "dingtalk:acc:chat1:child");
    assert_eq!(messages[0].payload.session_key, "dingtalk:acc:chat1:child");
    assert_eq!(
        messages[0]
            .payload
            .metadata
            .get("cron.original_session_key")
            .and_then(|value| value.as_str()),
        Some("cron:job-dingtalk")
    );
    assert_eq!(
        messages[0]
            .payload
            .metadata
            .get("cron.resolved_session_key")
            .and_then(|value| value.as_str()),
        Some("dingtalk:acc:chat1:child")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn run_tick_resolves_telegram_active_session_from_metadata() {
    let storage = Arc::new(FakeStorage::default());
    let transport = Arc::new(InMemoryTransport::new());
    let now = now_ms();
    storage
        .set_active_session(
            "telegram:acc:chat1",
            "chat1",
            "telegram",
            "telegram:acc:chat1:child",
        )
        .await
        .expect("session route should exist");
    storage
        .create_cron(&NewCronJob {
            id: "job-telegram".to_string(),
            name: "job".to_string(),
            schedule_kind: CronScheduleKind::Every,
            schedule_expr: "5s".to_string(),
            payload_json: "{\"channel\":\"telegram\",\"sender_id\":\"system\",\"chat_id\":\"chat1\",\"session_key\":\"cron:job-telegram\",\"content\":\"hello\",\"metadata\":{\"cron.base_session_key\":\"telegram:acc:chat1\"}}".to_string(),
            enabled: true,
            timezone: "UTC".to_string(),
            next_run_at_ms: now.saturating_sub(1_000),
        })
        .await
        .expect("create job");

    let worker = CronWorker::new(
        storage.clone(),
        transport.clone(),
        CronWorkerConfig::default(),
    );
    worker.run_tick().await.expect("tick");

    let messages = transport.published_messages().await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].header.session_key, "telegram:acc:chat1:child");
    assert_eq!(messages[0].payload.session_key, "telegram:acc:chat1:child");
    assert_eq!(
        messages[0]
            .payload
            .metadata
            .get("cron.original_session_key")
            .and_then(|value| value.as_str()),
        Some("cron:job-telegram")
    );
    assert_eq!(
        messages[0]
            .payload
            .metadata
            .get("cron.resolved_session_key")
            .and_then(|value| value.as_str()),
        Some("telegram:acc:chat1:child")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn run_tick_falls_back_to_original_session_when_no_active_route_exists() {
    let storage = Arc::new(FakeStorage::default());
    let transport = Arc::new(InMemoryTransport::new());
    let now = now_ms();
    storage
        .create_cron(&NewCronJob {
            id: "job-fallback".to_string(),
            name: "job".to_string(),
            schedule_kind: CronScheduleKind::Every,
            schedule_expr: "5s".to_string(),
            payload_json: "{\"channel\":\"dingtalk\",\"sender_id\":\"system\",\"chat_id\":\"chat1\",\"session_key\":\"cron:job-fallback\",\"content\":\"hello\",\"metadata\":{\"cron.base_session_key\":\"dingtalk:acc:chat1\"}}".to_string(),
            enabled: true,
            timezone: "UTC".to_string(),
            next_run_at_ms: now.saturating_sub(1_000),
        })
        .await
        .expect("create job");

    let worker = CronWorker::new(
        storage.clone(),
        transport.clone(),
        CronWorkerConfig::default(),
    );
    worker.run_tick().await.expect("tick");

    let messages = transport.published_messages().await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].header.session_key, "cron:job-fallback");
    assert_eq!(messages[0].payload.session_key, "cron:job-fallback");
}

#[tokio::test(flavor = "current_thread")]
async fn run_tick_keeps_stdio_session_unchanged() {
    let storage = Arc::new(FakeStorage::default());
    let transport = Arc::new(InMemoryTransport::new());
    let now = now_ms();
    storage
        .create_cron(&NewCronJob {
            id: "job-stdio".to_string(),
            name: "job".to_string(),
            schedule_kind: CronScheduleKind::Every,
            schedule_expr: "5s".to_string(),
            payload_json: "{\"channel\":\"stdio\",\"sender_id\":\"system\",\"chat_id\":\"chat1\",\"session_key\":\"stdio:chat1\",\"content\":\"hello\",\"metadata\":{}}".to_string(),
            enabled: true,
            timezone: "UTC".to_string(),
            next_run_at_ms: now.saturating_sub(1_000),
        })
        .await
        .expect("create job");

    let worker = CronWorker::new(
        storage.clone(),
        transport.clone(),
        CronWorkerConfig::default(),
    );
    worker.run_tick().await.expect("tick");

    let messages = transport.published_messages().await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].header.session_key, "stdio:chat1");
    assert_eq!(messages[0].payload.session_key, "stdio:chat1");
}

#[tokio::test(flavor = "current_thread")]
async fn run_job_now_publishes_inbound_and_records_run_without_schedule_claim() {
    let storage = Arc::new(FakeStorage::default());
    let transport = Arc::new(InMemoryTransport::new());
    let next_run_at_ms = now_ms() + 60_000;
    storage
        .create_cron(&NewCronJob {
            id: "job-manual".to_string(),
            name: "job".to_string(),
            schedule_kind: CronScheduleKind::Every,
            schedule_expr: "5s".to_string(),
            payload_json: "{\"channel\":\"cron\",\"sender_id\":\"system\",\"chat_id\":\"chat1\",\"session_key\":\"cron:chat1\",\"content\":\"hello\",\"metadata\":{}}".to_string(),
            enabled: true,
            timezone: "UTC".to_string(),
            next_run_at_ms,
        })
        .await
        .expect("create job");

    let worker = CronWorker::new(
        storage.clone(),
        transport.clone(),
        CronWorkerConfig::default(),
    );
    worker.run_job_now("job-manual").await.expect("manual run");

    let messages = transport.published_messages().await;
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0]
            .payload
            .metadata
            .get("cron_id")
            .and_then(|v| v.as_str()),
        Some("job-manual")
    );

    let job = storage.get_cron("job-manual").await.expect("job");
    assert_eq!(job.next_run_at_ms, next_run_at_ms);

    let runs = storage
        .list_task_runs("job-manual", 10, 0)
        .await
        .expect("runs");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, CronTaskStatus::Success);
}
