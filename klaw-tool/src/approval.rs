use async_trait::async_trait;
use klaw_storage::{
    ApprovalRecord, ApprovalStatus, DefaultSessionStore, NewApprovalRecord, SessionStorage,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

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

#[derive(Clone)]
pub struct ApprovalStoreService {
    store: DefaultSessionStore,
}

pub struct ApprovalTool {
    store: DefaultSessionStore,
}

impl ApprovalStoreService {
    pub fn new(store: DefaultSessionStore) -> Self {
        Self { store }
    }

    pub async fn create(&self, input: ApprovalCreateInput) -> Result<ApprovalRecord, ToolError> {
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
            return Err(ToolError::InvalidArgs(format!(
                "`expires_in_minutes` must be between 1 and {MAX_EXPIRES_IN_MINUTES}"
            )));
        }

        let now_ms = now_ms();
        self.store
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
            .await
            .map_err(|err| ToolError::ExecutionFailed(format!("failed to create approval: {err}")))
    }

    pub async fn get_for_session(
        &self,
        session_key: &str,
        approval_id: &str,
    ) -> Result<ApprovalRecord, ToolError> {
        let normalized_session = normalize_non_empty(session_key, "session_key")?;
        let normalized_id = normalize_non_empty(approval_id, "approval_id")?;
        let approval = self
            .store
            .get_approval(&normalized_id)
            .await
            .map_err(|err| ToolError::ExecutionFailed(format!("failed to get approval: {err}")))?;
        if approval.session_key != normalized_session {
            return Err(ToolError::ExecutionFailed(
                "approval does not belong to current session".to_string(),
            ));
        }
        Ok(approval)
    }

    pub async fn resolve_for_session(
        &self,
        session_key: &str,
        approval_id: &str,
        decision: ApprovalResolveDecision,
        actor: Option<&str>,
    ) -> Result<(ApprovalRecord, bool), ToolError> {
        let approval = self.get_for_session(session_key, approval_id).await?;
        if approval.status != ApprovalStatus::Pending {
            return Ok((approval, false));
        }

        let now = now_ms();
        if approval.expires_at_ms < now {
            let updated = self
                .store
                .update_approval_status(
                    &approval.id,
                    ApprovalStatus::Expired,
                    Some("approval-tool"),
                )
                .await
                .map_err(|err| {
                    ToolError::ExecutionFailed(format!("failed to mark approval expired: {err}"))
                })?;
            return Ok((updated, true));
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
            .await
            .map_err(|err| {
                ToolError::ExecutionFailed(format!("failed to resolve approval status: {err}"))
            })?;
        Ok((updated, true))
    }
}

impl ApprovalTool {
    pub fn with_store(store: DefaultSessionStore) -> Self {
        Self { store }
    }

    fn format_output(response: &ApprovalToolResponse) -> Result<String, ToolError> {
        serde_json::to_string_pretty(response)
            .map_err(|err| ToolError::ExecutionFailed(format!("failed to serialize output: {err}")))
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

fn normalize_non_empty(value: &str, field: &str) -> Result<String, ToolError> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(ToolError::InvalidArgs(format!("`{field}` cannot be empty")));
    }
    Ok(normalized.to_string())
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum ApprovalRequest {
    Request(ApprovalRequestCreate),
    Get(ApprovalRequestGet),
    Resolve(ApprovalRequestResolve),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalRequestCreate {
    tool_name: String,
    command_text: String,
    #[serde(default)]
    command_preview: Option<String>,
    #[serde(default)]
    command_hash: Option<String>,
    #[serde(default)]
    risk_level: Option<String>,
    #[serde(default)]
    requested_by: Option<String>,
    #[serde(default)]
    justification: Option<String>,
    #[serde(default)]
    expires_in_minutes: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalRequestGet {
    approval_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalRequestResolve {
    approval_id: String,
    decision: ApprovalDecision,
    #[serde(default)]
    actor: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ApprovalDecision {
    Approve,
    Reject,
}

#[derive(Debug, Serialize)]
struct ApprovalToolResponse {
    action: &'static str,
    updated: bool,
    approval: ApprovalRecord,
}

#[async_trait]
impl Tool for ApprovalTool {
    fn name(&self) -> &str {
        "approval"
    }

    fn description(&self) -> &str {
        "Manage persisted approval records for high-risk actions (request approval, check status, and resolve approve/reject decisions)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Approval workflow actions backed by session storage.",
            "oneOf": [
                {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "const": "request" },
                        "tool_name": {
                            "type": "string",
                            "description": "Tool that requires approval, e.g. `shell`."
                        },
                        "command_text": {
                            "type": "string",
                            "description": "Full operation text requiring approval."
                        },
                        "command_preview": {
                            "type": "string",
                            "description": "Optional short preview shown in approval UI."
                        },
                        "command_hash": {
                            "type": "string",
                            "description": "Optional stable hash; defaults to sha256(command_text)."
                        },
                        "risk_level": {
                            "type": "string",
                            "description": "Risk label such as `mutating` or `destructive`.",
                            "default": "mutating"
                        },
                        "requested_by": {
                            "type": "string",
                            "description": "Who requested this approval.",
                            "default": "agent"
                        },
                        "justification": {
                            "type": "string",
                            "description": "Optional reason displayed to approvers."
                        },
                        "expires_in_minutes": {
                            "type": "integer",
                            "description": "Approval TTL in minutes.",
                            "minimum": 1,
                            "maximum": 10080,
                            "default": 10
                        }
                    },
                    "required": ["action", "tool_name", "command_text"],
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "const": "get" },
                        "approval_id": {
                            "type": "string",
                            "description": "Approval ID to inspect."
                        }
                    },
                    "required": ["action", "approval_id"],
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "const": "resolve" },
                        "approval_id": {
                            "type": "string",
                            "description": "Approval ID to resolve."
                        },
                        "decision": {
                            "type": "string",
                            "enum": ["approve", "reject"],
                            "description": "Resolution decision."
                        },
                        "actor": {
                            "type": "string",
                            "description": "Who resolved the approval.",
                            "default": "channel-user"
                        }
                    },
                    "required": ["action", "approval_id", "decision"],
                    "additionalProperties": false
                }
            ]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Messaging
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let request: ApprovalRequest = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("invalid request: {err}")))?;
        let service = ApprovalStoreService::new(self.store.clone());

        let response = match request {
            ApprovalRequest::Request(input) => {
                let created = service
                    .create(ApprovalCreateInput {
                        session_key: ctx.session_key.clone(),
                        tool_name: input.tool_name,
                        command_text: input.command_text,
                        command_preview: input.command_preview,
                        command_hash: input.command_hash,
                        risk_level: input.risk_level,
                        requested_by: input.requested_by,
                        justification: input.justification,
                        expires_in_minutes: input.expires_in_minutes,
                    })
                    .await?;
                ApprovalToolResponse {
                    action: "request",
                    updated: true,
                    approval: created,
                }
            }
            ApprovalRequest::Get(input) => {
                let approval = service
                    .get_for_session(&ctx.session_key, &input.approval_id)
                    .await?;
                ApprovalToolResponse {
                    action: "get",
                    updated: false,
                    approval,
                }
            }
            ApprovalRequest::Resolve(input) => {
                let (final_record, updated) = service
                    .resolve_for_session(
                        &ctx.session_key,
                        &input.approval_id,
                        match input.decision {
                            ApprovalDecision::Approve => ApprovalResolveDecision::Approve,
                            ApprovalDecision::Reject => ApprovalResolveDecision::Reject,
                        },
                        input.actor.as_deref(),
                    )
                    .await?;

                ApprovalToolResponse {
                    action: "resolve",
                    updated,
                    approval: final_record,
                }
            }
        };

        let content = Self::format_output(&response)?;
        Ok(ToolOutput {
            content_for_model: content.clone(),
            content_for_user: Some(content),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolContext;
    use klaw_storage::{SessionStorage, StoragePaths};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn create_store() -> DefaultSessionStore {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("klaw-approval-tool-test-{}-{suffix}", now_ms()));
        DefaultSessionStore::open(StoragePaths::from_root(base))
            .await
            .expect("store should open")
    }

    fn base_ctx() -> ToolContext {
        ToolContext {
            session_key: "s1".to_string(),
            metadata: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn request_creates_pending_approval() {
        let store = create_store().await;
        store
            .touch_session("s1", "chat-1", "stdio")
            .await
            .expect("session should exist");
        let tool = ApprovalTool::with_store(store.clone());

        let output = tool
            .execute(
                json!({
                    "action": "request",
                    "tool_name": "shell",
                    "command_text": "touch file.txt",
                    "risk_level": "mutating"
                }),
                &base_ctx(),
            )
            .await
            .expect("request should succeed");
        let payload: Value =
            serde_json::from_str(&output.content_for_model).expect("response json should parse");
        let approval_id = payload
            .pointer("/approval/id")
            .and_then(Value::as_str)
            .expect("approval id in response");
        let approval = store
            .get_approval(approval_id)
            .await
            .expect("approval should exist");
        assert_eq!(approval.status, ApprovalStatus::Pending);
        assert_eq!(approval.session_key, "s1");
    }

    #[tokio::test]
    async fn resolve_updates_approval_to_approved() {
        let store = create_store().await;
        store
            .touch_session("s1", "chat-1", "stdio")
            .await
            .expect("session should exist");
        let tool = ApprovalTool::with_store(store.clone());

        let created = tool
            .execute(
                json!({
                    "action": "request",
                    "tool_name": "shell",
                    "command_text": "touch file.txt"
                }),
                &base_ctx(),
            )
            .await
            .expect("request should succeed");
        let created_json: Value =
            serde_json::from_str(&created.content_for_model).expect("response json should parse");
        let approval_id = created_json
            .pointer("/approval/id")
            .and_then(Value::as_str)
            .expect("approval id")
            .to_string();

        let resolved = tool
            .execute(
                json!({
                    "action": "resolve",
                    "approval_id": approval_id,
                    "decision": "approve",
                    "actor": "tester"
                }),
                &base_ctx(),
            )
            .await
            .expect("resolve should succeed");
        let resolved_json: Value =
            serde_json::from_str(&resolved.content_for_model).expect("response json should parse");
        assert_eq!(
            resolved_json
                .pointer("/approval/status")
                .and_then(Value::as_str),
            Some("approved")
        );
    }

    #[tokio::test]
    async fn get_rejects_cross_session_access() {
        let store = create_store().await;
        store
            .touch_session("s1", "chat-1", "stdio")
            .await
            .expect("session should exist");
        store
            .touch_session("s2", "chat-2", "stdio")
            .await
            .expect("session should exist");
        let tool = ApprovalTool::with_store(store.clone());

        let created = tool
            .execute(
                json!({
                    "action": "request",
                    "tool_name": "shell",
                    "command_text": "touch file.txt"
                }),
                &base_ctx(),
            )
            .await
            .expect("request should succeed");
        let created_json: Value =
            serde_json::from_str(&created.content_for_model).expect("response json should parse");
        let approval_id = created_json
            .pointer("/approval/id")
            .and_then(Value::as_str)
            .expect("approval id");

        let cross_ctx = ToolContext {
            session_key: "s2".to_string(),
            metadata: BTreeMap::new(),
        };
        let result = tool
            .execute(
                json!({
                    "action": "get",
                    "approval_id": approval_id
                }),
                &cross_ctx,
            )
            .await;
        let err = result.expect_err("cross session should fail").to_string();
        assert!(err.contains("does not belong"));
    }
}
