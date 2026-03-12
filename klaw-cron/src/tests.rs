use crate::{time::now_ms, CronWorker, CronWorkerConfig, ScheduleSpec};
use async_trait::async_trait;
use klaw_core::InMemoryTransport;
use klaw_storage::{
    CronJob, CronScheduleKind, CronStorage, CronTaskRun, CronTaskStatus, NewCronJob,
    NewCronTaskRun, StorageError, UpdateCronJobPatch,
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct FakeStorage {
    jobs: Mutex<Vec<CronJob>>,
    runs: Mutex<Vec<CronTaskRun>>,
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
