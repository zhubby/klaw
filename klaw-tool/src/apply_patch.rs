use async_trait::async_trait;
use klaw_approval::{ApprovalCreateInput, ApprovalManager, SqliteApprovalManager};
use klaw_config::{AppConfig, ApplyPatchConfig};
use klaw_util::{default_data_dir, workspace_dir};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput, ToolSignal};

const META_WORKSPACE: &str = "workspace";
const META_APPLY_PATCH_APPROVED: &str = "apply_patch.approved";
const META_APPLY_PATCH_APPROVAL_ID: &str = "apply_patch.approval_id";
const MAX_PATCH_OPERATIONS: usize = 50;
const MAX_CONTENT_BYTES: usize = 1_000_000;
const APPROVAL_TTL_MINUTES: i64 = 10;

pub struct ApplyPatchTool {
    config: ApplyPatchConfig,
    storage_root_dir: Option<String>,
    approval_manager: Option<SqliteApprovalManager>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ApplyPatchRequest {
    operations: Vec<PatchOperation>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum PatchOperation {
    AddFile { path: String, content: String },
    UpdateFile { path: String, content: String },
    DeleteFile { path: String },
    MoveFile { from: String, to: String },
}

#[derive(Debug, Serialize)]
struct PatchResult {
    action: &'static str,
    operations_applied: usize,
    summary: Vec<String>,
}

enum ResolvedPatchOperation {
    AddFile { path: PathBuf, content: String },
    UpdateFile { path: PathBuf, content: String },
    DeleteFile { path: PathBuf },
    MoveFile { from: PathBuf, to: PathBuf },
}

impl ApplyPatchTool {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            config: config.tools.apply_patch.clone(),
            storage_root_dir: config.storage.root_dir.clone(),
            approval_manager: None,
        }
    }

    pub fn with_store(config: &AppConfig, store: klaw_storage::DefaultSessionStore) -> Self {
        Self {
            config: config.tools.apply_patch.clone(),
            storage_root_dir: config.storage.root_dir.clone(),
            approval_manager: Some(SqliteApprovalManager::from_store(store)),
        }
    }

    fn parse_request(args: Value) -> Result<ApplyPatchRequest, ToolError> {
        let request: ApplyPatchRequest = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("invalid request: {err}")))?;

        if request.operations.is_empty() {
            return Err(ToolError::InvalidArgs(
                "`operations` cannot be empty".to_string(),
            ));
        }
        if request.operations.len() > MAX_PATCH_OPERATIONS {
            return Err(ToolError::InvalidArgs(format!(
                "`operations` must be <= {MAX_PATCH_OPERATIONS}"
            )));
        }

        for operation in &request.operations {
            match operation {
                PatchOperation::AddFile { path, content }
                | PatchOperation::UpdateFile { path, content } => {
                    Self::validate_relative_path(path, "path")?;
                    Self::validate_content_size(content)?;
                }
                PatchOperation::DeleteFile { path } => {
                    Self::validate_relative_path(path, "path")?;
                }
                PatchOperation::MoveFile { from, to } => {
                    Self::validate_relative_path(from, "from")?;
                    Self::validate_relative_path(to, "to")?;
                }
            }
        }

        Ok(request)
    }

    fn validate_relative_path(path: &str, field: &str) -> Result<(), ToolError> {
        if path.trim().is_empty() {
            return Err(ToolError::InvalidArgs(format!("`{field}` cannot be empty")));
        }
        Ok(())
    }

    fn validate_content_size(content: &str) -> Result<(), ToolError> {
        if content.len() > MAX_CONTENT_BYTES {
            return Err(ToolError::InvalidArgs(format!(
                "`content` exceeds max size of {MAX_CONTENT_BYTES} bytes"
            )));
        }
        Ok(())
    }

    fn resolve_workspace_base(&self, ctx: &ToolContext) -> Result<PathBuf, ToolError> {
        if let Some(workspace) = ctx.metadata.get(META_WORKSPACE).and_then(Value::as_str) {
            return fs::canonicalize(workspace).map_err(|err| {
                ToolError::ExecutionFailed(format!("invalid workspace path: {err}"))
            });
        }
        if let Some(workspace) = self.config.workspace.as_deref() {
            return fs::canonicalize(workspace).map_err(|err| {
                ToolError::ExecutionFailed(format!("invalid apply_patch workspace path: {err}"))
            });
        }
        Self::resolve_data_workspace(self.storage_root_dir.as_deref())
    }

    fn resolve_data_workspace(storage_root: Option<&str>) -> Result<PathBuf, ToolError> {
        let root = if let Some(root) = storage_root.map(str::trim).filter(|root| !root.is_empty()) {
            PathBuf::from(root)
        } else {
            default_data_dir().ok_or_else(|| {
                ToolError::ExecutionFailed("failed to resolve home dir".to_string())
            })?
        };
        let workspace = workspace_dir(&root);
        fs::create_dir_all(&workspace).map_err(|err| {
            ToolError::ExecutionFailed(format!(
                "failed to ensure data workspace `{}`: {err}",
                workspace.display()
            ))
        })?;
        fs::canonicalize(&workspace).map_err(|err| {
            ToolError::ExecutionFailed(format!(
                "failed to resolve data workspace `{}`: {err}",
                workspace.display()
            ))
        })
    }

    async fn check_approval(
        &self,
        base: &Path,
        request: &ApplyPatchRequest,
        ctx: &ToolContext,
    ) -> Result<bool, ToolError> {
        let unauthorized_paths = self.collect_unauthorized_paths(base, &request.operations)?;
        if unauthorized_paths.is_empty() {
            return Ok(false);
        }

        let approved_via_flag = ctx
            .metadata
            .get(META_APPLY_PATCH_APPROVED)
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if approved_via_flag {
            return Ok(true);
        }

        let request_hash = Self::request_hash(request)?;
        if let (Some(manager), Some(approval_id)) = (
            self.approval_manager.as_ref(),
            ctx.metadata
                .get(META_APPLY_PATCH_APPROVAL_ID)
                .and_then(Value::as_str),
        ) {
            let consumed = manager
                .consume_tool_approval(
                    approval_id,
                    self.name(),
                    &ctx.session_key,
                    &request_hash,
                    Self::now_ms(),
                )
                .await
                .map_err(|err| {
                    ToolError::ExecutionFailed(format!(
                        "failed to validate approval `{approval_id}`: {err}"
                    ))
                })?;
            if consumed {
                return Ok(true);
            }
        }

        if let Some(manager) = self.approval_manager.as_ref() {
            let consumed = manager
                .consume_latest_tool_approval(
                    self.name(),
                    &ctx.session_key,
                    &request_hash,
                    Self::now_ms(),
                )
                .await
                .map_err(|err| {
                    ToolError::ExecutionFailed(format!(
                        "failed to consume approved apply_patch request for session `{}`: {err}",
                        ctx.session_key
                    ))
                })?;
            if consumed {
                return Ok(true);
            }
        }

        let preview = Self::request_preview(request);
        let unauthorized = unauthorized_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>();
        if let Some(manager) = self.approval_manager.as_ref() {
            let approval = manager
                .create_approval(ApprovalCreateInput {
                    session_key: ctx.session_key.clone(),
                    tool_name: self.name().to_string(),
                    command_text: preview.clone(),
                    command_preview: Some(preview.clone()),
                    command_hash: Some(request_hash),
                    risk_level: Some("outside_workspace".to_string()),
                    requested_by: Some("agent".to_string()),
                    justification: None,
                    expires_in_minutes: Some(APPROVAL_TTL_MINUTES),
                })
                .await
                .map_err(|err| {
                    ToolError::ExecutionFailed(format!("failed to create approval: {err}"))
                })?;
            let approval_id = approval.id.clone();
            return Err(ToolError::structured_execution_failed(
                format!(
                    "This apply_patch request writes outside the allowed workspace and requires approval.\nApproval ID: {approval_id}\nPaths: {}\nAfter approval, retry the same tool call with metadata `apply_patch.approval_id` set to this ID.",
                    unauthorized.join(", ")
                ),
                "approval_required",
                Some(json!({
                    "approval_id": approval_id,
                    "tool_name": self.name(),
                    "session_key": ctx.session_key,
                    "risk_level": "outside_workspace",
                    "command_preview": approval.command_preview,
                    "command_hash": approval.command_hash,
                    "expires_at_ms": approval.expires_at_ms,
                    "unauthorized_paths": unauthorized,
                })),
                true,
                vec![ToolSignal::approval_required(
                    &approval.id,
                    self.name(),
                    &ctx.session_key,
                    Some("outside_workspace"),
                    Some(&approval.command_preview),
                )],
            ));
        }

        Err(ToolError::ExecutionFailed(format!(
            "This apply_patch request writes outside the allowed workspace and requires approval. Unauthorized paths: {}. Retry with approval metadata once it has been granted.",
            unauthorized.join(", ")
        )))
    }

    fn resolve_candidate_path(&self, base: &Path, input_path: &str) -> Result<PathBuf, ToolError> {
        let raw = PathBuf::from(input_path.trim());
        let candidate = if raw.is_absolute() {
            raw
        } else {
            base.join(raw)
        };

        if candidate.exists() {
            let canonical = fs::canonicalize(&candidate).map_err(|err| {
                ToolError::ExecutionFailed(format!(
                    "failed to resolve path `{}`: {err}",
                    candidate.display()
                ))
            })?;
            return Ok(canonical);
        }

        let mut ancestor = candidate.as_path();
        while !ancestor.exists() {
            ancestor = ancestor.parent().ok_or_else(|| {
                ToolError::ExecutionFailed(format!("invalid path `{}`", candidate.display()))
            })?;
        }

        let canonical_ancestor = fs::canonicalize(ancestor).map_err(|err| {
            ToolError::ExecutionFailed(format!(
                "failed to resolve ancestor path `{}`: {err}",
                ancestor.display()
            ))
        })?;
        let suffix = candidate.strip_prefix(ancestor).map_err(|_| {
            ToolError::ExecutionFailed(format!("failed to resolve path `{}`", candidate.display()))
        })?;
        Ok(canonical_ancestor.join(suffix))
    }

    fn resolve_workspace_path(
        &self,
        base: &Path,
        input_path: &str,
        approval_granted: bool,
    ) -> Result<PathBuf, ToolError> {
        let candidate = self.resolve_candidate_path(base, input_path)?;
        if approval_granted || self.is_allowed_path(base, &candidate)? {
            return Ok(candidate);
        }
        Err(ToolError::ExecutionFailed(format!(
            "path `{}` is not allowed by apply_patch policy",
            candidate.display(),
        )))
    }

    fn is_allowed_path(&self, workspace_base: &Path, path: &Path) -> Result<bool, ToolError> {
        if path.starts_with(workspace_base) {
            return Ok(true);
        }

        if path.is_absolute() && self.config.allow_absolute_paths {
            return Ok(true);
        }

        for root in &self.config.allowed_roots {
            let allowed_root = self.resolve_allowed_root(workspace_base, root)?;
            if path.starts_with(&allowed_root) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn resolve_allowed_root(
        &self,
        workspace_base: &Path,
        root: &str,
    ) -> Result<PathBuf, ToolError> {
        let raw = PathBuf::from(root.trim());
        let target = if raw.is_absolute() {
            raw
        } else {
            workspace_base.join(raw)
        };

        fs::canonicalize(&target).map_err(|err| {
            ToolError::ExecutionFailed(format!(
                "invalid apply_patch allowed root `{}`: {err}",
                target.display()
            ))
        })
    }

    fn resolve_operations(
        &self,
        base: &Path,
        operations: Vec<PatchOperation>,
        approval_granted: bool,
    ) -> Result<Vec<ResolvedPatchOperation>, ToolError> {
        operations
            .into_iter()
            .map(|operation| match operation {
                PatchOperation::AddFile { path, content } => Ok(ResolvedPatchOperation::AddFile {
                    path: self.resolve_workspace_path(base, &path, approval_granted)?,
                    content,
                }),
                PatchOperation::UpdateFile { path, content } => {
                    Ok(ResolvedPatchOperation::UpdateFile {
                        path: self.resolve_workspace_path(base, &path, approval_granted)?,
                        content,
                    })
                }
                PatchOperation::DeleteFile { path } => Ok(ResolvedPatchOperation::DeleteFile {
                    path: self.resolve_workspace_path(base, &path, approval_granted)?,
                }),
                PatchOperation::MoveFile { from, to } => Ok(ResolvedPatchOperation::MoveFile {
                    from: self.resolve_workspace_path(base, &from, approval_granted)?,
                    to: self.resolve_workspace_path(base, &to, approval_granted)?,
                }),
            })
            .collect()
    }

    fn collect_unauthorized_paths(
        &self,
        base: &Path,
        operations: &[PatchOperation],
    ) -> Result<Vec<PathBuf>, ToolError> {
        let mut unauthorized = BTreeSet::new();
        for operation in operations {
            let paths = match operation {
                PatchOperation::AddFile { path, .. }
                | PatchOperation::UpdateFile { path, .. }
                | PatchOperation::DeleteFile { path } => vec![path.as_str()],
                PatchOperation::MoveFile { from, to } => vec![from.as_str(), to.as_str()],
            };
            for raw_path in paths {
                let resolved = self.resolve_candidate_path(base, raw_path)?;
                if !self.is_allowed_path(base, &resolved)? {
                    unauthorized.insert(resolved);
                }
            }
        }
        Ok(unauthorized.into_iter().collect())
    }

    fn request_hash(request: &ApplyPatchRequest) -> Result<String, ToolError> {
        let serialized = serde_json::to_vec(request).map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to serialize apply_patch request: {err}"))
        })?;
        let mut hasher = Sha256::new();
        hasher.update(&serialized);
        Ok(format!("{:x}", hasher.finalize()))
    }

    fn request_preview(request: &ApplyPatchRequest) -> String {
        let mut summary = request
            .operations
            .iter()
            .map(|operation| match operation {
                PatchOperation::AddFile { path, .. } => format!("add_file:{path}"),
                PatchOperation::UpdateFile { path, .. } => format!("update_file:{path}"),
                PatchOperation::DeleteFile { path } => format!("delete_file:{path}"),
                PatchOperation::MoveFile { from, to } => format!("move_file:{from}->{to}"),
            })
            .collect::<Vec<_>>();
        let extra_count = summary.len().saturating_sub(3);
        summary.truncate(3);
        let preview = summary.join(", ");
        if extra_count == 0 {
            preview
        } else {
            format!("{preview}, +{extra_count} more")
        }
    }

    fn now_ms() -> i64 {
        (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
    }

    fn validate_operations(operations: &[ResolvedPatchOperation]) -> Result<(), ToolError> {
        let mut known_state = BTreeMap::<PathBuf, bool>::new();

        for operation in operations {
            match operation {
                ResolvedPatchOperation::AddFile { path, .. } => {
                    if Self::path_exists(path, &known_state)? {
                        return Err(ToolError::ExecutionFailed(format!(
                            "add_file failed: `{}` already exists",
                            path.display()
                        )));
                    }
                    known_state.insert(path.clone(), true);
                }
                ResolvedPatchOperation::UpdateFile { path, .. } => {
                    if !Self::path_exists(path, &known_state)? {
                        return Err(ToolError::ExecutionFailed(format!(
                            "update_file failed: `{}` is not a file",
                            path.display()
                        )));
                    }
                }
                ResolvedPatchOperation::DeleteFile { path } => {
                    if !Self::path_exists(path, &known_state)? {
                        return Err(ToolError::ExecutionFailed(format!(
                            "delete_file failed: `{}` is not a file",
                            path.display()
                        )));
                    }
                    known_state.insert(path.clone(), false);
                }
                ResolvedPatchOperation::MoveFile { from, to } => {
                    if !Self::path_exists(from, &known_state)? {
                        return Err(ToolError::ExecutionFailed(format!(
                            "move_file failed: source `{}` is not a file",
                            from.display()
                        )));
                    }
                    if Self::path_exists(to, &known_state)? {
                        return Err(ToolError::ExecutionFailed(format!(
                            "move_file failed: target `{}` already exists",
                            to.display()
                        )));
                    }
                    known_state.insert(from.clone(), false);
                    known_state.insert(to.clone(), true);
                }
            }
        }

        Ok(())
    }

    fn path_exists(path: &Path, known_state: &BTreeMap<PathBuf, bool>) -> Result<bool, ToolError> {
        if let Some(exists) = known_state.get(path) {
            return Ok(*exists);
        }

        Ok(path.is_file())
    }

    fn apply_patch(
        &self,
        base: &Path,
        operations: Vec<PatchOperation>,
        approval_granted: bool,
    ) -> Result<PatchResult, ToolError> {
        let operations = self.resolve_operations(base, operations, approval_granted)?;
        Self::validate_operations(&operations)?;

        let mut summary = Vec::with_capacity(operations.len());
        for operation in operations {
            match operation {
                ResolvedPatchOperation::AddFile { path, content } => {
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent).map_err(|err| {
                            ToolError::ExecutionFailed(format!(
                                "failed to create parent dirs `{}`: {err}",
                                parent.display()
                            ))
                        })?;
                    }
                    fs::write(&path, content.as_bytes()).map_err(|err| {
                        ToolError::ExecutionFailed(format!(
                            "failed to create `{}`: {err}",
                            path.display()
                        ))
                    })?;
                    summary.push(format!("add_file {}", path.display()));
                }
                ResolvedPatchOperation::UpdateFile { path, content } => {
                    fs::write(&path, content.as_bytes()).map_err(|err| {
                        ToolError::ExecutionFailed(format!(
                            "failed to update `{}`: {err}",
                            path.display()
                        ))
                    })?;
                    summary.push(format!("update_file {}", path.display()));
                }
                ResolvedPatchOperation::DeleteFile { path } => {
                    fs::remove_file(&path).map_err(|err| {
                        ToolError::ExecutionFailed(format!(
                            "failed to delete `{}`: {err}",
                            path.display()
                        ))
                    })?;
                    summary.push(format!("delete_file {}", path.display()));
                }
                ResolvedPatchOperation::MoveFile { from, to } => {
                    if let Some(parent) = to.parent() {
                        fs::create_dir_all(parent).map_err(|err| {
                            ToolError::ExecutionFailed(format!(
                                "failed to create parent dirs `{}`: {err}",
                                parent.display()
                            ))
                        })?;
                    }
                    fs::rename(&from, &to).map_err(|err| {
                        ToolError::ExecutionFailed(format!(
                            "failed to move `{}` -> `{}`: {err}",
                            from.display(),
                            to.display()
                        ))
                    })?;
                    summary.push(format!("move_file {} -> {}", from.display(), to.display()));
                }
            }
        }

        Ok(PatchResult {
            action: "apply_patch",
            operations_applied: summary.len(),
            summary,
        })
    }
}

impl Default for ApplyPatchTool {
    fn default() -> Self {
        Self {
            config: ApplyPatchConfig::default(),
            storage_root_dir: None,
            approval_manager: None,
        }
    }
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Apply batched file patches inside the workspace. Use this tool to add, update, delete, or move multiple files in one request."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Batch file edits scoped to the current workspace. Prefer one request containing the full set of related file changes.",
            "properties": {
                "operations": {
                    "type": "array",
                    "description": "Ordered file patch operations. Relative paths resolve from the workspace. Absolute paths are allowed only if they remain inside the workspace or match tools.apply_patch policy.",
                    "maxItems": MAX_PATCH_OPERATIONS,
                    "items": {
                        "type": "object",
                        "oneOf": [
                            {
                                "description": "Create a new file. Fails if the target already exists.",
                                "properties": {
                                    "op": { "const": "add_file" },
                                    "path": { "type": "string" },
                                    "content": {
                                        "type": "string",
                                        "description": "Full file content to write. Max 1,000,000 bytes."
                                    }
                                },
                                "required": ["op", "path", "content"],
                                "additionalProperties": false
                            },
                            {
                                "description": "Overwrite an existing file with new full content.",
                                "properties": {
                                    "op": { "const": "update_file" },
                                    "path": { "type": "string" },
                                    "content": {
                                        "type": "string",
                                        "description": "Full file content to write. Max 1,000,000 bytes."
                                    }
                                },
                                "required": ["op", "path", "content"],
                                "additionalProperties": false
                            },
                            {
                                "description": "Delete an existing file.",
                                "properties": {
                                    "op": { "const": "delete_file" },
                                    "path": { "type": "string" }
                                },
                                "required": ["op", "path"],
                                "additionalProperties": false
                            },
                            {
                                "description": "Move or rename an existing file. Fails if the target already exists.",
                                "properties": {
                                    "op": { "const": "move_file" },
                                    "from": { "type": "string" },
                                    "to": { "type": "string" }
                                },
                                "required": ["op", "from", "to"],
                                "additionalProperties": false
                            }
                        ]
                    }
                }
            },
            "required": ["operations"],
            "additionalProperties": false,
            "examples": [
                {
                    "operations": [
                        { "op": "update_file", "path": "src/lib.rs", "content": "pub fn run() {}\n" },
                        { "op": "move_file", "from": "src/old.rs", "to": "src/new.rs" }
                    ]
                }
            ]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::FilesystemWrite
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let request = Self::parse_request(args)?;
        let base = self.resolve_workspace_base(ctx)?;
        let approval_granted = self.check_approval(&base, &request, ctx).await?;
        let payload =
            serde_json::to_value(self.apply_patch(&base, request.operations, approval_granted)?)
                .map_err(|err| {
                    ToolError::ExecutionFailed(format!("serialize apply_patch result: {err}"))
                })?;

        let content = serde_json::to_string_pretty(&payload).map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to render fs output: {err}"))
        })?;
        Ok(ToolOutput {
            content_for_model: content.clone(),
            content_for_user: Some(content),
            signals: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_config::AppConfig;
    use klaw_storage::{ApprovalStatus, DefaultSessionStore, SessionStorage, StoragePaths};
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_workspace() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("klaw-fs-test-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    async fn create_store() -> DefaultSessionStore {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("klaw-apply-patch-test-{now_ms}-{suffix}"));
        DefaultSessionStore::open(StoragePaths::from_root(root))
            .await
            .expect("session store should open")
    }

    fn test_ctx(workspace: &Path) -> ToolContext {
        let mut metadata = BTreeMap::new();
        metadata.insert(
            META_WORKSPACE.to_string(),
            json!(workspace.to_string_lossy().to_string()),
        );
        ToolContext {
            session_key: "s1".to_string(),
            metadata,
        }
    }

    fn test_ctx_without_workspace() -> ToolContext {
        ToolContext {
            session_key: "s1".to_string(),
            metadata: BTreeMap::new(),
        }
    }

    fn test_tool() -> ApplyPatchTool {
        ApplyPatchTool::new(&AppConfig::default())
    }

    #[tokio::test]
    async fn apply_patch_runs_multi_file_operations() {
        let workspace = temp_workspace();
        let tool = test_tool();

        let out = tool
            .execute(
                json!({
                    "operations": [
                        {"op": "add_file", "path": "a.txt", "content": "one"},
                        {"op": "update_file", "path": "a.txt", "content": "two"},
                        {"op": "move_file", "from": "a.txt", "to": "dir/b.txt"},
                        {"op": "delete_file", "path": "dir/b.txt"}
                    ]
                }),
                &test_ctx(&workspace),
            )
            .await
            .unwrap();

        assert!(out.content_for_model.contains("\"operations_applied\": 4"));
        assert!(!workspace.join("dir/b.txt").exists());
    }

    #[tokio::test]
    async fn rejects_legacy_read_and_write_shapes() {
        let workspace = temp_workspace();
        let tool = test_tool();

        let out = tool
            .execute(
                json!({
                    "action": "read_file",
                    "path": "notes.txt"
                }),
                &test_ctx(&workspace),
            )
            .await;

        assert!(out.is_err());
        assert!(
            out.unwrap_err()
                .to_string()
                .contains("unknown field `action`")
        );
    }

    #[tokio::test]
    async fn rejects_access_outside_workspace() {
        let workspace = temp_workspace();
        let tool = test_tool();
        let out = tool
            .execute(
                json!({
                    "operations": [
                        {"op": "add_file", "path": "/etc/hosts", "content": "x"}
                    ]
                }),
                &test_ctx(&workspace),
            )
            .await;

        assert!(out.is_err());
        assert!(out.unwrap_err().to_string().contains("requires approval"));
    }

    #[tokio::test]
    async fn unauthorized_path_returns_approval_required_when_store_is_available() {
        let workspace = temp_workspace();
        let store = create_store().await;
        store
            .touch_session("s1", "chat-1", "terminal")
            .await
            .expect("session should exist");
        let tool = ApplyPatchTool::with_store(&AppConfig::default(), store);

        let out = tool
            .execute(
                json!({
                    "operations": [
                        {"op": "add_file", "path": "/etc/hosts", "content": "x"}
                    ]
                }),
                &test_ctx(&workspace),
            )
            .await;

        let err = out.expect_err("approval should be required");
        assert_eq!(err.code(), "approval_required");
        let approval_signal = err
            .signals()
            .iter()
            .find(|signal| signal.kind == "approval_required")
            .expect("approval signal should be present");
        assert_eq!(
            approval_signal.payload.get("tool_name"),
            Some(&json!("apply_patch"))
        );
    }

    #[tokio::test]
    async fn approved_apply_patch_request_succeeds_once_with_approval_id() {
        let workspace = temp_workspace();
        let outside_dir = temp_workspace();
        let outside_file = outside_dir.join("approved.txt");
        let store = create_store().await;
        store
            .touch_session("s1", "chat-1", "terminal")
            .await
            .expect("session should exist");
        let tool = ApplyPatchTool::with_store(&AppConfig::default(), store.clone());

        let first = tool
            .execute(
                json!({
                    "operations": [
                        {"op": "add_file", "path": outside_file.to_string_lossy(), "content": "ok"}
                    ]
                }),
                &test_ctx(&workspace),
            )
            .await;
        let first_err = first.expect_err("approval should be required").to_string();
        let approval_id = first_err
            .split("Approval ID: ")
            .nth(1)
            .and_then(|tail| tail.lines().next())
            .map(str::trim)
            .expect("approval id should be present")
            .to_string();

        store
            .update_approval_status(&approval_id, ApprovalStatus::Approved, Some("user"))
            .await
            .expect("approval should transition to approved");

        let mut metadata = BTreeMap::new();
        metadata.insert(
            META_WORKSPACE.to_string(),
            json!(workspace.to_string_lossy().to_string()),
        );
        metadata.insert(
            "apply_patch.approval_id".to_string(),
            json!(approval_id.clone()),
        );
        let approved_ctx = ToolContext {
            session_key: "s1".to_string(),
            metadata,
        };

        let approved_exec = tool
            .execute(
                json!({
                    "operations": [
                        {"op": "add_file", "path": outside_file.to_string_lossy(), "content": "ok"}
                    ]
                }),
                &approved_ctx,
            )
            .await
            .expect("approved apply_patch should execute");
        assert!(
            approved_exec
                .content_for_model
                .contains("\"operations_applied\": 1")
        );
        assert_eq!(fs::read_to_string(&outside_file).unwrap(), "ok");

        let consumed = store
            .get_approval(&approval_id)
            .await
            .expect("approval should exist");
        assert_eq!(consumed.status, ApprovalStatus::Consumed);
    }

    #[tokio::test]
    async fn approved_apply_patch_request_succeeds_with_latest_approval() {
        let workspace = temp_workspace();
        let outside_dir = temp_workspace();
        let outside_file = outside_dir.join("latest-approved.txt");
        let store = create_store().await;
        store
            .touch_session("s1", "chat-1", "terminal")
            .await
            .expect("session should exist");
        let tool = ApplyPatchTool::with_store(&AppConfig::default(), store.clone());

        let first = tool
            .execute(
                json!({
                    "operations": [
                        {"op": "add_file", "path": outside_file.to_string_lossy(), "content": "ok"}
                    ]
                }),
                &test_ctx(&workspace),
            )
            .await;
        let first_err = first.expect_err("approval should be required").to_string();
        let approval_id = first_err
            .split("Approval ID: ")
            .nth(1)
            .and_then(|tail| tail.lines().next())
            .map(str::trim)
            .expect("approval id should be present")
            .to_string();

        store
            .update_approval_status(&approval_id, ApprovalStatus::Approved, Some("user"))
            .await
            .expect("approval should transition to approved");

        let approved_exec = tool
            .execute(
                json!({
                    "operations": [
                        {"op": "add_file", "path": outside_file.to_string_lossy(), "content": "ok"}
                    ]
                }),
                &test_ctx(&workspace),
            )
            .await
            .expect("approved apply_patch should execute via latest approval");
        assert!(
            approved_exec
                .content_for_model
                .contains("\"operations_applied\": 1")
        );
        assert_eq!(fs::read_to_string(&outside_file).unwrap(), "ok");

        let consumed = store
            .get_approval(&approval_id)
            .await
            .expect("approval should exist");
        assert_eq!(consumed.status, ApprovalStatus::Consumed);
    }

    #[tokio::test]
    async fn validates_batch_before_writing() {
        let workspace = temp_workspace();
        fs::write(workspace.join("keep.txt"), "safe").unwrap();
        let tool = test_tool();

        let out = tool
            .execute(
                json!({
                    "operations": [
                        {"op": "update_file", "path": "keep.txt", "content": "changed"},
                        {"op": "delete_file", "path": "missing.txt"}
                    ]
                }),
                &test_ctx(&workspace),
            )
            .await;

        assert!(out.is_err());
        let content = fs::read_to_string(workspace.join("keep.txt")).unwrap();
        assert_eq!(content, "safe");
    }

    #[tokio::test]
    async fn allows_absolute_paths_when_configured() {
        let workspace = temp_workspace();
        let outside_dir = temp_workspace();
        let outside_file = outside_dir.join("allowed.txt");
        let mut config = AppConfig::default();
        config.tools.apply_patch.allow_absolute_paths = true;
        let tool = ApplyPatchTool::new(&config);

        tool.execute(
            json!({
                "operations": [
                    {"op": "add_file", "path": outside_file.to_string_lossy(), "content": "ok"}
                ]
            }),
            &test_ctx(&workspace),
        )
        .await
        .unwrap();

        assert_eq!(fs::read_to_string(outside_file).unwrap(), "ok");
    }

    #[tokio::test]
    async fn allows_whitelisted_roots_without_global_absolute_access() {
        let workspace = temp_workspace();
        let outside_dir = temp_workspace();
        let outside_file = outside_dir.join("allowed.txt");
        let mut config = AppConfig::default();
        config.tools.apply_patch.allowed_roots = vec![outside_dir.to_string_lossy().to_string()];
        let tool = ApplyPatchTool::new(&config);

        tool.execute(
            json!({
                "operations": [
                    {"op": "add_file", "path": outside_file.to_string_lossy(), "content": "ok"}
                ]
            }),
            &test_ctx(&workspace),
        )
        .await
        .unwrap();

        assert_eq!(fs::read_to_string(outside_file).unwrap(), "ok");
    }

    #[tokio::test]
    async fn falls_back_to_storage_root_workspace_when_unset() {
        let root_dir = temp_workspace();
        let mut config = AppConfig::default();
        config.tools.apply_patch.workspace = None;
        config.storage.root_dir = Some(root_dir.to_string_lossy().to_string());
        let tool = ApplyPatchTool::new(&config);

        tool.execute(
            json!({
                "operations": [
                    {"op": "add_file", "path": "fallback.txt", "content": "ok"}
                ]
            }),
            &test_ctx_without_workspace(),
        )
        .await
        .unwrap();

        let expected = root_dir.join("workspace").join("fallback.txt");
        assert_eq!(fs::read_to_string(expected).unwrap(), "ok");
    }
}
