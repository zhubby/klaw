use async_trait::async_trait;
use globset::{Glob, GlobMatcher};
use ignore::WalkBuilder;
use klaw_util::command_search_path;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, Command},
    time::timeout,
};

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 200;
const DEFAULT_TIMEOUT_MS: u64 = 10_000;
const MAX_TIMEOUT_MS: u64 = 60_000;
const GREP_BATCH_SIZE: usize = 200;

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

#[derive(Debug, Serialize)]
struct LocalSearchMatch {
    path: String,
}

#[derive(Debug, Serialize)]
struct LocalSearchTruncation {
    truncated: bool,
    reason: &'static str,
    limit: usize,
    total_matches: Option<usize>,
    total_matches_known: bool,
    returned_matches: usize,
    omitted_matches: Option<usize>,
}

#[derive(Debug, Serialize)]
struct LocalSearchResponse {
    query: String,
    path: String,
    include_pattern: Option<String>,
    total_matches: Option<usize>,
    total_matches_known: bool,
    returned_matches: usize,
    matches: Vec<LocalSearchMatch>,
    truncation: Option<LocalSearchTruncation>,
}

#[derive(Debug)]
struct SearchExecution {
    files: Vec<String>,
    total_matches: Option<usize>,
    limit_reached: bool,
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

    fn build_response(
        query: &str,
        display_path: &str,
        include_pattern: Option<&str>,
        limit: usize,
        execution: SearchExecution,
    ) -> LocalSearchResponse {
        let returned_matches = execution.files.len();
        let truncation = execution.limit_reached.then_some(LocalSearchTruncation {
            truncated: true,
            reason: "limit",
            limit,
            total_matches: execution.total_matches,
            total_matches_known: execution.total_matches.is_some(),
            returned_matches,
            omitted_matches: execution
                .total_matches
                .map(|total_matches| total_matches.saturating_sub(returned_matches)),
        });

        LocalSearchResponse {
            query: query.to_string(),
            path: display_path.to_string(),
            include_pattern: include_pattern.map(ToString::to_string),
            total_matches: execution.total_matches,
            total_matches_known: execution.total_matches.is_some(),
            returned_matches,
            matches: execution
                .files
                .iter()
                .cloned()
                .map(|path| LocalSearchMatch { path })
                .collect(),
            truncation,
        }
    }

    fn format_user_result(response: &LocalSearchResponse) -> String {
        if response.matches.is_empty() {
            return format!(
                "query: {}\npath: {}\nno matching files",
                response.query, response.path
            );
        }

        let total_label = response
            .total_matches
            .map(|total_matches| total_matches.to_string())
            .unwrap_or_else(|| format!("at least {}", response.returned_matches));
        let mut out = format!(
            "query: {}\npath: {}\nmatched files: {} (showing {})\n",
            response.query, response.path, total_label, response.returned_matches
        );
        for (idx, file) in response.matches.iter().enumerate() {
            out.push_str(&format!("{}. {}\n", idx + 1, file.path));
        }
        if let Some(truncation) = &response.truncation {
            if let Some(omitted_matches) = truncation.omitted_matches {
                out.push_str(&format!(
                    "... truncated by {} ({} more)",
                    truncation.reason, omitted_matches
                ));
            } else {
                out.push_str(&format!(
                    "... truncated by {} (additional matches not counted)",
                    truncation.reason
                ));
            }
        } else {
            out = out.trim_end().to_string();
        }
        out
    }

    fn workspace_root(ctx: &ToolContext) -> Option<&Path> {
        ctx.metadata
            .get("workspace")
            .and_then(Value::as_str)
            .map(Path::new)
    }

    fn resolve_paths<'a>(search_path: &'a str, workspace_root: Option<&'a Path>) -> SearchPaths {
        let workspace_root = workspace_root.map(Path::to_path_buf);
        let requested_path = PathBuf::from(search_path);
        let search_root = if requested_path.is_relative() {
            workspace_root
                .as_ref()
                .map(|root| root.join(&requested_path))
                .unwrap_or(requested_path.clone())
        } else {
            requested_path.clone()
        };
        let command_cwd = requested_path
            .is_relative()
            .then(|| workspace_root.clone())
            .flatten();
        let explicit_file = search_root.is_file();

        SearchPaths {
            command_cwd,
            workspace_root,
            search_root,
            explicit_file,
        }
    }

    fn build_rg_command(
        request: &LocalSearchRequest,
        search_path: &str,
        ctx: &ToolContext,
    ) -> Command {
        let mut command = binary_command("rg");
        command.kill_on_drop(true);
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

        if let Some(workspace) = Self::workspace_root(ctx) {
            if Path::new(search_path).is_relative() {
                command.current_dir(workspace);
            }
        }

        command
    }

    async fn collect_search_execution(
        mut child: Child,
        limit: usize,
        normalize_path: impl Fn(&str) -> String,
    ) -> Result<SearchExecution, ToolError> {
        let stdout = child.stdout.take().ok_or_else(|| {
            ToolError::ExecutionFailed("local_search failed to capture stdout".to_string())
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            ToolError::ExecutionFailed("local_search failed to capture stderr".to_string())
        })?;
        let stderr_task = tokio::spawn(async move {
            let mut bytes = Vec::new();
            let mut reader = BufReader::new(stderr);
            let _ = reader.read_to_end(&mut bytes).await;
            bytes
        });

        let mut reader = BufReader::new(stdout).lines();
        let mut files = Vec::new();
        let mut limit_reached = false;

        while let Some(line) = reader.next_line().await.map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to read local_search output: {err}"))
        })? {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if files.len() >= limit {
                limit_reached = true;
                break;
            }
            files.push(normalize_path(trimmed));
        }

        if limit_reached {
            child.start_kill().map_err(|err| {
                ToolError::ExecutionFailed(format!(
                    "failed to stop local_search after limit: {err}"
                ))
            })?;
        }

        let status = child.wait().await.map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to wait for local_search: {err}"))
        })?;
        let stderr = stderr_task.await.unwrap_or_default();

        if !limit_reached {
            let exit_code = status.code().unwrap_or(-1);
            if exit_code != 0 && exit_code != 1 {
                let stderr = String::from_utf8_lossy(&stderr).trim().to_string();
                return Err(ToolError::ExecutionFailed(format!(
                    "local_search failed with exit code {exit_code}: {stderr}"
                )));
            }
        }

        Ok(SearchExecution {
            total_matches: (!limit_reached).then_some(files.len()),
            files,
            limit_reached,
        })
    }

    async fn execute_rg(
        request: &LocalSearchRequest,
        search_path: &str,
        search_paths: &SearchPaths,
        limit: usize,
        ctx: &ToolContext,
    ) -> Result<Result<SearchExecution, std::io::Error>, ToolError> {
        let child = match Self::build_rg_command(request, search_path, ctx).spawn() {
            Ok(child) => child,
            Err(err) => return Ok(Err(err)),
        };

        let execution = timeout(
            Duration::from_millis(request.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS)),
            Self::collect_search_execution(child, limit, |path| {
                Self::normalize_rg_result_path(path, &search_paths)
            }),
        )
        .await
        .map_err(|_| {
            ToolError::ExecutionFailed(format!(
                "local_search timed out after {}ms",
                request.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS)
            ))
        })??;

        Ok(Ok(execution))
    }

    fn build_include_matcher(
        include_pattern: Option<&str>,
    ) -> Result<Option<GlobMatcher>, ToolError> {
        include_pattern
            .map(|pattern| {
                Glob::new(pattern)
                    .map_err(|err| {
                        ToolError::InvalidArgs(format!(
                            "invalid `include_pattern` glob `{pattern}`: {err}"
                        ))
                    })
                    .map(|glob| glob.compile_matcher())
            })
            .transpose()
    }

    fn collect_fallback_candidates(
        search_paths: &SearchPaths,
        include_pattern: Option<&str>,
    ) -> Result<Vec<PathBuf>, ToolError> {
        let include_matcher = Self::build_include_matcher(include_pattern)?;
        if search_paths.search_root.is_file() {
            let path = &search_paths.search_root;
            if is_strongly_excluded(path) {
                return Ok(Vec::new());
            }
            if let Some(matcher) = include_matcher.as_ref() {
                let candidate = path
                    .file_name()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| path.to_path_buf());
                if !matcher.is_match(&normalize_path_for_glob(&candidate)) {
                    return Ok(Vec::new());
                }
            }
            return Ok(vec![path.to_path_buf()]);
        }

        let mut files = Vec::new();
        let mut builder = WalkBuilder::new(&search_paths.search_root);
        builder
            .hidden(false)
            .follow_links(false)
            .require_git(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .ignore(true);

        for entry in builder.build() {
            let entry = entry.map_err(|err| {
                ToolError::ExecutionFailed(format!("failed to walk search path: {err}"))
            })?;
            if is_strongly_excluded(entry.path()) {
                continue;
            }
            if !entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
            {
                continue;
            }

            let path = entry.path();
            let relative = path.strip_prefix(&search_paths.search_root).unwrap_or(path);
            if let Some(matcher) = include_matcher.as_ref() {
                let candidate = normalize_path_for_glob(relative);
                if !matcher.is_match(&candidate) {
                    continue;
                }
            }

            files.push(path.to_path_buf());
        }

        Ok(files)
    }

    async fn execute_grep(
        request: &LocalSearchRequest,
        search_paths: &SearchPaths,
        limit: usize,
    ) -> Result<SearchExecution, ToolError> {
        let candidates =
            Self::collect_fallback_candidates(search_paths, request.include_pattern.as_deref())?;
        if candidates.is_empty() {
            return Ok(SearchExecution {
                files: Vec::new(),
                total_matches: Some(0),
                limit_reached: false,
            });
        }

        let mut matches = BTreeSet::new();
        for chunk in candidates.chunks(GREP_BATCH_SIZE) {
            let mut command = binary_command("grep");
            command.kill_on_drop(true);
            command.args(["-R", "-l", "-E", &request.query]);
            command.args(chunk.iter().map(|path| path.as_os_str()));
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
            if let Some(cwd) = &search_paths.command_cwd {
                command.current_dir(cwd);
            }

            let child = command.spawn().map_err(|err| {
                if err.kind() == ErrorKind::NotFound {
                    ToolError::ExecutionFailed(
                        "both ripgrep (`rg`) and grep are not available in PATH".to_string(),
                    )
                } else {
                    ToolError::ExecutionFailed(format!("failed to execute local_search: {err}"))
                }
            })?;

            let remaining = limit.saturating_sub(matches.len());
            let execution = Self::collect_search_execution(child, remaining, |path| {
                Self::normalize_result_path(path, search_paths)
            })
            .await?;

            matches.extend(execution.files);
            if execution.limit_reached || matches.len() >= limit {
                return Ok(SearchExecution {
                    files: matches.into_iter().collect(),
                    total_matches: None,
                    limit_reached: true,
                });
            }
        }

        let files: Vec<String> = matches.into_iter().collect();
        Ok(SearchExecution {
            total_matches: Some(files.len()),
            files,
            limit_reached: false,
        })
    }

    fn normalize_result_path(path: &str, search_paths: &SearchPaths) -> String {
        let path = Path::new(path);
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else if search_paths.explicit_file {
            search_paths.search_root.clone()
        } else {
            search_paths.search_root.join(path)
        };

        Self::format_result_path(&absolute, search_paths)
    }

    fn normalize_rg_result_path(path: &str, search_paths: &SearchPaths) -> String {
        let path = Path::new(path);
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else if let Some(cwd) = &search_paths.command_cwd {
            cwd.join(path)
        } else if search_paths.explicit_file {
            search_paths.search_root.clone()
        } else {
            search_paths.search_root.join(path)
        };

        Self::format_result_path(&absolute, search_paths)
    }

    fn format_result_path(path: &Path, search_paths: &SearchPaths) -> String {
        if let Some(workspace_root) = &search_paths.workspace_root
            && let Ok(relative) = path.strip_prefix(workspace_root)
        {
            return relative.to_string_lossy().to_string();
        }

        path.to_string_lossy().to_string()
    }

    fn display_search_path(search_paths: &SearchPaths) -> String {
        Self::format_result_path(&search_paths.search_root, search_paths)
    }
}

fn binary_command(binary: &str) -> Command {
    let mut command = Command::new(binary);
    if let Some(path) = command_search_path() {
        command.env("PATH", path);
    }
    command
}

struct SearchPaths {
    command_cwd: Option<PathBuf>,
    workspace_root: Option<PathBuf>,
    search_root: PathBuf,
    explicit_file: bool,
}

fn normalize_path_for_glob(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn is_strongly_excluded(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == std::ffi::OsStr::new(".git"))
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
            "description": "Arguments for local file-content search using ripgrep first, with grep fallback when ripgrep is unavailable.",
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
                    "description": "Optional glob include filter for matching file paths before content search. Example: '**/*.rs'.",
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
        let search_paths = Self::resolve_paths(search_path, Self::workspace_root(ctx));
        let execution = match Self::execute_rg(&request, search_path, &search_paths, limit, ctx)
            .await?
        {
            Ok(execution) => execution,
            Err(err) if err.kind() == ErrorKind::NotFound => timeout(
                Duration::from_millis(timeout_ms),
                Self::execute_grep(&request, &search_paths, limit),
            )
            .await
            .map_err(|_| {
                ToolError::ExecutionFailed(format!("local_search timed out after {timeout_ms}ms"))
            })??,
            Err(err) => {
                return Err(ToolError::ExecutionFailed(format!(
                    "failed to execute local_search: {err}"
                )));
            }
        };
        let response = Self::build_response(
            &request.query,
            &Self::display_search_path(&search_paths),
            request.include_pattern.as_deref(),
            limit,
            execution,
        );
        let content_for_model = serde_json::to_string_pretty(&response).map_err(|err| {
            ToolError::ExecutionFailed(format!("local_search serialization failed: {err}"))
        })?;
        let content_for_user = Self::format_user_result(&response);
        Ok(ToolOutput {
            content_for_model,
            content_for_user: Some(content_for_user),
            media: Vec::new(),
            signals: Vec::new(),
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

    fn matches_path_suffix(value: &Value, suffix: &str) -> bool {
        value["matches"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["path"].as_str())
            .any(|path| path.ends_with(suffix))
    }

    #[test]
    fn test_collect_fallback_candidates_respects_include_pattern() {
        let dir = temp_workspace();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/a.rs"), "search_me").unwrap();
        fs::write(dir.join("notes.md"), "search_me").unwrap();

        let search_paths = SearchPaths {
            command_cwd: Some(dir.clone()),
            workspace_root: Some(dir.clone()),
            search_root: dir.clone(),
            explicit_file: false,
        };

        let files =
            LocalSearchTool::collect_fallback_candidates(&search_paths, Some("**/*.rs")).unwrap();
        let normalized: Vec<String> = files
            .iter()
            .map(|path| {
                path.strip_prefix(&dir)
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();

        assert_eq!(normalized, vec!["src/a.rs"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_collect_fallback_candidates_skips_default_excluded_dirs() {
        let dir = temp_workspace();
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join(".git/ignored.txt"), "needle").unwrap();
        fs::write(dir.join("src/ok.rs"), "needle").unwrap();

        let search_paths = SearchPaths {
            command_cwd: Some(dir.clone()),
            workspace_root: Some(dir.clone()),
            search_root: dir.clone(),
            explicit_file: false,
        };

        let files =
            LocalSearchTool::collect_fallback_candidates(&search_paths, Some("**/*.rs")).unwrap();
        let normalized: Vec<String> = files
            .iter()
            .map(|path| {
                path.strip_prefix(&dir)
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();

        assert_eq!(normalized, vec!["src/ok.rs"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_collect_fallback_candidates_respects_gitignore() {
        let dir = temp_workspace();
        fs::create_dir_all(dir.join("dist")).unwrap();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join(".gitignore"), "dist/\n").unwrap();
        fs::write(dir.join("dist/generated.rs"), "needle").unwrap();
        fs::write(dir.join("src/ok.rs"), "needle").unwrap();

        let search_paths = SearchPaths {
            command_cwd: Some(dir.clone()),
            workspace_root: Some(dir.clone()),
            search_root: dir.clone(),
            explicit_file: false,
        };

        let files =
            LocalSearchTool::collect_fallback_candidates(&search_paths, Some("**/*.rs")).unwrap();
        let normalized: Vec<String> = files
            .iter()
            .map(|path| {
                path.strip_prefix(&dir)
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();

        assert_eq!(normalized, vec!["src/ok.rs"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_collect_fallback_candidates_explicit_file_bypasses_gitignore() {
        let dir = temp_workspace();
        fs::create_dir_all(dir.join("dist")).unwrap();
        fs::write(dir.join(".gitignore"), "dist/\n").unwrap();
        fs::write(dir.join("dist/generated.rs"), "needle").unwrap();

        let search_paths = SearchPaths {
            command_cwd: Some(dir.clone()),
            workspace_root: Some(dir.clone()),
            search_root: dir.join("dist/generated.rs"),
            explicit_file: true,
        };

        let files =
            LocalSearchTool::collect_fallback_candidates(&search_paths, Some("**/*.rs")).unwrap();

        assert_eq!(files, vec![dir.join("dist/generated.rs")]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_normalize_result_path_matches_rg_relative_output() {
        let dir = temp_workspace();
        let search_paths = SearchPaths {
            command_cwd: Some(dir.clone()),
            workspace_root: Some(dir.clone()),
            search_root: dir.clone(),
            explicit_file: false,
        };

        let file = dir.join("src/a.rs");
        assert_eq!(
            LocalSearchTool::normalize_result_path(&file.to_string_lossy(), &search_paths),
            "src/a.rs"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_normalize_rg_result_path_for_workspace_explicit_file_is_workspace_relative() {
        let dir = temp_workspace();
        let search_paths = SearchPaths {
            command_cwd: Some(dir.clone()),
            workspace_root: Some(dir.clone()),
            search_root: dir.join("src/a.rs"),
            explicit_file: true,
        };

        assert_eq!(
            LocalSearchTool::normalize_rg_result_path("src/a.rs", &search_paths),
            "src/a.rs"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_normalize_result_path_outside_workspace_stays_absolute() {
        let dir = temp_workspace();
        let outside = std::env::temp_dir().join("klaw-local-search-outside.rs");
        let search_paths = SearchPaths {
            command_cwd: None,
            workspace_root: Some(dir.clone()),
            search_root: outside.clone(),
            explicit_file: true,
        };

        assert_eq!(
            LocalSearchTool::normalize_result_path(&outside.to_string_lossy(), &search_paths),
            outside.to_string_lossy()
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_display_search_path_is_workspace_relative_when_inside_workspace() {
        let dir = temp_workspace();
        let search_paths = SearchPaths {
            command_cwd: Some(dir.clone()),
            workspace_root: Some(dir.clone()),
            search_root: dir.join("klaw-tool/src"),
            explicit_file: false,
        };

        assert_eq!(
            LocalSearchTool::display_search_path(&search_paths),
            "klaw-tool/src"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_display_search_path_is_absolute_when_outside_workspace() {
        let dir = temp_workspace();
        let outside = std::env::temp_dir().join("klaw-local-search-outside-dir");
        let search_paths = SearchPaths {
            command_cwd: None,
            workspace_root: Some(dir.clone()),
            search_root: outside.clone(),
            explicit_file: false,
        };

        assert_eq!(
            LocalSearchTool::display_search_path(&search_paths),
            outside.to_string_lossy()
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_build_response_includes_truncation_metadata() {
        let response = LocalSearchTool::build_response(
            "needle",
            ".",
            Some("**/*.rs"),
            2,
            SearchExecution {
                files: vec!["src/a.rs".to_string(), "src/b.rs".to_string()],
                total_matches: None,
                limit_reached: true,
            },
        );

        assert!(response.total_matches.is_none());
        assert!(!response.total_matches_known);
        assert_eq!(response.returned_matches, 2);
        assert_eq!(response.matches.len(), 2);
        assert_eq!(response.include_pattern.as_deref(), Some("**/*.rs"));
        let truncation = response.truncation.expect("truncation metadata");
        assert!(truncation.truncated);
        assert_eq!(truncation.reason, "limit");
        assert!(truncation.total_matches.is_none());
        assert!(!truncation.total_matches_known);
        assert!(truncation.omitted_matches.is_none());
    }

    #[test]
    fn test_rg_missing_should_trigger_grep_fallback_branch() {
        let err = std::io::Error::new(ErrorKind::NotFound, "rg not found");
        assert_eq!(err.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn test_build_response_without_truncation_keeps_exact_total() {
        let response = LocalSearchTool::build_response(
            "needle",
            ".",
            None,
            20,
            SearchExecution {
                files: vec!["src/a.rs".to_string()],
                total_matches: Some(1),
                limit_reached: false,
            },
        );

        assert_eq!(response.total_matches, Some(1));
        assert!(response.total_matches_known);
        assert!(response.truncation.is_none());
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
        let parsed: Value = serde_json::from_str(&result.content_for_model).unwrap();
        assert_eq!(parsed["query"], "alpha");
        assert_eq!(parsed["total_matches"], 2);
        assert_eq!(parsed["total_matches_known"], true);
        assert_eq!(parsed["returned_matches"], 2);
        assert!(matches_path_suffix(&parsed, "src/a.rs"));
        assert!(matches_path_suffix(&parsed, "README.md"));
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
        let parsed: Value = serde_json::from_str(&result.content_for_model).unwrap();
        assert_eq!(parsed["total_matches"], 1);
        assert_eq!(parsed["total_matches_known"], true);
        assert!(matches_path_suffix(&parsed, "src/a.rs"));
        assert!(!matches_path_suffix(&parsed, "notes.md"));
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
        let parsed: Value = serde_json::from_str(&result.content_for_model).unwrap();
        assert!(parsed["total_matches"].is_null());
        assert_eq!(parsed["total_matches_known"], false);
        assert_eq!(parsed["returned_matches"], 2);
        assert_eq!(parsed["truncation"]["truncated"], true);
        assert_eq!(parsed["truncation"]["reason"], "limit");
        assert!(parsed["truncation"]["total_matches"].is_null());
        assert_eq!(parsed["truncation"]["total_matches_known"], false);
        assert!(parsed["truncation"]["omitted_matches"].is_null());
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
        let parsed: Value = serde_json::from_str(&result.content_for_model).unwrap();
        assert_eq!(parsed["total_matches"], 0);
        assert_eq!(parsed["total_matches_known"], true);
        assert_eq!(parsed["returned_matches"], 0);
        assert_eq!(parsed["matches"], json!([]));
        assert!(parsed["truncation"].is_null());
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
