use async_trait::async_trait;
use klaw_config::{AppConfig, ApplyPatchConfig};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

const META_WORKSPACE: &str = "workspace";
const MAX_PATCH_OPERATIONS: usize = 50;
const MAX_CONTENT_BYTES: usize = 1_000_000;

pub struct ApplyPatchTool {
    config: ApplyPatchConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyPatchRequest {
    operations: Vec<PatchOperation>,
}

#[derive(Debug, Deserialize)]
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
        std::env::var("HOME")
            .map(PathBuf::from)
            .map_err(|err| ToolError::ExecutionFailed(format!("failed to resolve home dir: {err}")))
    }

    fn resolve_workspace_path(&self, base: &Path, input_path: &str) -> Result<PathBuf, ToolError> {
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
            if self.is_allowed_path(base, &canonical)? {
                return Ok(canonical);
            }
            return Err(ToolError::ExecutionFailed(format!(
                "path `{}` is not allowed by apply_patch policy",
                canonical.display(),
            )));
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
        if !self.is_allowed_path(base, &canonical_ancestor)? {
            return Err(ToolError::ExecutionFailed(format!(
                "path `{}` is not allowed by apply_patch policy",
                candidate.display(),
            )));
        }

        let suffix = candidate.strip_prefix(ancestor).map_err(|_| {
            ToolError::ExecutionFailed(format!("failed to resolve path `{}`", candidate.display()))
        })?;
        Ok(canonical_ancestor.join(suffix))
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
    ) -> Result<Vec<ResolvedPatchOperation>, ToolError> {
        operations
            .into_iter()
            .map(|operation| match operation {
                PatchOperation::AddFile { path, content } => Ok(ResolvedPatchOperation::AddFile {
                    path: self.resolve_workspace_path(base, &path)?,
                    content,
                }),
                PatchOperation::UpdateFile { path, content } => {
                    Ok(ResolvedPatchOperation::UpdateFile {
                        path: self.resolve_workspace_path(base, &path)?,
                        content,
                    })
                }
                PatchOperation::DeleteFile { path } => Ok(ResolvedPatchOperation::DeleteFile {
                    path: self.resolve_workspace_path(base, &path)?,
                }),
                PatchOperation::MoveFile { from, to } => Ok(ResolvedPatchOperation::MoveFile {
                    from: self.resolve_workspace_path(base, &from)?,
                    to: self.resolve_workspace_path(base, &to)?,
                }),
            })
            .collect()
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
    ) -> Result<PatchResult, ToolError> {
        let operations = self.resolve_operations(base, operations)?;
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
        let payload =
            serde_json::to_value(self.apply_patch(&base, request.operations)?).map_err(|err| {
                ToolError::ExecutionFailed(format!("serialize apply_patch result: {err}"))
            })?;

        let content = serde_json::to_string_pretty(&payload).map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to render fs output: {err}"))
        })?;
        Ok(ToolOutput {
            content_for_model: content.clone(),
            content_for_user: Some(content),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_config::AppConfig;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_workspace() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("klaw-fs-test-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
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
        assert!(out
            .unwrap_err()
            .to_string()
            .contains("unknown field `action`"));
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
        assert!(out.unwrap_err().to_string().contains("not allowed"));
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
}
