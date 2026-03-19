use crate::{time::now_ms, CronError, ScheduleSpec};
use klaw_core::{Envelope, EnvelopeHeader, InboundMessage, MessageTopic, MessageTransport};
use klaw_storage::{CronJob, CronStorage, CronTaskStatus, NewCronTaskRun, SessionStorage};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CronWorkerConfig {
    pub poll_interval: Duration,
    pub batch_limit: i64,
}

impl Default for CronWorkerConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            batch_limit: 64,
        }
    }
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
            let next_run_at_ms = schedule.next_run_after_ms(job.next_run_at_ms)?;
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
        let resolved_session_key = self
            .resolve_active_session_key(&payload)
            .await?
            .unwrap_or_else(|| original_session_key.clone());
        payload.metadata.insert(
            "cron_id".to_string(),
            serde_json::Value::String(job.id.clone()),
        );
        payload.metadata.insert(
            "cron.original_session_key".to_string(),
            serde_json::Value::String(original_session_key),
        );
        payload.metadata.insert(
            "cron.resolved_session_key".to_string(),
            serde_json::Value::String(resolved_session_key.clone()),
        );
        payload.session_key = resolved_session_key.clone();

        let envelope = Envelope {
            header: EnvelopeHeader::new(resolved_session_key),
            metadata: BTreeMap::new(),
            payload,
        };
        let message_id = envelope.header.message_id.to_string();
        self.transport
            .publish(MessageTopic::Inbound.as_str(), envelope)
            .await?;
        Ok(message_id)
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

    async fn resolve_active_session_key(
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

#[cfg(test)]
mod tests {
    use super::{infer_base_session_key, infer_dingtalk_base_session_key};
    use klaw_core::InboundMessage;
    use std::collections::BTreeMap;

    #[test]
    fn infers_dingtalk_base_session_from_child_session() {
        assert_eq!(
            infer_dingtalk_base_session_key("dingtalk:acc:chat-1:child", "chat-1"),
            Some("dingtalk:acc:chat-1".to_string())
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
}
