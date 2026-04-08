use crate::ApprovalError;
use async_trait::async_trait;
use klaw_storage::{
    ApprovalRecord, ApprovalStatus, DbRow, DbValue, DefaultSessionStore, MemoryDb,
    NewApprovalRecord, SessionStorage, open_default_store,
};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

const DEFAULT_EXPIRES_IN_MINUTES: i64 = 10;
const MAX_EXPIRES_IN_MINUTES: i64 = 7 * 24 * 60;

#[derive(Debug, Clone)]
pub struct ApprovalCreateInput {
    pub session_key: String,
    pub tool_name: String,
    pub command_text: String,
    pub command_preview: Option<String>,
    pub command_hash: Option<String>,
    pub risk_level: Option<String>,
    pub requested_by: Option<String>,
    pub justification: Option<String>,
    pub expires_in_minutes: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
pub enum ApprovalResolveDecision {
    Approve,
    Reject,
}

#[derive(Debug, Clone)]
pub struct ApprovalResolveOutcome {
    pub approval: ApprovalRecord,
    pub updated: bool,
}

#[derive(Debug, Clone)]
pub struct ApprovalListQuery {
    pub session_key: Option<String>,
    pub tool_name: Option<String>,
    pub status: Option<ApprovalStatus>,
    pub preview_filter: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

impl Default for ApprovalListQuery {
    fn default() -> Self {
        Self {
            session_key: None,
            tool_name: None,
            status: None,
            preview_filter: None,
            limit: 100,
            offset: 0,
        }
    }
}

#[async_trait]
pub trait ApprovalManager: Send + Sync {
    async fn create_approval(
        &self,
        input: ApprovalCreateInput,
    ) -> Result<ApprovalRecord, ApprovalError>;

    async fn get_approval(&self, approval_id: &str) -> Result<ApprovalRecord, ApprovalError>;

    async fn list_approvals(
        &self,
        query: ApprovalListQuery,
    ) -> Result<Vec<ApprovalRecord>, ApprovalError>;

    async fn resolve_approval(
        &self,
        approval_id: &str,
        decision: ApprovalResolveDecision,
        actor: Option<&str>,
        now_ms: i64,
    ) -> Result<ApprovalResolveOutcome, ApprovalError>;

    async fn consume_shell_approval(
        &self,
        approval_id: &str,
        session_key: &str,
        command_hash: &str,
        now_ms: i64,
    ) -> Result<bool, ApprovalError>;

    async fn consume_latest_shell_approval(
        &self,
        session_key: &str,
        command_hash: &str,
        now_ms: i64,
    ) -> Result<bool, ApprovalError>;

    async fn consume_approval(
        &self,
        approval_id: &str,
        now_ms: i64,
    ) -> Result<ApprovalResolveOutcome, ApprovalError>;

    async fn list_session_keys(&self) -> Result<Vec<String>, ApprovalError>;

    async fn list_tool_names(&self) -> Result<Vec<String>, ApprovalError>;
}

#[derive(Clone)]
pub struct SqliteApprovalManager {
    store: DefaultSessionStore,
}

impl SqliteApprovalManager {
    pub async fn open_default() -> Result<Self, ApprovalError> {
        let store = open_default_store().await?;
        Ok(Self { store })
    }

    pub fn from_store(store: DefaultSessionStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl ApprovalManager for SqliteApprovalManager {
    async fn create_approval(
        &self,
        input: ApprovalCreateInput,
    ) -> Result<ApprovalRecord, ApprovalError> {
        let tool_name = normalize_non_empty(&input.tool_name, "tool_name")?;
        let session_key = normalize_non_empty(&input.session_key, "session_key")?;
        let command_text = normalize_non_empty(&input.command_text, "command_text")?;
        let command_preview = match input.command_preview {
            Some(preview) => normalize_non_empty(&preview, "command_preview")?,
            None => command_preview(&command_text),
        };
        let command_hash = match input.command_hash {
            Some(hash) => normalize_non_empty(&hash, "command_hash")?,
            None => command_hash(&command_text),
        };
        let risk_level = input
            .risk_level
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("mutating")
            .to_string();
        let requested_by = input
            .requested_by
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("agent")
            .to_string();
        let expires_in_minutes = input
            .expires_in_minutes
            .unwrap_or(DEFAULT_EXPIRES_IN_MINUTES);
        if !(1..=MAX_EXPIRES_IN_MINUTES).contains(&expires_in_minutes) {
            return Err(ApprovalError::InvalidArgs(format!(
                "`expires_in_minutes` must be between 1 and {MAX_EXPIRES_IN_MINUTES}"
            )));
        }

        let now_ms = now_ms();
        Ok(self
            .store
            .create_approval(&NewApprovalRecord {
                id: Uuid::new_v4().to_string(),
                session_key,
                tool_name,
                command_hash,
                command_preview,
                command_text,
                risk_level,
                requested_by,
                justification: input.justification,
                expires_at_ms: now_ms + expires_in_minutes * 60_000,
            })
            .await?)
    }

    async fn get_approval(&self, approval_id: &str) -> Result<ApprovalRecord, ApprovalError> {
        let approval_id = normalize_non_empty(approval_id, "approval_id")?;
        Ok(self.store.get_approval(&approval_id).await?)
    }

    async fn list_approvals(
        &self,
        query: ApprovalListQuery,
    ) -> Result<Vec<ApprovalRecord>, ApprovalError> {
        let mut filters = Vec::new();
        let mut params = Vec::new();

        if let Some(session_key) = query
            .session_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push(format!("session_key = ?{}", params.len() + 1));
            params.push(DbValue::Text(session_key.to_string()));
        }
        if let Some(tool_name) = query
            .tool_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push(format!("tool_name = ?{}", params.len() + 1));
            params.push(DbValue::Text(tool_name.to_string()));
        }
        if let Some(status) = query.status {
            filters.push(format!("status = ?{}", params.len() + 1));
            params.push(DbValue::Text(status.as_str().to_string()));
        }
        if let Some(preview_filter) = query
            .preview_filter
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push(format!("command_preview LIKE ?{}", params.len() + 1));
            params.push(DbValue::Text(format!("%{preview_filter}%")));
        }

        let where_clause = if filters.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", filters.join(" AND "))
        };
        let limit = query.limit.max(1);
        let offset = query.offset.max(0);
        let sql = format!(
            "SELECT id, session_key, tool_name, command_hash, command_preview, command_text, risk_level, status,\
                    requested_by, approved_by, justification, expires_at_ms, created_at_ms, updated_at_ms, consumed_at_ms \
             FROM approvals{where_clause} \
             ORDER BY created_at_ms DESC \
             LIMIT {limit} OFFSET {offset}"
        );

        let rows = self.store.query(&sql, &params).await?;
        rows.into_iter().map(row_to_approval).collect()
    }

    async fn resolve_approval(
        &self,
        approval_id: &str,
        decision: ApprovalResolveDecision,
        actor: Option<&str>,
        now_ms: i64,
    ) -> Result<ApprovalResolveOutcome, ApprovalError> {
        let approval = self.get_approval(approval_id).await?;
        if approval.status != ApprovalStatus::Pending {
            return Ok(ApprovalResolveOutcome {
                approval,
                updated: false,
            });
        }

        if approval.expires_at_ms < now_ms {
            let updated = self
                .store
                .update_approval_status(
                    &approval.id,
                    ApprovalStatus::Expired,
                    Some("approval-manager"),
                )
                .await?;
            return Ok(ApprovalResolveOutcome {
                approval: updated,
                updated: true,
            });
        }

        let actor = actor
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("channel-user");
        let status = match decision {
            ApprovalResolveDecision::Approve => ApprovalStatus::Approved,
            ApprovalResolveDecision::Reject => ApprovalStatus::Rejected,
        };
        let updated = self
            .store
            .update_approval_status(&approval.id, status, Some(actor))
            .await?;
        Ok(ApprovalResolveOutcome {
            approval: updated,
            updated: true,
        })
    }

    async fn consume_shell_approval(
        &self,
        approval_id: &str,
        session_key: &str,
        command_hash: &str,
        now_ms: i64,
    ) -> Result<bool, ApprovalError> {
        let approval_id = normalize_non_empty(approval_id, "approval_id")?;
        let session_key = normalize_non_empty(session_key, "session_key")?;
        let command_hash = normalize_non_empty(command_hash, "command_hash")?;
        Ok(self
            .store
            .consume_approved_shell_command(&approval_id, &session_key, &command_hash, now_ms)
            .await?)
    }

    async fn consume_latest_shell_approval(
        &self,
        session_key: &str,
        command_hash: &str,
        now_ms: i64,
    ) -> Result<bool, ApprovalError> {
        let session_key = normalize_non_empty(session_key, "session_key")?;
        let command_hash = normalize_non_empty(command_hash, "command_hash")?;
        Ok(self
            .store
            .consume_latest_approved_shell_command(&session_key, &command_hash, now_ms)
            .await?)
    }

    async fn consume_approval(
        &self,
        approval_id: &str,
        now_ms: i64,
    ) -> Result<ApprovalResolveOutcome, ApprovalError> {
        let approval = self.get_approval(approval_id).await?;
        if approval.tool_name != "shell" {
            return Err(ApprovalError::NotShellApproval(approval.id));
        }
        let updated = self
            .store
            .consume_approved_shell_command(
                &approval.id,
                &approval.session_key,
                &approval.command_hash,
                now_ms,
            )
            .await?;
        let approval = self.get_approval(&approval.id).await?;
        Ok(ApprovalResolveOutcome { approval, updated })
    }

    async fn list_session_keys(&self) -> Result<Vec<String>, ApprovalError> {
        let sql = "SELECT DISTINCT session_key FROM approvals ORDER BY session_key";
        let rows = self.store.query(sql, &[]).await?;
        rows.into_iter()
            .map(|row| {
                row.get(0)
                    .and_then(|v| match v {
                        DbValue::Text(s) => Some(s.clone()),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        ApprovalError::InvalidApprovalRow("missing session_key".to_string())
                    })
            })
            .collect()
    }

    async fn list_tool_names(&self) -> Result<Vec<String>, ApprovalError> {
        let sql = "SELECT DISTINCT tool_name FROM approvals ORDER BY tool_name";
        let rows = self.store.query(sql, &[]).await?;
        rows.into_iter()
            .map(|row| {
                row.get(0)
                    .and_then(|v| match v {
                        DbValue::Text(s) => Some(s.clone()),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        ApprovalError::InvalidApprovalRow("missing tool_name".to_string())
                    })
            })
            .collect()
    }
}

fn now_ms() -> i64 {
    (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

fn command_hash(command: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(command.trim().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn command_preview(command: &str) -> String {
    let trimmed = command.trim();
    let max = 160;
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let mut preview = trimmed.chars().take(max).collect::<String>();
    preview.push_str("...");
    preview
}

fn normalize_non_empty(value: &str, field: &str) -> Result<String, ApprovalError> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(ApprovalError::InvalidArgs(format!(
            "`{field}` cannot be empty"
        )));
    }
    Ok(normalized.to_string())
}

fn row_to_approval(row: DbRow) -> Result<ApprovalRecord, ApprovalError> {
    let status_raw = row_text(&row, 7)?;
    let status = ApprovalStatus::parse(&status_raw).ok_or_else(|| {
        ApprovalError::InvalidApprovalRow(format!("invalid status: {status_raw}"))
    })?;
    Ok(ApprovalRecord {
        id: row_text(&row, 0)?,
        session_key: row_text(&row, 1)?,
        tool_name: row_text(&row, 2)?,
        command_hash: row_text(&row, 3)?,
        command_preview: row_text(&row, 4)?,
        command_text: row_text(&row, 5)?,
        risk_level: row_text(&row, 6)?,
        status,
        requested_by: row_text(&row, 8)?,
        approved_by: row_opt_text(&row, 9)?,
        justification: row_opt_text(&row, 10)?,
        expires_at_ms: row_i64(&row, 11)?,
        created_at_ms: row_i64(&row, 12)?,
        updated_at_ms: row_i64(&row, 13)?,
        consumed_at_ms: row_opt_i64(&row, 14)?,
    })
}

fn row_text(row: &DbRow, index: usize) -> Result<String, ApprovalError> {
    match row.get(index) {
        Some(DbValue::Text(value)) => Ok(value.clone()),
        Some(DbValue::Integer(value)) => Ok(value.to_string()),
        Some(DbValue::Null) | None => Err(ApprovalError::InvalidApprovalRow(format!(
            "missing text at column {index}"
        ))),
        Some(other) => Err(ApprovalError::InvalidApprovalRow(format!(
            "unexpected value at column {index}: {other:?}"
        ))),
    }
}

fn row_opt_text(row: &DbRow, index: usize) -> Result<Option<String>, ApprovalError> {
    match row.get(index) {
        Some(DbValue::Null) | None => Ok(None),
        Some(DbValue::Text(value)) => Ok(Some(value.clone())),
        Some(DbValue::Integer(value)) => Ok(Some(value.to_string())),
        Some(other) => Err(ApprovalError::InvalidApprovalRow(format!(
            "unexpected value at column {index}: {other:?}"
        ))),
    }
}

fn row_i64(row: &DbRow, index: usize) -> Result<i64, ApprovalError> {
    match row.get(index) {
        Some(DbValue::Integer(value)) => Ok(*value),
        Some(DbValue::Text(value)) => value.parse::<i64>().map_err(|_| {
            ApprovalError::InvalidApprovalRow(format!("invalid integer text at column {index}"))
        }),
        Some(DbValue::Null) | None => Err(ApprovalError::InvalidApprovalRow(format!(
            "missing integer at column {index}"
        ))),
        Some(other) => Err(ApprovalError::InvalidApprovalRow(format!(
            "unexpected value at column {index}: {other:?}"
        ))),
    }
}

fn row_opt_i64(row: &DbRow, index: usize) -> Result<Option<i64>, ApprovalError> {
    match row.get(index) {
        Some(DbValue::Null) | None => Ok(None),
        Some(DbValue::Integer(value)) => Ok(Some(*value)),
        Some(DbValue::Text(value)) => value.parse::<i64>().map(Some).map_err(|_| {
            ApprovalError::InvalidApprovalRow(format!("invalid integer text at column {index}"))
        }),
        Some(other) => Err(ApprovalError::InvalidApprovalRow(format!(
            "unexpected value at column {index}: {other:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ApprovalCreateInput, ApprovalListQuery, ApprovalManager, ApprovalResolveDecision,
        SqliteApprovalManager, now_ms,
    };
    use klaw_storage::{ApprovalStatus, DefaultSessionStore, SessionStorage, StoragePaths};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn create_store() -> DefaultSessionStore {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("klaw-approval-test-{now_ms}-{suffix}"));
        DefaultSessionStore::open(StoragePaths::from_root(root))
            .await
            .expect("session store should open")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_approvals_returns_latest_first() {
        let store = create_store().await;
        store
            .touch_session("terminal:test", "chat-1", "terminal")
            .await
            .expect("session should exist");
        let manager = SqliteApprovalManager::from_store(store);
        let _ = manager
            .create_approval(ApprovalCreateInput {
                session_key: "terminal:test".to_string(),
                tool_name: "shell".to_string(),
                command_text: "touch a".to_string(),
                command_preview: None,
                command_hash: None,
                risk_level: None,
                requested_by: None,
                justification: None,
                expires_in_minutes: Some(10),
            })
            .await
            .expect("first approval should be created");
        let second = manager
            .create_approval(ApprovalCreateInput {
                session_key: "terminal:test".to_string(),
                tool_name: "shell".to_string(),
                command_text: "touch b".to_string(),
                command_preview: None,
                command_hash: None,
                risk_level: None,
                requested_by: None,
                justification: None,
                expires_in_minutes: Some(10),
            })
            .await
            .expect("second approval should be created");

        let approvals = manager
            .list_approvals(ApprovalListQuery::default())
            .await
            .expect("approvals should load");
        assert_eq!(approvals[0].id, second.id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_approval_marks_record_approved() {
        let store = create_store().await;
        store
            .touch_session("terminal:test", "chat-1", "terminal")
            .await
            .expect("session should exist");
        let manager = SqliteApprovalManager::from_store(store);
        let approval = manager
            .create_approval(ApprovalCreateInput {
                session_key: "terminal:test".to_string(),
                tool_name: "shell".to_string(),
                command_text: "touch a".to_string(),
                command_preview: None,
                command_hash: None,
                risk_level: None,
                requested_by: None,
                justification: None,
                expires_in_minutes: Some(10),
            })
            .await
            .expect("approval should be created");

        let outcome = manager
            .resolve_approval(
                &approval.id,
                ApprovalResolveDecision::Approve,
                Some("tester"),
                now_ms(),
            )
            .await
            .expect("approval should resolve");
        assert!(outcome.updated);
        assert_eq!(outcome.approval.status, ApprovalStatus::Approved);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn consume_approval_marks_record_consumed() {
        let store = create_store().await;
        store
            .touch_session("terminal:test", "chat-1", "terminal")
            .await
            .expect("session should exist");
        let manager = SqliteApprovalManager::from_store(store);
        let approval = manager
            .create_approval(ApprovalCreateInput {
                session_key: "terminal:test".to_string(),
                tool_name: "shell".to_string(),
                command_text: "touch a".to_string(),
                command_preview: None,
                command_hash: None,
                risk_level: None,
                requested_by: None,
                justification: None,
                expires_in_minutes: Some(10),
            })
            .await
            .expect("approval should be created");
        let _ = manager
            .resolve_approval(
                &approval.id,
                ApprovalResolveDecision::Approve,
                Some("tester"),
                now_ms(),
            )
            .await
            .expect("approval should resolve");

        let outcome = manager
            .consume_approval(&approval.id, now_ms())
            .await
            .expect("approval should consume");
        assert!(outcome.updated);
        assert_eq!(outcome.approval.status, ApprovalStatus::Consumed);
    }
}
