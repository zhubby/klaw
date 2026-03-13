use async_trait::async_trait;
use klaw_config::{AppConfig, HeartbeatConfig, HeartbeatSessionConfig};
use klaw_storage::{CronScheduleKind, CronStorage, NewCronJob, StorageError, UpdateCronJobPatch};
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Arc};
use thiserror::Error;

pub const TRIGGER_KIND_KEY: &str = "trigger.kind";
pub const TRIGGER_KIND_HEARTBEAT: &str = "heartbeat";
pub const HEARTBEAT_SESSION_KEY: &str = "heartbeat.session_key";
pub const HEARTBEAT_SILENT_ACK_TOKEN_KEY: &str = "heartbeat.silent_ack_token";
pub const DEFAULT_SILENT_ACK_TOKEN: &str = "HEARTBEAT_OK";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatSpec {
    pub session_key: String,
    pub channel: String,
    pub chat_id: String,
    pub enabled: bool,
    pub every: String,
    pub prompt: String,
    pub silent_ack_token: String,
    pub timezone: String,
}

#[derive(Debug, Error)]
pub enum HeartbeatError {
    #[error("invalid heartbeat config: {0}")]
    InvalidConfig(String),
    #[error("invalid heartbeat schedule: {0}")]
    InvalidSchedule(String),
    #[error("failed to serialize heartbeat payload: {0}")]
    SerializePayload(#[from] serde_json::Error),
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
}

#[async_trait]
pub trait HeartbeatScheduler: Send + Sync {
    async fn reconcile(&self, specs: &[HeartbeatSpec]) -> Result<(), HeartbeatError>;
}

pub struct CronHeartbeatScheduler<S> {
    storage: Arc<S>,
}

impl<S> CronHeartbeatScheduler<S> {
    pub fn new(storage: Arc<S>) -> Self {
        Self { storage }
    }
}

#[async_trait]
impl<S> HeartbeatScheduler for CronHeartbeatScheduler<S>
where
    S: CronStorage + Send + Sync + 'static,
{
    async fn reconcile(&self, specs: &[HeartbeatSpec]) -> Result<(), HeartbeatError> {
        for spec in specs {
            reconcile_one(self.storage.as_ref(), spec).await?;
        }
        Ok(())
    }
}

pub fn specs_from_config(config: &AppConfig) -> Result<Vec<HeartbeatSpec>, HeartbeatError> {
    config
        .heartbeat
        .sessions
        .iter()
        .map(|session| resolve_session_spec(&config.heartbeat, session))
        .collect()
}

pub fn heartbeat_cron_id(session_key: &str) -> String {
    format!("heartbeat:{session_key}")
}

pub fn build_payload_json(spec: &HeartbeatSpec) -> Result<String, HeartbeatError> {
    Ok(serde_json::to_string(&json!({
        "channel": spec.channel,
        "sender_id": "system-heartbeat",
        "chat_id": spec.chat_id,
        "session_key": spec.session_key,
        "content": spec.prompt,
        "metadata": {
            TRIGGER_KIND_KEY: TRIGGER_KIND_HEARTBEAT,
            HEARTBEAT_SESSION_KEY: spec.session_key,
            HEARTBEAT_SILENT_ACK_TOKEN_KEY: spec.silent_ack_token,
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

async fn reconcile_one<S>(storage: &S, spec: &HeartbeatSpec) -> Result<(), HeartbeatError>
where
    S: CronStorage + Send + Sync + 'static,
{
    let cron_id = heartbeat_cron_id(&spec.session_key);
    let payload_json = build_payload_json(spec)?;
    let next_run_at_ms = compute_next_run_at_ms(&spec.every)?;
    match storage.get_cron(&cron_id).await {
        Ok(current) => {
            let mut patch = UpdateCronJobPatch {
                name: Some(cron_id.clone()),
                schedule_kind: None,
                schedule_expr: None,
                payload_json: None,
                timezone: None,
                next_run_at_ms: None,
            };

            if current.schedule_kind != CronScheduleKind::Every {
                patch.schedule_kind = Some(CronScheduleKind::Every);
            }
            if current.schedule_expr != spec.every {
                patch.schedule_expr = Some(spec.every.clone());
                patch.next_run_at_ms = Some(next_run_at_ms);
            }
            if current.payload_json != payload_json {
                patch.payload_json = Some(payload_json);
            }
            if current.timezone != spec.timezone {
                patch.timezone = Some(spec.timezone.clone());
            }
            if current.name != cron_id {
                patch.name = Some(cron_id.clone());
            }
            if patch.schedule_kind.is_some()
                || patch.schedule_expr.is_some()
                || patch.payload_json.is_some()
                || patch.timezone.is_some()
                || patch.next_run_at_ms.is_some()
                || current.name != cron_id
            {
                storage.update_cron(&cron_id, &patch).await?;
            }
            if current.enabled != spec.enabled {
                storage.set_enabled(&cron_id, spec.enabled).await?;
            }
        }
        Err(err) if is_missing_cron_error(&err) => {
            storage
                .create_cron(&NewCronJob {
                    id: cron_id.clone(),
                    name: cron_id,
                    schedule_kind: CronScheduleKind::Every,
                    schedule_expr: spec.every.clone(),
                    payload_json,
                    enabled: spec.enabled,
                    timezone: spec.timezone.clone(),
                    next_run_at_ms,
                })
                .await?;
        }
        Err(err) => return Err(err.into()),
    }

    Ok(())
}

fn resolve_session_spec(
    heartbeat: &HeartbeatConfig,
    session: &HeartbeatSessionConfig,
) -> Result<HeartbeatSpec, HeartbeatError> {
    let defaults = &heartbeat.defaults;
    let enabled = session.enabled.unwrap_or(defaults.enabled);
    let every = resolve_text_field(
        session.every.as_deref(),
        &defaults.every,
        "heartbeat.sessions.every",
    )?;
    let prompt = resolve_text_field(
        session.prompt.as_deref(),
        &defaults.prompt,
        "heartbeat.sessions.prompt",
    )?;
    let silent_ack_token = resolve_text_field(
        session.silent_ack_token.as_deref(),
        &defaults.silent_ack_token,
        "heartbeat.sessions.silent_ack_token",
    )?;
    let timezone = resolve_text_field(
        session.timezone.as_deref(),
        &defaults.timezone,
        "heartbeat.sessions.timezone",
    )?;
    if enabled {
        compute_next_run_at_ms(&every)?;
    }

    Ok(HeartbeatSpec {
        session_key: require_trimmed(&session.session_key, "heartbeat.sessions.session_key")?,
        channel: require_trimmed(&session.channel, "heartbeat.sessions.channel")?,
        chat_id: require_trimmed(&session.chat_id, "heartbeat.sessions.chat_id")?,
        enabled,
        every,
        prompt,
        silent_ack_token,
        timezone,
    })
}

fn resolve_text_field(
    override_value: Option<&str>,
    default_value: &str,
    field_name: &str,
) -> Result<String, HeartbeatError> {
    match override_value {
        Some(value) => require_trimmed(value, field_name),
        None => require_trimmed(default_value, field_name),
    }
}

fn require_trimmed(value: &str, field_name: &str) -> Result<String, HeartbeatError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(HeartbeatError::InvalidConfig(format!(
            "{field_name} cannot be empty"
        )));
    }
    Ok(trimmed.to_string())
}

fn compute_next_run_at_ms(expr: &str) -> Result<i64, HeartbeatError> {
    let parsed = humantime::parse_duration(expr)
        .map_err(|err| HeartbeatError::InvalidSchedule(err.to_string()))?;
    if parsed.is_zero() {
        return Err(HeartbeatError::InvalidSchedule(
            "every duration must be greater than zero".to_string(),
        ));
    }
    Ok(now_ms().saturating_add(parsed.as_millis() as i64))
}

fn is_missing_cron_error(err: &StorageError) -> bool {
    matches!(err, StorageError::Backend(message) if message.contains("not found") || message.contains("no rows"))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_config::{AppConfig, HeartbeatSessionConfig};
    use klaw_storage::StoragePaths;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn create_store() -> klaw_storage::DefaultSessionStore {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("klaw-heartbeat-test-{suffix}"));
        klaw_storage::DefaultSessionStore::open(StoragePaths::from_root(base))
            .await
            .expect("store should open")
    }

    #[test]
    fn specs_from_config_applies_defaults() {
        let mut config = AppConfig::default();
        config.heartbeat.sessions = vec![HeartbeatSessionConfig {
            session_key: "stdio:main".to_string(),
            chat_id: "main".to_string(),
            channel: "stdio".to_string(),
            enabled: None,
            every: None,
            prompt: None,
            silent_ack_token: None,
            timezone: None,
        }];

        let specs = specs_from_config(&config).expect("specs should resolve");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].session_key, "stdio:main");
        assert_eq!(specs[0].every, "30m");
        assert_eq!(specs[0].silent_ack_token, DEFAULT_SILENT_ACK_TOKEN);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reconcile_creates_and_updates_heartbeat_cron() {
        let store = create_store().await;
        let scheduler = CronHeartbeatScheduler::new(Arc::new(store.clone()));
        let spec = HeartbeatSpec {
            session_key: "stdio:test".to_string(),
            channel: "stdio".to_string(),
            chat_id: "test".to_string(),
            enabled: true,
            every: "30s".to_string(),
            prompt: "ping".to_string(),
            silent_ack_token: DEFAULT_SILENT_ACK_TOKEN.to_string(),
            timezone: "UTC".to_string(),
        };

        scheduler
            .reconcile(std::slice::from_ref(&spec))
            .await
            .expect("create should succeed");
        let created = store
            .get_cron(&heartbeat_cron_id(&spec.session_key))
            .await
            .expect("created cron should exist");
        assert_eq!(created.schedule_expr, "30s");
        assert!(created.enabled);

        let mut updated_spec = spec.clone();
        updated_spec.every = "45s".to_string();
        updated_spec.enabled = false;
        scheduler
            .reconcile(std::slice::from_ref(&updated_spec))
            .await
            .expect("update should succeed");
        let updated = store
            .get_cron(&heartbeat_cron_id(&updated_spec.session_key))
            .await
            .expect("updated cron should exist");
        assert_eq!(updated.schedule_expr, "45s");
        assert!(!updated.enabled);
    }

    #[test]
    fn suppress_output_only_for_exact_silent_ack() {
        let metadata = BTreeMap::from([
            (
                TRIGGER_KIND_KEY.to_string(),
                Value::String(TRIGGER_KIND_HEARTBEAT.to_string()),
            ),
            (
                HEARTBEAT_SILENT_ACK_TOKEN_KEY.to_string(),
                Value::String(DEFAULT_SILENT_ACK_TOKEN.to_string()),
            ),
        ]);

        assert!(should_suppress_output("  HEARTBEAT_OK \n", &metadata));
        assert!(!should_suppress_output("HEARTBEAT_OK extra", &metadata));
    }
}
