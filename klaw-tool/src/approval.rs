use async_trait::async_trait;
use klaw_approval::{
    ApprovalCreateInput, ApprovalManager, ApprovalRecord, ApprovalResolveDecision,
    SqliteApprovalManager,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

pub struct ApprovalTool {
    manager: SqliteApprovalManager,
}

impl ApprovalTool {
    pub fn with_manager(manager: SqliteApprovalManager) -> Self {
        Self { manager }
    }

    fn format_output(response: &ApprovalToolResponse) -> Result<String, ToolError> {
        serde_json::to_string_pretty(response)
            .map_err(|err| ToolError::ExecutionFailed(format!("failed to serialize output: {err}")))
    }
}

fn normalize_non_empty(value: &str, field: &str) -> Result<String, ToolError> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(ToolError::InvalidArgs(format!("`{field}` cannot be empty")));
    }
    Ok(normalized.to_string())
}

fn shell_now_ms() -> i64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
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

        let response = match request {
            ApprovalRequest::Request(input) => {
                let created = self
                    .manager
                    .create_approval(ApprovalCreateInput {
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
                    .await
                    .map_err(|err| {
                        ToolError::ExecutionFailed(format!("failed to create approval: {err}"))
                    })?;
                ApprovalToolResponse {
                    action: "request",
                    updated: true,
                    approval: created,
                }
            }
            ApprovalRequest::Get(input) => {
                let normalized_session = normalize_non_empty(&ctx.session_key, "session_key")?;
                let approval = self
                    .manager
                    .get_approval(&input.approval_id)
                    .await
                    .map_err(|err| {
                        ToolError::ExecutionFailed(format!("failed to get approval: {err}"))
                    })?;
                if approval.session_key != normalized_session {
                    return Err(ToolError::ExecutionFailed(
                        "approval does not belong to current session".to_string(),
                    ));
                }
                ApprovalToolResponse {
                    action: "get",
                    updated: false,
                    approval,
                }
            }
            ApprovalRequest::Resolve(input) => {
                let normalized_session = normalize_non_empty(&ctx.session_key, "session_key")?;
                let approval = self
                    .manager
                    .get_approval(&input.approval_id)
                    .await
                    .map_err(|err| {
                        ToolError::ExecutionFailed(format!("failed to get approval: {err}"))
                    })?;
                if approval.session_key != normalized_session {
                    return Err(ToolError::ExecutionFailed(
                        "approval does not belong to current session".to_string(),
                    ));
                }
                let outcome = self
                    .manager
                    .resolve_approval(
                        &input.approval_id,
                        match input.decision {
                            ApprovalDecision::Approve => ApprovalResolveDecision::Approve,
                            ApprovalDecision::Reject => ApprovalResolveDecision::Reject,
                        },
                        input.actor.as_deref(),
                        shell_now_ms(),
                    )
                    .await
                    .map_err(|err| {
                        ToolError::ExecutionFailed(format!("failed to resolve approval: {err}"))
                    })?;

                ApprovalToolResponse {
                    action: "resolve",
                    updated: outcome.updated,
                    approval: outcome.approval,
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
    use klaw_approval::ApprovalStatus;
    use klaw_storage::{DefaultSessionStore, SessionStorage, StoragePaths};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn create_store() -> DefaultSessionStore {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        let base = std::env::temp_dir().join(format!("klaw-approval-tool-test-{now_ms}-{suffix}"));
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
        let tool = ApprovalTool::with_manager(SqliteApprovalManager::from_store(store.clone()));

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
        let tool = ApprovalTool::with_manager(SqliteApprovalManager::from_store(store.clone()));

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
        let tool = ApprovalTool::with_manager(SqliteApprovalManager::from_store(store.clone()));

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
