use klaw_core::{Envelope, EnvelopeHeader, InboundMessage, MessageTopic, MessageTransport};
use klaw_storage::{
    ChatRecord, HeartbeatJob, HeartbeatStorage, HeartbeatTaskRun, HeartbeatTaskStatus,
    NewHeartbeatJob, NewHeartbeatTaskRun, SessionStorage, StorageError, UpdateHeartbeatJobPatch,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use thiserror::Error;
use uuid::Uuid;

pub const TRIGGER_KIND_KEY: &str = "trigger.kind";
pub const TRIGGER_KIND_HEARTBEAT: &str = "heartbeat";
pub const HEARTBEAT_SESSION_KEY: &str = "heartbeat.session_key";
pub const HEARTBEAT_SILENT_ACK_TOKEN_KEY: &str = "heartbeat.silent_ack_token";
pub const HEARTBEAT_RESOLVED_SESSION_KEY: &str = "heartbeat.resolved_session_key";
pub const HEARTBEAT_RECENT_MESSAGES_LIMIT_KEY: &str = "heartbeat.recent_messages_limit";
pub const DEFAULT_SILENT_ACK_TOKEN: &str = "HEARTBEAT_OK";
pub const DEFAULT_TIMEZONE: &str = "UTC";
pub const DEFAULT_RECENT_MESSAGES_LIMIT: i64 = 12;
pub const DEFAULT_HEARTBEAT_EVERY: &str = "30m";
pub const DEFAULT_HEARTBEAT_PROMPT: &str =
    "Review the session state. If no user-visible action is needed, reply exactly HEARTBEAT_OK.";
const META_CONVERSATION_HISTORY_KEY: &str = "agent.conversation_history";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatInput {
    pub id: Option<String>,
    pub session_key: String,
    pub channel: String,
    pub chat_id: String,
    pub enabled: bool,
    pub every: String,
    pub prompt: String,
    pub silent_ack_token: String,
    pub recent_messages_limit: i64,
    pub timezone: String,
}

#[derive(Debug, Error)]
pub enum HeartbeatError {
    #[error("invalid heartbeat input: {0}")]
    InvalidInput(String),
    #[error("invalid heartbeat schedule: {0}")]
    InvalidSchedule(String),
    #[error("failed to serialize heartbeat payload: {0}")]
    SerializePayload(#[from] serde_json::Error),
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("transport error: {0}")]
    Transport(String),
}

#[derive(Clone)]
pub struct HeartbeatManager<S> {
    storage: Arc<S>,
}

impl<S> HeartbeatManager<S> {
    pub fn new(storage: Arc<S>) -> Self {
        Self { storage }
    }
}

impl<S> HeartbeatManager<S>
where
    S: HeartbeatStorage + Send + Sync + 'static,
{
    pub async fn list_jobs(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<HeartbeatJob>, HeartbeatError> {
        Ok(self.storage.list_heartbeats(limit, offset).await?)
    }

    pub async fn list_runs(
        &self,
        heartbeat_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<HeartbeatTaskRun>, HeartbeatError> {
        Ok(self
            .storage
            .list_heartbeat_task_runs(heartbeat_id, limit, offset)
            .await?)
    }

    pub async fn get_job(&self, heartbeat_id: &str) -> Result<HeartbeatJob, HeartbeatError> {
        Ok(self.storage.get_heartbeat(heartbeat_id).await?)
    }

    pub async fn create_job(&self, input: &HeartbeatInput) -> Result<HeartbeatJob, HeartbeatError> {
        let normalized = normalize_input(input)?;
        let next_run_at_ms = compute_next_run_at_ms(&normalized.every)?;
        let id = normalized
            .id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        Ok(self
            .storage
            .create_heartbeat(&NewHeartbeatJob {
                id,
                session_key: normalized.session_key,
                channel: normalized.channel,
                chat_id: normalized.chat_id,
                enabled: normalized.enabled,
                every: normalized.every,
                prompt: normalized.prompt,
                silent_ack_token: normalized.silent_ack_token,
                recent_messages_limit: normalized.recent_messages_limit,
                timezone: normalized.timezone,
                next_run_at_ms,
            })
            .await?)
    }

    pub async fn update_job(
        &self,
        heartbeat_id: &str,
        input: &HeartbeatInput,
    ) -> Result<HeartbeatJob, HeartbeatError> {
        let normalized = normalize_input(input)?;
        let next_run_at_ms = compute_next_run_at_ms(&normalized.every)?;
        Ok(self
            .storage
            .update_heartbeat(
                heartbeat_id,
                &UpdateHeartbeatJobPatch {
                    session_key: Some(normalized.session_key),
                    channel: Some(normalized.channel),
                    chat_id: Some(normalized.chat_id),
                    every: Some(normalized.every),
                    prompt: Some(normalized.prompt),
                    silent_ack_token: Some(normalized.silent_ack_token),
                    recent_messages_limit: Some(normalized.recent_messages_limit),
                    timezone: Some(normalized.timezone),
                    next_run_at_ms: Some(next_run_at_ms),
                },
            )
            .await?)
    }

    pub async fn sync_job_to_session(
        &self,
        session_key: &str,
        channel: &str,
        chat_id: &str,
    ) -> Result<HeartbeatJob, HeartbeatError> {
        let session_key = require_trimmed(session_key, "session_key")?;
        let channel = require_trimmed(channel, "channel")?;
        let chat_id = require_trimmed(chat_id, "chat_id")?;

        match self
            .storage
            .get_heartbeat_by_session_key(&session_key)
            .await
        {
            Ok(existing) => Ok(self
                .storage
                .update_heartbeat(
                    &existing.id,
                    &UpdateHeartbeatJobPatch {
                        session_key: Some(session_key),
                        channel: Some(channel),
                        chat_id: Some(chat_id),
                        ..UpdateHeartbeatJobPatch::default()
                    },
                )
                .await?),
            Err(_) => Ok(self
                .storage
                .create_heartbeat(&NewHeartbeatJob {
                    id: Uuid::new_v4().to_string(),
                    session_key,
                    channel,
                    chat_id,
                    enabled: true,
                    every: DEFAULT_HEARTBEAT_EVERY.to_string(),
                    prompt: String::new(),
                    silent_ack_token: DEFAULT_SILENT_ACK_TOKEN.to_string(),
                    recent_messages_limit: DEFAULT_RECENT_MESSAGES_LIMIT,
                    timezone: DEFAULT_TIMEZONE.to_string(),
                    next_run_at_ms: compute_next_run_at_ms(DEFAULT_HEARTBEAT_EVERY)?,
                })
                .await?),
        }
    }

    pub async fn set_enabled(
        &self,
        heartbeat_id: &str,
        enabled: bool,
    ) -> Result<(), HeartbeatError> {
        Ok(self
            .storage
            .set_heartbeat_enabled(heartbeat_id, enabled)
            .await?)
    }

    pub async fn delete_job(&self, heartbeat_id: &str) -> Result<(), HeartbeatError> {
        Ok(self.storage.delete_heartbeat(heartbeat_id).await?)
    }
}

#[derive(Debug, Clone)]
pub struct HeartbeatWorkerConfig {
    pub poll_interval: Duration,
    pub batch_limit: i64,
}

impl Default for HeartbeatWorkerConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            batch_limit: 64,
        }
    }
}

#[derive(Clone)]
pub struct HeartbeatWorker<S, T> {
    storage: Arc<S>,
    transport: Arc<T>,
    config: HeartbeatWorkerConfig,
}

impl<S, T> HeartbeatWorker<S, T> {
    pub fn new(storage: Arc<S>, transport: Arc<T>, config: HeartbeatWorkerConfig) -> Self {
        Self {
            storage,
            transport,
            config,
        }
    }
}

impl<S, T> HeartbeatWorker<S, T>
where
    S: HeartbeatStorage + SessionStorage + Send + Sync + 'static,
    T: MessageTransport<InboundMessage> + Send + Sync + 'static,
{
    pub async fn run_tick(&self) -> Result<usize, HeartbeatError> {
        let now = now_ms();
        let due_jobs = self
            .storage
            .list_due_heartbeats(now, self.config.batch_limit)
            .await?;
        let mut executed = 0usize;

        for job in due_jobs {
            let next_run_at_ms = next_run_after(&job.every, job.next_run_at_ms)?;
            let claimed = self
                .storage
                .claim_next_heartbeat_run(&job.id, job.next_run_at_ms, next_run_at_ms, now)
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

    pub async fn run_job_now(&self, heartbeat_id: &str) -> Result<String, HeartbeatError> {
        let job = self.storage.get_heartbeat(heartbeat_id).await?;
        self.execute_job_run(&job, now_ms()).await
    }

    pub async fn run_until_stopped(
        &self,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), HeartbeatError> {
        while !*shutdown.borrow() {
            self.run_tick().await?;
            tokio::select! {
                _ = shutdown.changed() => {}
                _ = tokio::time::sleep(self.config.poll_interval) => {}
            }
        }
        Ok(())
    }

    async fn execute_job_run(
        &self,
        job: &HeartbeatJob,
        scheduled_at_ms: i64,
    ) -> Result<String, HeartbeatError> {
        let run_id = Uuid::new_v4().to_string();
        self.storage
            .append_heartbeat_task_run(&NewHeartbeatTaskRun {
                id: run_id.clone(),
                heartbeat_id: job.id.clone(),
                scheduled_at_ms,
                status: HeartbeatTaskStatus::Pending,
                attempt: 0,
                created_at_ms: now_ms(),
            })
            .await?;
        self.storage
            .mark_heartbeat_task_running(&run_id, now_ms())
            .await?;

        match self.publish_inbound(job).await {
            Ok(message_id) => {
                self.storage
                    .mark_heartbeat_task_result(
                        &run_id,
                        HeartbeatTaskStatus::Success,
                        now_ms(),
                        None,
                        Some(&message_id),
                    )
                    .await?;
                Ok(message_id)
            }
            Err(err) => {
                self.storage
                    .mark_heartbeat_task_result(
                        &run_id,
                        HeartbeatTaskStatus::Failed,
                        now_ms(),
                        Some(&err.to_string()),
                        None,
                    )
                    .await?;
                Err(err)
            }
        }
    }

    async fn publish_inbound(&self, job: &HeartbeatJob) -> Result<String, HeartbeatError> {
        let resolved_session_key = self
            .resolve_active_session_key(&job.session_key)
            .await?
            .unwrap_or_else(|| job.session_key.clone());
        let mut payload = build_inbound_message(job);
        payload.session_key = resolved_session_key.clone();
        payload.metadata.insert(
            META_CONVERSATION_HISTORY_KEY.to_string(),
            build_conversation_history_value(
                self.storage
                    .read_chat_records(&resolved_session_key)
                    .await?,
                self.storage
                    .get_session_compression_state(&resolved_session_key)
                    .await?
                    .and_then(|state| state.summary_json),
                job.recent_messages_limit,
            )?,
        );
        payload.metadata.insert(
            HEARTBEAT_RESOLVED_SESSION_KEY.to_string(),
            Value::String(resolved_session_key.clone()),
        );

        let envelope = Envelope {
            header: EnvelopeHeader::new(resolved_session_key),
            metadata: BTreeMap::new(),
            payload,
        };
        let message_id = envelope.header.message_id.to_string();
        self.transport
            .publish(MessageTopic::Inbound.as_str(), envelope)
            .await
            .map_err(|err| HeartbeatError::Transport(err.to_string()))?;
        Ok(message_id)
    }

    async fn resolve_active_session_key(
        &self,
        session_key: &str,
    ) -> Result<Option<String>, HeartbeatError> {
        match self.storage.get_session(session_key).await {
            Ok(session) => Ok(session
                .active_session_key
                .filter(|value| !value.trim().is_empty())),
            Err(_) => Ok(None),
        }
    }
}

pub fn build_inbound_message(job: &HeartbeatJob) -> InboundMessage {
    InboundMessage {
        channel: job.channel.clone(),
        sender_id: "system-heartbeat".to_string(),
        chat_id: job.chat_id.clone(),
        session_key: job.session_key.clone(),
        content: build_heartbeat_content(&job.prompt),
        metadata: BTreeMap::from([
            (
                TRIGGER_KIND_KEY.to_string(),
                Value::String(TRIGGER_KIND_HEARTBEAT.to_string()),
            ),
            (
                HEARTBEAT_SESSION_KEY.to_string(),
                Value::String(job.session_key.clone()),
            ),
            (
                HEARTBEAT_SILENT_ACK_TOKEN_KEY.to_string(),
                Value::String(job.silent_ack_token.clone()),
            ),
            (
                HEARTBEAT_RECENT_MESSAGES_LIMIT_KEY.to_string(),
                Value::Number(job.recent_messages_limit.into()),
            ),
        ]),
        media_references: Vec::new(),
    }
}

pub fn build_payload_json(job: &HeartbeatJob) -> Result<String, HeartbeatError> {
    Ok(serde_json::to_string(&json!({
        "channel": job.channel,
        "sender_id": "system-heartbeat",
        "chat_id": job.chat_id,
        "session_key": job.session_key,
        "content": build_heartbeat_content(&job.prompt),
        "metadata": {
            TRIGGER_KIND_KEY: TRIGGER_KIND_HEARTBEAT,
            HEARTBEAT_SESSION_KEY: job.session_key,
            HEARTBEAT_SILENT_ACK_TOKEN_KEY: job.silent_ack_token,
            HEARTBEAT_RECENT_MESSAGES_LIMIT_KEY: job.recent_messages_limit,
        }
    }))?)
}

pub fn is_heartbeat_metadata(metadata: &BTreeMap<String, Value>) -> bool {
    metadata
        .get(TRIGGER_KIND_KEY)
        .and_then(Value::as_str)
        .is_some_and(|value| value == TRIGGER_KIND_HEARTBEAT)
}

pub fn should_suppress_output(content: &str, metadata: &BTreeMap<String, Value>) -> bool {
    if !is_heartbeat_metadata(metadata) {
        return false;
    }

    let Some(token) = metadata
        .get(HEARTBEAT_SILENT_ACK_TOKEN_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };

    content.trim() == token
}

fn normalize_input(input: &HeartbeatInput) -> Result<HeartbeatInput, HeartbeatError> {
    let session_key = require_trimmed(&input.session_key, "session_key")?;
    let channel = require_trimmed(&input.channel, "channel")?;
    let chat_id = require_trimmed(&input.chat_id, "chat_id")?;
    let every = require_trimmed(&input.every, "every")?;
    let prompt = input.prompt.trim().to_string();
    let silent_ack_token = require_trimmed(&input.silent_ack_token, "silent_ack_token")?;
    if input.recent_messages_limit <= 0 {
        return Err(HeartbeatError::InvalidInput(
            "recent_messages_limit must be greater than zero".to_string(),
        ));
    }
    let timezone = require_trimmed(&input.timezone, "timezone")?;
    compute_next_run_at_ms(&every)?;

    Ok(HeartbeatInput {
        id: input.id.clone(),
        session_key,
        channel,
        chat_id,
        enabled: input.enabled,
        every,
        prompt,
        silent_ack_token,
        recent_messages_limit: input.recent_messages_limit,
        timezone,
    })
}

fn build_heartbeat_content(custom_prompt: &str) -> String {
    let custom_prompt = custom_prompt.trim();
    if custom_prompt.is_empty() {
        return DEFAULT_HEARTBEAT_PROMPT.to_string();
    }
    format!("{custom_prompt}\n\n{DEFAULT_HEARTBEAT_PROMPT}")
}

fn build_conversation_history_value(
    full_history: Vec<ChatRecord>,
    summary_json: Option<String>,
    recent_messages_limit: i64,
) -> Result<Value, HeartbeatError> {
    let limit = usize::try_from(recent_messages_limit).map_err(|_| {
        HeartbeatError::InvalidInput("recent_messages_limit is out of range".to_string())
    })?;
    let history = build_history_for_model(full_history, limit, summary_json.as_deref());
    Ok(serde_json::to_value(
        history
            .into_iter()
            .map(|record| {
                json!({
                    "role": record.role,
                    "content": record.content,
                })
            })
            .collect::<Vec<_>>(),
    )?)
}

fn build_history_for_model(
    full_history: Vec<ChatRecord>,
    limit: usize,
    summary_json: Option<&str>,
) -> Vec<ChatRecord> {
    let trimmed = trim_conversation_history(full_history, limit);
    let Some(summary_json) = summary_json
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return trimmed;
    };

    let mut merged = Vec::with_capacity(trimmed.len() + 1);
    merged.push(ChatRecord::new(
        "system",
        format!("Conversation Summary (JSON): {summary_json}"),
        None,
    ));
    merged.extend(trimmed);
    merged
}

fn trim_conversation_history(
    mut conversation_history: Vec<ChatRecord>,
    limit: usize,
) -> Vec<ChatRecord> {
    if conversation_history.len() <= limit {
        return conversation_history;
    }
    let keep_from = conversation_history.len().saturating_sub(limit);
    conversation_history.split_off(keep_from)
}

fn require_trimmed(value: &str, field_name: &str) -> Result<String, HeartbeatError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(HeartbeatError::InvalidInput(format!(
            "{field_name} cannot be empty"
        )));
    }
    Ok(trimmed.to_string())
}

fn compute_next_run_at_ms(value: &str) -> Result<i64, HeartbeatError> {
    let every = humantime::parse_duration(value)
        .map_err(|err| HeartbeatError::InvalidSchedule(err.to_string()))?;
    if every.is_zero() {
        return Err(HeartbeatError::InvalidSchedule(
            "every duration must be greater than zero".to_string(),
        ));
    }
    Ok(now_ms().saturating_add(every.as_millis() as i64))
}

fn next_run_after(value: &str, after_ms: i64) -> Result<i64, HeartbeatError> {
    let every = humantime::parse_duration(value)
        .map_err(|err| HeartbeatError::InvalidSchedule(err.to_string()))?;
    if every.is_zero() {
        return Err(HeartbeatError::InvalidSchedule(
            "every duration must be greater than zero".to_string(),
        ));
    }
    Ok(after_ms.saturating_add(every.as_millis() as i64))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_core::InMemoryTransport;
    use klaw_storage::{DefaultSessionStore, HeartbeatStorage, SessionStorage, StoragePaths};
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn create_store() -> DefaultSessionStore {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("klaw-heartbeat-test-{}-{suffix}", now_ms()));
        DefaultSessionStore::open(StoragePaths::from_root(base))
            .await
            .expect("store should open")
    }

    #[test]
    fn build_payload_json_includes_heartbeat_metadata() {
        let job = HeartbeatJob {
            id: "hb-1".to_string(),
            session_key: "stdio:main".to_string(),
            channel: "stdio".to_string(),
            chat_id: "main".to_string(),
            enabled: true,
            every: "30m".to_string(),
            prompt: "Focus on stale tasks.".to_string(),
            silent_ack_token: DEFAULT_SILENT_ACK_TOKEN.to_string(),
            recent_messages_limit: DEFAULT_RECENT_MESSAGES_LIMIT,
            timezone: "UTC".to_string(),
            next_run_at_ms: 1,
            last_run_at_ms: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        };

        let payload_json = build_payload_json(&job).expect("payload");
        let payload: serde_json::Value = serde_json::from_str(&payload_json).expect("json");
        assert_eq!(
            payload["metadata"][TRIGGER_KIND_KEY],
            TRIGGER_KIND_HEARTBEAT
        );
        assert_eq!(payload["metadata"][HEARTBEAT_SESSION_KEY], "stdio:main");
        assert_eq!(
            payload["metadata"][HEARTBEAT_SILENT_ACK_TOKEN_KEY],
            DEFAULT_SILENT_ACK_TOKEN
        );
        assert_eq!(
            payload["content"],
            format!("Focus on stale tasks.\n\n{DEFAULT_HEARTBEAT_PROMPT}")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn manager_persists_heartbeat_job() {
        let store = Arc::new(create_store().await);
        let manager = HeartbeatManager::new(store.clone());
        let created = manager
            .create_job(&HeartbeatInput {
                id: None,
                session_key: "stdio:main".to_string(),
                channel: "stdio".to_string(),
                chat_id: "main".to_string(),
                enabled: true,
                every: "10m".to_string(),
                prompt: "review state".to_string(),
                silent_ack_token: DEFAULT_SILENT_ACK_TOKEN.to_string(),
                recent_messages_limit: DEFAULT_RECENT_MESSAGES_LIMIT,
                timezone: "UTC".to_string(),
            })
            .await
            .expect("create heartbeat");
        let stored = store
            .get_heartbeat_by_session_key("stdio:main")
            .await
            .expect("stored heartbeat");
        assert_eq!(stored.id, created.id);
        assert_eq!(stored.prompt, "review state");
        assert_eq!(stored.recent_messages_limit, DEFAULT_RECENT_MESSAGES_LIMIT);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sync_job_to_session_creates_default_binding() {
        let store = Arc::new(create_store().await);
        let manager = HeartbeatManager::new(store.clone());

        let job = manager
            .sync_job_to_session("telegram:main", "telegram", "chat-1")
            .await
            .expect("sync heartbeat");

        assert_eq!(job.session_key, "telegram:main");
        assert_eq!(job.every, DEFAULT_HEARTBEAT_EVERY);
        assert_eq!(job.prompt, "");
        assert_eq!(job.silent_ack_token, DEFAULT_SILENT_ACK_TOKEN);
        assert_eq!(job.recent_messages_limit, DEFAULT_RECENT_MESSAGES_LIMIT);
        assert_eq!(job.timezone, DEFAULT_TIMEZONE);
    }

    #[test]
    fn silent_ack_detection_requires_heartbeat_metadata() {
        let metadata = BTreeMap::from([
            (TRIGGER_KIND_KEY.to_string(), json!(TRIGGER_KIND_HEARTBEAT)),
            (
                HEARTBEAT_SILENT_ACK_TOKEN_KEY.to_string(),
                json!(DEFAULT_SILENT_ACK_TOKEN),
            ),
        ]);
        assert!(should_suppress_output(" HEARTBEAT_OK ", &metadata));

        let normal = BTreeMap::new();
        assert!(!should_suppress_output("HEARTBEAT_OK", &normal));
    }

    #[test]
    fn compute_next_run_rejects_zero_duration() {
        let err = compute_next_run_at_ms("0s").expect_err("zero duration should fail");
        assert!(format!("{err}").contains("greater than zero"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn worker_uses_active_session_key_when_present() {
        let store = Arc::new(create_store().await);
        store
            .get_or_create_session_state("stdio:main", "main", "stdio", "openai", "gpt-4o-mini")
            .await
            .expect("base session");
        store
            .get_or_create_session_state(
                "stdio:main:child",
                "main",
                "stdio",
                "openai",
                "gpt-4o-mini",
            )
            .await
            .expect("child session");
        store
            .set_active_session("stdio:main", "main", "stdio", "stdio:main:child")
            .await
            .expect("set active session");
        let heartbeat = store
            .create_heartbeat(&NewHeartbeatJob {
                id: "hb-1".to_string(),
                session_key: "stdio:main".to_string(),
                channel: "stdio".to_string(),
                chat_id: "main".to_string(),
                enabled: true,
                every: "1m".to_string(),
                prompt: "Keep an eye on follow-ups.".to_string(),
                silent_ack_token: DEFAULT_SILENT_ACK_TOKEN.to_string(),
                recent_messages_limit: DEFAULT_RECENT_MESSAGES_LIMIT,
                timezone: "UTC".to_string(),
                next_run_at_ms: now_ms(),
            })
            .await
            .expect("create heartbeat");
        let transport = Arc::new(InMemoryTransport::<InboundMessage>::default());
        let worker = HeartbeatWorker::new(
            store.clone(),
            transport.clone(),
            HeartbeatWorkerConfig::default(),
        );

        worker
            .run_job_now(&heartbeat.id)
            .await
            .expect("run now should succeed");

        let messages = transport.published_messages().await;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].payload.session_key, "stdio:main:child");
        assert_eq!(
            messages[0].payload.content,
            format!("Keep an eye on follow-ups.\n\n{DEFAULT_HEARTBEAT_PROMPT}")
        );
        assert_eq!(
            messages[0]
                .payload
                .metadata
                .get(HEARTBEAT_SESSION_KEY)
                .and_then(Value::as_str),
            Some("stdio:main")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn worker_injects_summary_and_recent_messages_into_history_metadata() {
        let store = Arc::new(create_store().await);
        store
            .get_or_create_session_state("stdio:main", "main", "stdio", "openai", "gpt-4o-mini")
            .await
            .expect("session");
        store
            .append_chat_record("stdio:main", &ChatRecord::new("user", "old-1", None))
            .await
            .expect("old-1");
        store
            .append_chat_record("stdio:main", &ChatRecord::new("assistant", "old-2", None))
            .await
            .expect("old-2");
        store
            .append_chat_record("stdio:main", &ChatRecord::new("user", "keep-1", None))
            .await
            .expect("keep-1");
        store
            .append_chat_record("stdio:main", &ChatRecord::new("assistant", "keep-2", None))
            .await
            .expect("keep-2");
        store
            .set_session_compression_state(
                "stdio:main",
                &klaw_storage::SessionCompressionState {
                    last_compressed_len: 2,
                    summary_json: Some("{\"important\":\"summary\"}".to_string()),
                },
            )
            .await
            .expect("summary");
        let heartbeat = store
            .create_heartbeat(&NewHeartbeatJob {
                id: "hb-history".to_string(),
                session_key: "stdio:main".to_string(),
                channel: "stdio".to_string(),
                chat_id: "main".to_string(),
                enabled: true,
                every: "1m".to_string(),
                prompt: String::new(),
                silent_ack_token: DEFAULT_SILENT_ACK_TOKEN.to_string(),
                recent_messages_limit: 2,
                timezone: "UTC".to_string(),
                next_run_at_ms: now_ms(),
            })
            .await
            .expect("create heartbeat");
        let transport = Arc::new(InMemoryTransport::<InboundMessage>::default());
        let worker = HeartbeatWorker::new(
            store.clone(),
            transport.clone(),
            HeartbeatWorkerConfig::default(),
        );

        worker
            .run_job_now(&heartbeat.id)
            .await
            .expect("run now should succeed");

        let messages = transport.published_messages().await;
        let history = messages[0]
            .payload
            .metadata
            .get(META_CONVERSATION_HISTORY_KEY)
            .and_then(Value::as_array)
            .expect("history metadata should exist");
        assert_eq!(history.len(), 3);
        assert_eq!(
            history[0]["content"].as_str(),
            Some("Conversation Summary (JSON): {\"important\":\"summary\"}")
        );
        assert_eq!(history[1]["content"].as_str(), Some("keep-1"));
        assert_eq!(history[2]["content"].as_str(), Some("keep-2"));
    }
}
