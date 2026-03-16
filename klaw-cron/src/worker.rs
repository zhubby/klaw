use crate::{time::now_ms, CronError, ScheduleSpec};
use klaw_core::{Envelope, EnvelopeHeader, InboundMessage, MessageTopic, MessageTransport};
use klaw_storage::{CronJob, CronStorage, CronTaskStatus, NewCronTaskRun};
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
    S: CronStorage + 'static,
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
        payload.metadata.insert(
            "cron_id".to_string(),
            serde_json::Value::String(job.id.clone()),
        );

        let envelope = Envelope {
            header: EnvelopeHeader::new(payload.session_key.clone()),
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
}
