use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::{process::Command, time::timeout};

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 200;
const DEFAULT_TIMEOUT_MS: u64 = 10_000;
const MAX_TIMEOUT_MS: u64 = 60_000;

pub struct LocalSearchTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalSearchRequest {
    query: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    include_pattern: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

impl LocalSearchTool {
    pub fn new() -> Self {
        Self
    }

    fn parse_request(args: Value) -> Result<LocalSearchRequest, ToolError> {
        let mut request: LocalSearchRequest = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("invalid request: {err}")))?;

        request.query = request.query.trim().to_string();
        if request.query.is_empty() {
            return Err(ToolError::InvalidArgs(
                "`query` cannot be empty".to_string(),
            ));
        }

        if let Some(path) = request.path.as_mut() {
            *path = path.trim().to_string();
            if path.is_empty() {
                return Err(ToolError::InvalidArgs("`path` cannot be empty".to_string()));
            }
        }

        if let Some(pattern) = request.include_pattern.as_mut() {
            *pattern = pattern.trim().to_string();
            if pattern.is_empty() {
                return Err(ToolError::InvalidArgs(
                    "`include_pattern` cannot be empty".to_string(),
                ));
            }
        }

        if let Some(limit) = request.limit {
            if limit == 0 {
                return Err(ToolError::InvalidArgs(
                    "`limit` must be greater than 0".to_string(),
                ));
            }
            if limit > MAX_LIMIT {
                return Err(ToolError::InvalidArgs(format!(
                    "`limit` must be <= {MAX_LIMIT}"
                )));
            }
        }

        if let Some(timeout_ms) = request.timeout_ms {
            if timeout_ms == 0 {
                return Err(ToolError::InvalidArgs(
                    "`timeout_ms` must be greater than 0".to_string(),
                ));
            }
            if timeout_ms > MAX_TIMEOUT_MS {
                return Err(ToolError::InvalidArgs(format!(
                    "`timeout_ms` must be <= {MAX_TIMEOUT_MS}"
                )));
            }
        }

        Ok(request)
    }

    fn format_result(
        query: &str,
        search_path: &str,
        limit: usize,
        total: usize,
        files: &[String],
    ) -> String {
        if files.is_empty() {
            return format!("query: {query}\npath: {search_path}\nno matching files");
        }

        let mut out = format!(
            "query: {query}\npath: {search_path}\nmatched files: {} (showing up to {limit})\n",
            total.min(limit)
        );
        for (idx, file) in files.iter().enumerate() {
            out.push_str(&format!("{}. {}\n", idx + 1, file));
        }
        if total > files.len() {
            out.push_str(&format!("... truncated ({} more)", total - files.len()));
        } else {
            out = out.trim_end().to_string();
        }
        out
    }
}

#[async_trait]
impl Tool for LocalSearchTool {
    fn name(&self) -> &str {
        "local_search"
    }

    fn description(&self) -> &str {
        "Search local files by content and return matching file paths. Use this to quickly locate where terms appear before opening specific files."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Arguments for local file-content search powered by ripgrep.",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Literal or regex pattern to search in file contents.",
                    "minLength": 1,
                    "examples": ["ToolRegistry::default()", "fn execute\\(", "OPENAI_API_KEY"]
                },
                "path": {
                    "type": "string",
                    "description": "Search root path. Relative paths are resolved from workspace. Defaults to current workspace root.",
                    "default": ".",
                    "examples": [".", "klaw-tool/src", "docs/src"]
                },
                "include_pattern": {
                    "type": "string",
                    "description": "Optional glob include filter passed to ripgrep via --glob. Example: '**/*.rs'.",
                    "examples": ["**/*.rs", "docs/**/*.md"]
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of matching file paths to return. Defaults to 20.",
                    "minimum": 1,
                    "maximum": 200,
                    "default": 20
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Search timeout in milliseconds. Defaults to 10000.",
                    "minimum": 1,
                    "maximum": 60000,
                    "default": 10000
                }
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::FilesystemRead
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let request = Self::parse_request(args)?;
        let limit = request.limit.unwrap_or(DEFAULT_LIMIT);
        let timeout_ms = request.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
        let search_path = request.path.as_deref().unwrap_or(".");

        let mut command = Command::new("rg");
        command.args([
            "--files-with-matches",
            "--hidden",
            "--no-messages",
            "--glob",
            "!.git",
            "--glob",
            "!node_modules",
        ]);
        if let Some(pattern) = request.include_pattern.as_deref() {
            command.arg("--glob").arg(pattern);
        }
        command.arg(&request.query).arg(search_path);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        if let Some(workspace) = ctx.metadata.get("workspace").and_then(Value::as_str) {
            if Path::new(search_path).is_relative() {
                command.current_dir(Path::new(workspace));
            }
        }

        let output = timeout(Duration::from_millis(timeout_ms), command.output())
            .await
            .map_err(|_| {
                ToolError::ExecutionFailed(format!("local_search timed out after {timeout_ms}ms"))
            })?
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::NotFound {
                    ToolError::ExecutionFailed(
                        "ripgrep (`rg`) not found in PATH; install ripgrep to use local_search"
                            .to_string(),
                    )
                } else {
                    ToolError::ExecutionFailed(format!("failed to execute local_search: {err}"))
                }
            })?;

        let exit_code = output.status.code().unwrap_or(-1);
        if exit_code != 0 && exit_code != 1 {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(ToolError::ExecutionFailed(format!(
                "local_search failed with exit code {exit_code}: {stderr}"
            )));
        }

        let all_files: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToString::to_string)
            .collect();
        let files: Vec<String> = all_files.iter().take(limit).cloned().collect();

        let content =
            Self::format_result(&request.query, search_path, limit, all_files.len(), &files);
        Ok(ToolOutput {
            content_for_model: content.clone(),
            content_for_user: Some(content),
        })
    }
}

impl Default for LocalSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::fs;

    fn test_context(workspace: &std::path::Path) -> ToolContext {
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

    fn temp_workspace() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "klaw-local-search-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn test_local_search_finds_matching_files() {
        let dir = temp_workspace();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/a.rs"), "fn alpha() {}\n").unwrap();
        fs::write(dir.join("src/b.rs"), "fn beta() {}\n").unwrap();
        fs::write(dir.join("README.md"), "alpha appears here too\n").unwrap();
        let tool = LocalSearchTool::new();

        let result = tool
            .execute(
                json!({
                    "query": "alpha",
                    "path": ".",
                    "include_pattern": "**/*"
                }),
                &test_context(&dir),
            )
            .await
            .unwrap();

        assert!(result.content_for_model.contains("a.rs"));
        assert!(result.content_for_model.contains("README.md"));
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn test_local_search_respects_include_pattern() {
        let dir = temp_workspace();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/a.rs"), "search_me").unwrap();
        fs::write(dir.join("notes.md"), "search_me").unwrap();
        let tool = LocalSearchTool::new();

        let result = tool
            .execute(
                json!({
                    "query": "search_me",
                    "path": ".",
                    "include_pattern": "**/*.rs"
                }),
                &test_context(&dir),
            )
            .await
            .unwrap();

        assert!(result.content_for_model.contains("a.rs"));
        assert!(!result.content_for_model.contains("notes.md"));
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn test_local_search_truncates_by_limit() {
        let dir = temp_workspace();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/a.rs"), "needle").unwrap();
        fs::write(dir.join("src/b.rs"), "needle").unwrap();
        fs::write(dir.join("src/c.rs"), "needle").unwrap();
        let tool = LocalSearchTool::new();

        let result = tool
            .execute(
                json!({
                    "query": "needle",
                    "path": ".",
                    "limit": 2
                }),
                &test_context(&dir),
            )
            .await
            .unwrap();

        assert!(result.content_for_model.contains("truncated"));
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn test_local_search_no_matches() {
        let dir = temp_workspace();
        fs::write(dir.join("a.txt"), "hello").unwrap();
        let tool = LocalSearchTool::new();

        let result = tool
            .execute(
                json!({
                    "query": "not-found",
                    "path": "."
                }),
                &test_context(&dir),
            )
            .await
            .unwrap();

        assert!(result.content_for_model.contains("no matching files"));
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn test_local_search_rejects_invalid_limit() {
        let tool = LocalSearchTool::new();
        let err = tool
            .execute(
                json!({
                    "query": "abc",
                    "limit": 0
                }),
                &ToolContext {
                    session_key: "s1".to_string(),
                    metadata: BTreeMap::new(),
                },
            )
            .await
            .unwrap_err();

        assert!(err.to_string().contains("`limit` must be greater than 0"));
    }
}
