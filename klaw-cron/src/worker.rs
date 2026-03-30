use crate::{CronError, ScheduleSpec, time::now_ms};
use klaw_core::{Envelope, EnvelopeHeader, InboundMessage, MessageTopic, MessageTransport};
use klaw_storage::{CronJob, CronStorage, CronTaskStatus, NewCronTaskRun, SessionStorage};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CronWorkerConfig {
    pub poll_interval: Duration,
    pub batch_limit: i64,
    pub missed_run_policy: MissedRunPolicy,
}

impl Default for CronWorkerConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            batch_limit: 64,
            missed_run_policy: MissedRunPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MissedRunPolicy {
    #[default]
    Skip,
    CatchUp,
}

#[derive(Clone)]
pub struct CronWorker<S, T> {
    storage: Arc<S>,
    transport: Arc<T>,
    config: CronWorkerConfig,
}

impl<S, T> CronWorker<S, T>
where
    S: CronStorage + SessionStorage + 'static,
    T: MessageTransport<InboundMessage> + 'static,
{
    pub fn new(storage: Arc<S>, transport: Arc<T>, config: CronWorkerConfig) -> Self {
        Self {
            storage,
            transport,
            config,
        }
    }

    pub async fn run_tick(&self) -> Result<usize, CronError> {
        let now = now_ms();
        let due_jobs = self
            .storage
            .list_due_crons(now, self.config.batch_limit)
            .await?;
        let mut executed = 0usize;

        for job in due_jobs {
            let schedule = ScheduleSpec::from_job(&job)?;
            let next_run_seed_ms = match self.config.missed_run_policy {
                MissedRunPolicy::Skip => now,
                MissedRunPolicy::CatchUp => job.next_run_at_ms,
            };
            let next_run_at_ms =
                schedule.next_run_after_ms_in_timezone(next_run_seed_ms, &job.timezone)?;
            let claimed = self
                .storage
                .claim_next_run(&job.id, job.next_run_at_ms, next_run_at_ms, now)
                .await?;
            if !claimed {
                continue;
            }

            if self.execute_job_run(&job, job.next_run_at_ms).await.is_ok() {
                executed += 1;
            }
        }

        Ok(executed)
    }

    pub async fn run_job_now(&self, cron_id: &str) -> Result<String, CronError> {
        let job = self.storage.get_cron(cron_id).await?;
        self.execute_job_run(&job, now_ms()).await
    }

    pub async fn run_until_stopped(
        &self,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), CronError> {
        while !*shutdown.borrow() {
            self.run_tick().await?;
            tokio::select! {
                _ = shutdown.changed() => {}
                _ = tokio::time::sleep(self.config.poll_interval) => {}
            }
        }
        Ok(())
    }

    async fn publish_inbound(&self, job: &CronJob) -> Result<String, CronError> {
        let mut payload: InboundMessage = serde_json::from_str(&job.payload_json)?;
        let original_session_key = payload.session_key.clone();
        let delivery_session_key = self
            .resolve_delivery_session_key(&payload)
            .await?
            .unwrap_or_else(|| original_session_key.clone());
        self.refresh_session_delivery_metadata(&mut payload, &delivery_session_key)
            .await?;
        payload.metadata.insert(
            "cron_id".to_string(),
            serde_json::Value::String(job.id.clone()),
        );
        payload.metadata.insert(
            "cron.original_session_key".to_string(),
            serde_json::Value::String(original_session_key.clone()),
        );
        payload.metadata.insert(
            "cron.resolved_session_key".to_string(),
            serde_json::Value::String(original_session_key.clone()),
        );

        let envelope = Envelope {
            header: EnvelopeHeader::new(original_session_key),
            metadata: BTreeMap::new(),
            payload,
        };
        let message_id = envelope.header.message_id.to_string();
        self.transport
            .publish(MessageTopic::Inbound.as_str(), envelope)
            .await?;
        Ok(message_id)
    }

    async fn refresh_session_delivery_metadata(
        &self,
        payload: &mut InboundMessage,
        session_key: &str,
    ) -> Result<(), CronError> {
        let Ok(session) = self.storage.get_session(session_key).await else {
            return Ok(());
        };
        let Some(raw) = session.delivery_metadata_json.as_deref() else {
            return Ok(());
        };
        let Ok(metadata) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(raw)
        else {
            return Ok(());
        };
        payload.metadata.extend(metadata.into_iter());
        Ok(())
    }

    async fn execute_job_run(
        &self,
        job: &CronJob,
        scheduled_at_ms: i64,
    ) -> Result<String, CronError> {
        let run_id = Uuid::new_v4().to_string();
        self.storage
            .append_task_run(&NewCronTaskRun {
                id: run_id.clone(),
                cron_id: job.id.clone(),
                scheduled_at_ms,
                status: CronTaskStatus::Pending,
                attempt: 0,
                created_at_ms: now_ms(),
            })
            .await?;
        self.storage.mark_task_running(&run_id, now_ms()).await?;

        match self.publish_inbound(job).await {
            Ok(message_id) => {
                self.storage
                    .mark_task_result(
                        &run_id,
                        CronTaskStatus::Success,
                        now_ms(),
                        None,
                        Some(&message_id),
                    )
                    .await?;
                Ok(message_id)
            }
            Err(err) => {
                self.storage
                    .mark_task_result(
                        &run_id,
                        CronTaskStatus::Failed,
                        now_ms(),
                        Some(&err.to_string()),
                        None,
                    )
                    .await?;
                Err(err)
            }
        }
    }

    async fn resolve_delivery_session_key(
        &self,
        payload: &InboundMessage,
    ) -> Result<Option<String>, CronError> {
        let Some(base_session_key) = infer_base_session_key(payload) else {
            return Ok(None);
        };

        match self.storage.get_session(&base_session_key).await {
            Ok(session) => Ok(session
                .active_session_key
                .filter(|value| !value.trim().is_empty())),
            Err(_) => Ok(None),
        }
    }
}

fn infer_base_session_key(payload: &InboundMessage) -> Option<String> {
    if let Some(base_session_key) = payload
        .metadata
        .get("cron.base_session_key")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(base_session_key.to_string());
    }

    match payload.channel.as_str() {
        "dingtalk" => infer_dingtalk_base_session_key(&payload.session_key, &payload.chat_id),
        "telegram" => infer_telegram_base_session_key(&payload.session_key, &payload.chat_id),
        _ => None,
    }
}

fn infer_dingtalk_base_session_key(session_key: &str, chat_id: &str) -> Option<String> {
    let mut parts = session_key.split(':');
    let channel = parts.next()?.trim();
    if channel != "dingtalk" {
        return None;
    }

    let account_id = parts.next()?.trim();
    if account_id.is_empty() || chat_id.trim().is_empty() {
        return None;
    }

    Some(format!("dingtalk:{account_id}:{chat_id}"))
}

fn infer_telegram_base_session_key(session_key: &str, chat_id: &str) -> Option<String> {
    let mut parts = session_key.split(':');
    let channel = parts.next()?.trim();
    if channel != "telegram" {
        return None;
    }

    let account_id = parts.next()?.trim();
    if account_id.is_empty() || chat_id.trim().is_empty() {
        return None;
    }

    Some(format!("telegram:{account_id}:{chat_id}"))
}

#[cfg(test)]
mod tests {
    use super::{
        CronWorker, CronWorkerConfig, MissedRunPolicy, infer_base_session_key,
        infer_dingtalk_base_session_key,
        infer_telegram_base_session_key,
    };
    use crate::time::now_ms;
    use async_trait::async_trait;
    use klaw_core::{InMemoryTransport, InboundMessage};
    use klaw_storage::{
        ApprovalRecord, ApprovalStatus, ChatRecord, CronJob, CronScheduleKind, CronStorage,
        CronTaskRun, CronTaskStatus, LlmAuditFilterOptions, LlmAuditFilterOptionsQuery,
        LlmAuditQuery, LlmAuditRecord, LlmUsageRecord, LlmUsageSummary, NewApprovalRecord,
        NewCronJob, NewCronTaskRun, NewLlmAuditRecord, NewLlmUsageRecord, NewToolAuditRecord,
        NewWebhookAgentRecord, NewWebhookEventRecord, SessionCompressionState, SessionIndex,
        SessionStorage, StorageError, ToolAuditFilterOptions, ToolAuditFilterOptionsQuery,
        ToolAuditQuery, ToolAuditRecord, UpdateCronJobPatch, UpdateWebhookAgentResult,
        UpdateWebhookEventResult, WebhookAgentQuery, WebhookAgentRecord, WebhookEventQuery,
        WebhookEventRecord,
    };
    use std::{
        collections::BTreeMap,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    type TestTransport = InMemoryTransport<InboundMessage>;

    #[derive(Default)]
    struct FakeStorage {
        jobs: Mutex<Vec<CronJob>>,
        runs: Mutex<Vec<CronTaskRun>>,
        sessions: Mutex<Vec<SessionIndex>>,
    }

    impl FakeStorage {
        fn upsert_session(&self, session: SessionIndex) -> SessionIndex {
            let mut sessions = self.sessions.lock().expect("lock");
            if let Some(item) = sessions
                .iter_mut()
                .find(|item| item.session_key == session.session_key)
            {
                *item = session.clone();
            } else {
                sessions.push(session.clone());
            }
            session
        }
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

        async fn list_due_crons(
            &self,
            now_ms: i64,
            limit: i64,
        ) -> Result<Vec<CronJob>, StorageError> {
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

        async fn append_task_run(
            &self,
            input: &NewCronTaskRun,
        ) -> Result<CronTaskRun, StorageError> {
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
                model_provider_explicit: false,
                model: None,
                model_explicit: false,
                delivery_metadata_json: None,
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

        async fn read_chat_records(
            &self,
            _session_key: &str,
        ) -> Result<Vec<ChatRecord>, StorageError> {
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
            Ok(self.upsert_session(session))
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
            Ok(self.upsert_session(session))
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
            Ok(self.upsert_session(session))
        }

        async fn set_delivery_metadata(
            &self,
            session_key: &str,
            chat_id: &str,
            channel: &str,
            delivery_metadata_json: Option<&str>,
        ) -> Result<SessionIndex, StorageError> {
            let mut session = self.touch_session(session_key, chat_id, channel).await?;
            session.delivery_metadata_json = delivery_metadata_json.map(ToString::to_string);
            Ok(self.upsert_session(session))
        }

        async fn clear_model_routing_override(
            &self,
            session_key: &str,
            chat_id: &str,
            channel: &str,
        ) -> Result<SessionIndex, StorageError> {
            let mut session = self.touch_session(session_key, chat_id, channel).await?;
            session.model_provider = None;
            session.model = None;
            Ok(self.upsert_session(session))
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
            _channel: Option<&str>,
            _sort_order: klaw_storage::SessionSortOrder,
        ) -> Result<Vec<SessionIndex>, StorageError> {
            Ok(self.sessions.lock().expect("lock").clone())
        }

        async fn list_session_channels(&self) -> Result<Vec<String>, StorageError> {
            Ok(Vec::new())
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
                metadata_json: input.metadata_json.clone(),
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

        async fn list_llm_audit_filter_options(
            &self,
            _query: &LlmAuditFilterOptionsQuery,
        ) -> Result<LlmAuditFilterOptions, StorageError> {
            Ok(LlmAuditFilterOptions::default())
        }

        async fn append_tool_audit(
            &self,
            _input: &NewToolAuditRecord,
        ) -> Result<ToolAuditRecord, StorageError> {
            Err(StorageError::backend("not implemented for test"))
        }

        async fn list_tool_audit(
            &self,
            _query: &ToolAuditQuery,
        ) -> Result<Vec<ToolAuditRecord>, StorageError> {
            Ok(Vec::new())
        }

        async fn list_tool_audit_filter_options(
            &self,
            _query: &ToolAuditFilterOptionsQuery,
        ) -> Result<ToolAuditFilterOptions, StorageError> {
            Ok(ToolAuditFilterOptions::default())
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

        async fn append_webhook_agent(
            &self,
            input: &NewWebhookAgentRecord,
        ) -> Result<WebhookAgentRecord, StorageError> {
            Ok(WebhookAgentRecord {
                id: input.id.clone(),
                hook_id: input.hook_id.clone(),
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

        async fn update_webhook_agent_status(
            &self,
            event_id: &str,
            update: &UpdateWebhookAgentResult,
        ) -> Result<WebhookAgentRecord, StorageError> {
            Ok(WebhookAgentRecord {
                id: event_id.to_string(),
                hook_id: String::new(),
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

        async fn list_webhook_agents(
            &self,
            _query: &WebhookAgentQuery,
        ) -> Result<Vec<WebhookAgentRecord>, StorageError> {
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
    fn infers_dingtalk_base_session_from_child_session() {
        assert_eq!(
            infer_dingtalk_base_session_key("dingtalk:acc:chat-1:child", "chat-1"),
            Some("dingtalk:acc:chat-1".to_string())
        );
    }

    #[test]
    fn infers_telegram_base_session_from_child_session() {
        assert_eq!(
            infer_telegram_base_session_key("telegram:acc:chat-1:child", "chat-1"),
            Some("telegram:acc:chat-1".to_string())
        );
    }

    #[test]
    fn prefers_base_session_key_from_metadata() {
        let payload = InboundMessage {
            channel: "dingtalk".to_string(),
            sender_id: "system".to_string(),
            chat_id: "chat-1".to_string(),
            session_key: "cron:job-1".to_string(),
            content: "hello".to_string(),
            media_references: Vec::new(),
            metadata: BTreeMap::from([(
                "cron.base_session_key".to_string(),
                serde_json::Value::String("dingtalk:acc:chat-1".to_string()),
            )]),
        };

        assert_eq!(
            infer_base_session_key(&payload),
            Some("dingtalk:acc:chat-1".to_string())
        );
    }

    #[test]
    fn does_not_infer_base_session_for_stdio() {
        let payload = InboundMessage {
            channel: "stdio".to_string(),
            sender_id: "system".to_string(),
            chat_id: "chat-1".to_string(),
            session_key: "stdio:chat-1".to_string(),
            content: "hello".to_string(),
            media_references: Vec::new(),
            metadata: BTreeMap::new(),
        };

        assert_eq!(infer_base_session_key(&payload), None);
    }

    fn cron_payload_json(channel: &str, session_key: &str, metadata_json: &str) -> String {
        format!(
            "{{\"channel\":\"{channel}\",\"sender_id\":\"system\",\"chat_id\":\"chat1\",\"session_key\":\"{session_key}\",\"content\":\"hello\",\"metadata\":{metadata_json}}}"
        )
    }

    async fn insert_due_job(
        storage: &Arc<FakeStorage>,
        job_id: &str,
        channel: &str,
        session_key: &str,
        metadata_json: &str,
    ) {
        storage
            .create_cron(&NewCronJob {
                id: job_id.to_string(),
                name: "job".to_string(),
                schedule_kind: CronScheduleKind::Every,
                schedule_expr: "5s".to_string(),
                payload_json: cron_payload_json(channel, session_key, metadata_json),
                enabled: true,
                timezone: "UTC".to_string(),
                next_run_at_ms: now_ms().saturating_sub(1_000),
            })
            .await
            .expect("create job");
    }

    async fn insert_job_with_next_run(
        storage: &Arc<FakeStorage>,
        job_id: &str,
        channel: &str,
        session_key: &str,
        metadata_json: &str,
        next_run_at_ms: i64,
    ) {
        storage
            .create_cron(&NewCronJob {
                id: job_id.to_string(),
                name: "job".to_string(),
                schedule_kind: CronScheduleKind::Every,
                schedule_expr: "5s".to_string(),
                payload_json: cron_payload_json(channel, session_key, metadata_json),
                enabled: true,
                timezone: "UTC".to_string(),
                next_run_at_ms,
            })
            .await
            .expect("create job");
    }

    fn test_worker(
        storage: &Arc<FakeStorage>,
        transport: &Arc<TestTransport>,
    ) -> CronWorker<FakeStorage, TestTransport> {
        test_worker_with_policy(storage, transport, MissedRunPolicy::Skip)
    }

    fn test_worker_with_policy(
        storage: &Arc<FakeStorage>,
        transport: &Arc<TestTransport>,
        missed_run_policy: MissedRunPolicy,
    ) -> CronWorker<FakeStorage, TestTransport> {
        CronWorker::new(
            storage.clone(),
            transport.clone(),
            CronWorkerConfig {
                missed_run_policy,
                ..CronWorkerConfig::default()
            },
        )
    }

    async fn assert_single_message_session(
        transport: &Arc<TestTransport>,
        expected_session_key: &str,
    ) {
        let messages = transport.published_messages().await;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].header.session_key, expected_session_key);
        assert_eq!(messages[0].payload.session_key, expected_session_key);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_tick_publishes_inbound_and_marks_success() {
        let storage = Arc::new(FakeStorage::default());
        let transport = Arc::new(InMemoryTransport::new());
        insert_due_job(&storage, "job-1", "cron", "cron:chat1", "{}").await;

        let worker = test_worker(&storage, &transport);
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
    async fn run_tick_advances_next_run_using_job_timezone() {
        let storage = Arc::new(FakeStorage::default());
        let transport = Arc::new(InMemoryTransport::new());
        storage
            .create_cron(&NewCronJob {
                id: "job-tz".to_string(),
                name: "job".to_string(),
                schedule_kind: CronScheduleKind::Cron,
                schedule_expr: "0 0 9 * * *".to_string(),
                payload_json: cron_payload_json("cron", "cron:chat1", "{}"),
                enabled: true,
                timezone: "Asia/Shanghai".to_string(),
                next_run_at_ms: 0,
            })
            .await
            .expect("create job");

        let worker = test_worker_with_policy(&storage, &transport, MissedRunPolicy::CatchUp);
        worker.run_tick().await.expect("tick");

        let job = storage.get_cron("job-tz").await.expect("job");
        assert_eq!(job.next_run_at_ms, 3_600_000);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_tick_skips_missed_runs_by_default() {
        let storage = Arc::new(FakeStorage::default());
        let transport = Arc::new(InMemoryTransport::new());
        let overdue_next_run_at_ms = now_ms().saturating_sub(60_000);
        insert_job_with_next_run(
            &storage,
            "job-skip",
            "cron",
            "cron:chat1",
            "{}",
            overdue_next_run_at_ms,
        )
        .await;

        let before_tick = now_ms();
        let worker = test_worker(&storage, &transport);
        worker.run_tick().await.expect("tick");

        let job = storage.get_cron("job-skip").await.expect("job");
        assert!(job.next_run_at_ms >= before_tick + 5_000);
        assert!(job.next_run_at_ms > overdue_next_run_at_ms + 5_000);

        let runs = storage.list_task_runs("job-skip", 10, 0).await.expect("runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].scheduled_at_ms, overdue_next_run_at_ms);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_tick_can_catch_up_missed_runs_when_enabled() {
        let storage = Arc::new(FakeStorage::default());
        let transport = Arc::new(InMemoryTransport::new());
        let overdue_next_run_at_ms = now_ms().saturating_sub(60_000);
        insert_job_with_next_run(
            &storage,
            "job-catch-up",
            "cron",
            "cron:chat1",
            "{}",
            overdue_next_run_at_ms,
        )
        .await;

        let worker = test_worker_with_policy(&storage, &transport, MissedRunPolicy::CatchUp);
        worker.run_tick().await.expect("tick");

        let job = storage.get_cron("job-catch-up").await.expect("job");
        assert_eq!(job.next_run_at_ms, overdue_next_run_at_ms + 5_000);

        let runs = storage
            .list_task_runs("job-catch-up", 10, 0)
            .await
            .expect("runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].scheduled_at_ms, overdue_next_run_at_ms);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_tick_resolves_dingtalk_active_session_from_metadata() {
        let storage = Arc::new(FakeStorage::default());
        let transport = Arc::new(InMemoryTransport::new());
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
            .set_delivery_metadata(
                "dingtalk:acc:chat1:child",
                "chat1",
                "dingtalk",
                Some(
                    "{\"channel.dingtalk.session_webhook\":\"https://example/new-session\",\"channel.dingtalk.bot_title\":\"Klaw\"}",
                ),
            )
            .await
            .expect("delivery metadata should be stored");
        insert_due_job(
            &storage,
            "job-dingtalk",
            "dingtalk",
            "cron:job-dingtalk",
            "{\"cron.base_session_key\":\"dingtalk:acc:chat1\",\"channel.dingtalk.session_webhook\":\"https://example/stale-session\"}",
        )
        .await;

        let worker = test_worker(&storage, &transport);
        worker.run_tick().await.expect("tick");

        let messages = transport.published_messages().await;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].header.session_key, "cron:job-dingtalk");
        assert_eq!(messages[0].payload.session_key, "cron:job-dingtalk");
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
            Some("cron:job-dingtalk")
        );
        assert_eq!(
            messages[0]
                .payload
                .metadata
                .get("channel.dingtalk.session_webhook")
                .and_then(|value| value.as_str()),
            Some("https://example/new-session")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_tick_resolves_telegram_active_session_from_metadata() {
        let storage = Arc::new(FakeStorage::default());
        let transport = Arc::new(InMemoryTransport::new());
        storage
            .set_active_session(
                "telegram:acc:chat1",
                "chat1",
                "telegram",
                "telegram:acc:chat1:child",
            )
            .await
            .expect("session route should exist");
        insert_due_job(
            &storage,
            "job-telegram",
            "telegram",
            "cron:job-telegram",
            "{\"cron.base_session_key\":\"telegram:acc:chat1\"}",
        )
        .await;

        let worker = test_worker(&storage, &transport);
        worker.run_tick().await.expect("tick");

        let messages = transport.published_messages().await;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].header.session_key, "cron:job-telegram");
        assert_eq!(messages[0].payload.session_key, "cron:job-telegram");
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
            Some("cron:job-telegram")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_tick_falls_back_to_original_session_when_no_active_route_exists() {
        let storage = Arc::new(FakeStorage::default());
        let transport = Arc::new(InMemoryTransport::new());
        insert_due_job(
            &storage,
            "job-fallback",
            "dingtalk",
            "cron:job-fallback",
            "{\"cron.base_session_key\":\"dingtalk:acc:chat1\"}",
        )
        .await;

        let worker = test_worker(&storage, &transport);
        worker.run_tick().await.expect("tick");

        assert_single_message_session(&transport, "cron:job-fallback").await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_tick_keeps_stdio_session_unchanged() {
        let storage = Arc::new(FakeStorage::default());
        let transport = Arc::new(InMemoryTransport::new());
        insert_due_job(&storage, "job-stdio", "stdio", "stdio:chat1", "{}").await;

        let worker = test_worker(&storage, &transport);
        worker.run_tick().await.expect("tick");

        assert_single_message_session(&transport, "stdio:chat1").await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_job_now_publishes_inbound_and_records_run_without_schedule_claim() {
        let storage = Arc::new(FakeStorage::default());
        let transport = Arc::new(InMemoryTransport::new());
        let next_run_at_ms = now_ms() + 60_000;
        insert_job_with_next_run(
            &storage,
            "job-manual",
            "cron",
            "cron:chat1",
            "{}",
            next_run_at_ms,
        )
        .await;

        let worker = test_worker(&storage, &transport);
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
}
