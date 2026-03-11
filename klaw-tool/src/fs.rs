use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

const META_WORKSPACE: &str = "workspace";
const DEFAULT_READ_LIMIT: usize = 200;
const MAX_READ_LIMIT: usize = 2000;
const MAX_READ_CHARS: usize = 200_000;
const MAX_WRITE_BYTES: usize = 1_000_000;
const MAX_PATCH_OPERATIONS: usize = 50;

pub struct FsTool;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum FsAction {
    ReadFile,
    WriteFile,
    ApplyPatch,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FsRequest {
    action: FsAction,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    append: Option<bool>,
    #[serde(default)]
    create_dirs: Option<bool>,
    #[serde(default)]
    operations: Option<Vec<PatchOperation>>,
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
struct ReadFileResult {
    action: &'static str,
    path: String,
    total_lines: usize,
    offset: usize,
    limit: usize,
    returned_lines: usize,
    truncated: bool,
    content: String,
}

#[derive(Debug, Serialize)]
struct WriteFileResult {
    action: &'static str,
    path: String,
    bytes_written: usize,
    append: bool,
}

#[derive(Debug, Serialize)]
struct PatchResult {
    action: &'static str,
    operations_applied: usize,
    summary: Vec<String>,
}

impl FsTool {
    pub fn new() -> Self {
        Self
    }

    fn parse_request(args: Value) -> Result<FsRequest, ToolError> {
        let mut request: FsRequest = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("invalid request: {err}")))?;

        if let Some(path) = request.path.as_mut() {
            *path = path.trim().to_string();
            if path.is_empty() {
                return Err(ToolError::InvalidArgs("`path` cannot be empty".to_string()));
            }
        }
        if let Some(content) = request.content.as_mut() {
            if content.len() > MAX_WRITE_BYTES {
                return Err(ToolError::InvalidArgs(format!(
                    "`content` exceeds max size of {MAX_WRITE_BYTES} bytes"
                )));
            }
        }

        match request.action {
            FsAction::ReadFile => {
                if request.path.is_none() {
                    return Err(ToolError::InvalidArgs(
                        "`path` is required for `read_file`".to_string(),
                    ));
                }
                if let Some(limit) = request.limit {
                    if limit == 0 || limit > MAX_READ_LIMIT {
                        return Err(ToolError::InvalidArgs(format!(
                            "`limit` must be in 1..={MAX_READ_LIMIT}"
                        )));
                    }
                }
            }
            FsAction::WriteFile => {
                if request.path.is_none() {
                    return Err(ToolError::InvalidArgs(
                        "`path` is required for `write_file`".to_string(),
                    ));
                }
                if request.content.is_none() {
                    return Err(ToolError::InvalidArgs(
                        "`content` is required for `write_file`".to_string(),
                    ));
                }
            }
            FsAction::ApplyPatch => {
                let ops = request.operations.as_ref().ok_or_else(|| {
                    ToolError::InvalidArgs("`operations` is required for `apply_patch`".to_string())
                })?;
                if ops.is_empty() {
                    return Err(ToolError::InvalidArgs(
                        "`operations` cannot be empty".to_string(),
                    ));
                }
                if ops.len() > MAX_PATCH_OPERATIONS {
                    return Err(ToolError::InvalidArgs(format!(
                        "`operations` must be <= {MAX_PATCH_OPERATIONS}"
                    )));
                }
            }
        }

        Ok(request)
    }

    fn resolve_workspace_base(ctx: &ToolContext) -> Result<PathBuf, ToolError> {
        if let Some(workspace) = ctx.metadata.get(META_WORKSPACE).and_then(Value::as_str) {
            return fs::canonicalize(workspace).map_err(|err| {
                ToolError::ExecutionFailed(format!("invalid workspace path: {err}"))
            });
        }
        std::env::current_dir().map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to resolve current dir: {err}"))
        })
    }

    fn resolve_workspace_path(base: &Path, input_path: &str) -> Result<PathBuf, ToolError> {
        let raw = PathBuf::from(input_path);
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
            if !canonical.starts_with(base) {
                return Err(ToolError::ExecutionFailed(format!(
                    "path `{}` is outside workspace `{}`",
                    canonical.display(),
                    base.display()
                )));
            }
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
        if !canonical_ancestor.starts_with(base) {
            return Err(ToolError::ExecutionFailed(format!(
                "path `{}` is outside workspace `{}`",
                candidate.display(),
                base.display()
            )));
        }

        let suffix = candidate.strip_prefix(ancestor).map_err(|_| {
            ToolError::ExecutionFailed(format!("failed to resolve path `{}`", candidate.display()))
        })?;
        Ok(canonical_ancestor.join(suffix))
    }

    fn read_file(
        base: &Path,
        path: &str,
        offset: usize,
        limit: usize,
    ) -> Result<ReadFileResult, ToolError> {
        let resolved = Self::resolve_workspace_path(base, path)?;
        if !resolved.is_file() {
            return Err(ToolError::ExecutionFailed(format!(
                "`{}` is not a file",
                resolved.display()
            )));
        }

        let content = fs::read_to_string(&resolved).map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to read `{}`: {err}", resolved.display()))
        })?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();
        let start = offset.min(total_lines);
        let end = start.saturating_add(limit).min(total_lines);
        let selected = &lines[start..end];

        let mut selected_text = selected.join("\n");
        if !selected_text.is_empty() && content.ends_with('\n') && end == total_lines {
            selected_text.push('\n');
        }

        let truncated_by_char_limit = selected_text.len() > MAX_READ_CHARS;
        if truncated_by_char_limit {
            selected_text.truncate(MAX_READ_CHARS);
        }

        Ok(ReadFileResult {
            action: "read_file",
            path: resolved.display().to_string(),
            total_lines,
            offset: start,
            limit,
            returned_lines: selected.len(),
            truncated: truncated_by_char_limit || end < total_lines,
            content: selected_text,
        })
    }

    fn write_file(
        base: &Path,
        path: &str,
        content: &str,
        append: bool,
        create_dirs: bool,
    ) -> Result<WriteFileResult, ToolError> {
        let resolved = Self::resolve_workspace_path(base, path)?;
        let content_bytes = content.as_bytes();
        if content_bytes.len() > MAX_WRITE_BYTES {
            return Err(ToolError::InvalidArgs(format!(
                "`content` exceeds max size of {MAX_WRITE_BYTES} bytes"
            )));
        }

        if let Some(parent) = resolved.parent() {
            if create_dirs {
                fs::create_dir_all(parent).map_err(|err| {
                    ToolError::ExecutionFailed(format!(
                        "failed to create parent dirs `{}`: {err}",
                        parent.display()
                    ))
                })?;
            } else if !parent.exists() {
                return Err(ToolError::ExecutionFailed(format!(
                    "parent directory `{}` does not exist",
                    parent.display()
                )));
            }
        }

        if append {
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&resolved)
                .map_err(|err| {
                    ToolError::ExecutionFailed(format!(
                        "failed to open `{}` for append: {err}",
                        resolved.display()
                    ))
                })?;
            file.write_all(content_bytes).map_err(|err| {
                ToolError::ExecutionFailed(format!(
                    "failed to append `{}`: {err}",
                    resolved.display()
                ))
            })?;
        } else {
            fs::write(&resolved, content_bytes).map_err(|err| {
                ToolError::ExecutionFailed(format!(
                    "failed to write `{}`: {err}",
                    resolved.display()
                ))
            })?;
        }

        Ok(WriteFileResult {
            action: "write_file",
            path: resolved.display().to_string(),
            bytes_written: content_bytes.len(),
            append,
        })
    }

    fn apply_patch(base: &Path, operations: Vec<PatchOperation>) -> Result<PatchResult, ToolError> {
        let mut summary = Vec::with_capacity(operations.len());

        for op in operations {
            match op {
                PatchOperation::AddFile { path, content } => {
                    let resolved = Self::resolve_workspace_path(base, path.trim())?;
                    if resolved.exists() {
                        return Err(ToolError::ExecutionFailed(format!(
                            "add_file failed: `{}` already exists",
                            resolved.display()
                        )));
                    }
                    if let Some(parent) = resolved.parent() {
                        fs::create_dir_all(parent).map_err(|err| {
                            ToolError::ExecutionFailed(format!(
                                "failed to create parent dirs `{}`: {err}",
                                parent.display()
                            ))
                        })?;
                    }
                    fs::write(&resolved, content.as_bytes()).map_err(|err| {
                        ToolError::ExecutionFailed(format!(
                            "failed to create `{}`: {err}",
                            resolved.display()
                        ))
                    })?;
                    summary.push(format!("add_file {}", resolved.display()));
                }
                PatchOperation::UpdateFile { path, content } => {
                    let resolved = Self::resolve_workspace_path(base, path.trim())?;
                    if !resolved.is_file() {
                        return Err(ToolError::ExecutionFailed(format!(
                            "update_file failed: `{}` is not a file",
                            resolved.display()
                        )));
                    }
                    fs::write(&resolved, content.as_bytes()).map_err(|err| {
                        ToolError::ExecutionFailed(format!(
                            "failed to update `{}`: {err}",
                            resolved.display()
                        ))
                    })?;
                    summary.push(format!("update_file {}", resolved.display()));
                }
                PatchOperation::DeleteFile { path } => {
                    let resolved = Self::resolve_workspace_path(base, path.trim())?;
                    if !resolved.is_file() {
                        return Err(ToolError::ExecutionFailed(format!(
                            "delete_file failed: `{}` is not a file",
                            resolved.display()
                        )));
                    }
                    fs::remove_file(&resolved).map_err(|err| {
                        ToolError::ExecutionFailed(format!(
                            "failed to delete `{}`: {err}",
                            resolved.display()
                        ))
                    })?;
                    summary.push(format!("delete_file {}", resolved.display()));
                }
                PatchOperation::MoveFile { from, to } => {
                    let from_resolved = Self::resolve_workspace_path(base, from.trim())?;
                    let to_resolved = Self::resolve_workspace_path(base, to.trim())?;
                    if !from_resolved.is_file() {
                        return Err(ToolError::ExecutionFailed(format!(
                            "move_file failed: source `{}` is not a file",
                            from_resolved.display()
                        )));
                    }
                    if to_resolved.exists() {
                        return Err(ToolError::ExecutionFailed(format!(
                            "move_file failed: target `{}` already exists",
                            to_resolved.display()
                        )));
                    }
                    if let Some(parent) = to_resolved.parent() {
                        fs::create_dir_all(parent).map_err(|err| {
                            ToolError::ExecutionFailed(format!(
                                "failed to create parent dirs `{}`: {err}",
                                parent.display()
                            ))
                        })?;
                    }
                    fs::rename(&from_resolved, &to_resolved).map_err(|err| {
                        ToolError::ExecutionFailed(format!(
                            "failed to move `{}` -> `{}`: {err}",
                            from_resolved.display(),
                            to_resolved.display()
                        ))
                    })?;
                    summary.push(format!(
                        "move_file {} -> {}",
                        from_resolved.display(),
                        to_resolved.display()
                    ));
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

impl Default for FsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for FsTool {
    fn name(&self) -> &str {
        "fs"
    }

    fn description(&self) -> &str {
        "Read and modify local workspace files. Use `read_file` to read by line window, `write_file` to overwrite/append file content, and `apply_patch` for multi-file add/update/delete/move operations."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Filesystem request for reading and editing files under the current workspace.",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["read_file", "write_file", "apply_patch"],
                    "description": "Operation to perform."
                },
                "path": {
                    "type": "string",
                    "description": "Target file path for read_file/write_file. Relative paths are resolved from workspace."
                },
                "offset": {
                    "type": "integer",
                    "minimum": 0,
                    "default": 0,
                    "description": "Line offset for read_file (0-based)."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_READ_LIMIT,
                    "default": DEFAULT_READ_LIMIT,
                    "description": "Maximum number of lines to return for read_file."
                },
                "content": {
                    "type": "string",
                    "description": "Text content for write_file."
                },
                "append": {
                    "type": "boolean",
                    "default": false,
                    "description": "When true, append to file for write_file; otherwise overwrite."
                },
                "create_dirs": {
                    "type": "boolean",
                    "default": true,
                    "description": "Whether write_file should create missing parent directories."
                },
                "operations": {
                    "type": "array",
                    "description": "Patch operations for apply_patch.",
                    "maxItems": MAX_PATCH_OPERATIONS,
                    "items": {
                        "type": "object",
                        "oneOf": [
                            {
                                "properties": {
                                    "op": { "const": "add_file" },
                                    "path": { "type": "string" },
                                    "content": { "type": "string" }
                                },
                                "required": ["op", "path", "content"],
                                "additionalProperties": false
                            },
                            {
                                "properties": {
                                    "op": { "const": "update_file" },
                                    "path": { "type": "string" },
                                    "content": { "type": "string" }
                                },
                                "required": ["op", "path", "content"],
                                "additionalProperties": false
                            },
                            {
                                "properties": {
                                    "op": { "const": "delete_file" },
                                    "path": { "type": "string" }
                                },
                                "required": ["op", "path"],
                                "additionalProperties": false
                            },
                            {
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
            "required": ["action"],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::FilesystemWrite
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let request = Self::parse_request(args)?;
        let base = Self::resolve_workspace_base(ctx)?;
        let payload = match request.action {
            FsAction::ReadFile => {
                let path = request
                    .path
                    .as_deref()
                    .expect("validated path for read_file");
                let offset = request.offset.unwrap_or(0);
                let limit = request.limit.unwrap_or(DEFAULT_READ_LIMIT);
                serde_json::to_value(Self::read_file(&base, path, offset, limit)?).map_err(
                    |err| ToolError::ExecutionFailed(format!("serialize read_file result: {err}")),
                )?
            }
            FsAction::WriteFile => {
                let path = request
                    .path
                    .as_deref()
                    .expect("validated path for write_file");
                let content = request
                    .content
                    .as_deref()
                    .expect("validated content for write_file");
                let append = request.append.unwrap_or(false);
                let create_dirs = request.create_dirs.unwrap_or(true);
                serde_json::to_value(Self::write_file(&base, path, content, append, create_dirs)?)
                    .map_err(|err| {
                    ToolError::ExecutionFailed(format!("serialize write_file result: {err}"))
                })?
            }
            FsAction::ApplyPatch => {
                let operations = request
                    .operations
                    .expect("validated operations for apply_patch");
                serde_json::to_value(Self::apply_patch(&base, operations)?).map_err(|err| {
                    ToolError::ExecutionFailed(format!("serialize apply_patch result: {err}"))
                })?
            }
        };

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
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_workspace() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("klaw-fs-test-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn test_ctx(workspace: &Path) -> ToolContext {
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "workspace".to_string(),
            json!(workspace.to_string_lossy().to_string()),
        );
        ToolContext {
            session_key: "s1".to_string(),
            metadata,
        }
    }

    #[tokio::test]
    async fn read_file_supports_offset_and_limit() {
        let workspace = temp_workspace();
        let file = workspace.join("notes.txt");
        fs::write(&file, "a\nb\nc\nd\n").unwrap();
        let tool = FsTool::new();

        let out = tool
            .execute(
                json!({
                    "action": "read_file",
                    "path": "notes.txt",
                    "offset": 1,
                    "limit": 2
                }),
                &test_ctx(&workspace),
            )
            .await
            .unwrap();

        assert!(out.content_for_model.contains("\"returned_lines\": 2"));
        assert!(out.content_for_model.contains("b\\nc"));
    }

    #[tokio::test]
    async fn write_file_and_append_work() {
        let workspace = temp_workspace();
        let tool = FsTool::new();

        tool.execute(
            json!({
                "action": "write_file",
                "path": "src/a.txt",
                "content": "hello"
            }),
            &test_ctx(&workspace),
        )
        .await
        .unwrap();

        tool.execute(
            json!({
                "action": "write_file",
                "path": "src/a.txt",
                "content": " world",
                "append": true
            }),
            &test_ctx(&workspace),
        )
        .await
        .unwrap();

        let file_content = fs::read_to_string(workspace.join("src/a.txt")).unwrap();
        assert_eq!(file_content, "hello world");
    }

    #[tokio::test]
    async fn apply_patch_runs_multi_file_operations() {
        let workspace = temp_workspace();
        let tool = FsTool::new();

        let out = tool
            .execute(
                json!({
                    "action": "apply_patch",
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
    async fn rejects_access_outside_workspace() {
        let workspace = temp_workspace();
        let tool = FsTool::new();
        let out = tool
            .execute(
                json!({
                    "action": "read_file",
                    "path": "/etc/hosts"
                }),
                &test_ctx(&workspace),
            )
            .await;

        assert!(out.is_err());
        assert!(out.unwrap_err().to_string().contains("outside workspace"));
    }
}
