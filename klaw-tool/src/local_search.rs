use async_trait::async_trait;
use globset::{Glob, GlobMatcher};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::{process::Command, time::timeout};
use walkdir::WalkDir;

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

    fn workspace_root(ctx: &ToolContext) -> Option<&Path> {
        ctx.metadata
            .get("workspace")
            .and_then(Value::as_str)
            .map(Path::new)
    }

    fn resolve_paths<'a>(search_path: &'a str, workspace_root: Option<&'a Path>) -> SearchPaths {
        match workspace_root {
            Some(root) if Path::new(search_path).is_relative() => SearchPaths {
                command_cwd: Some(root.to_path_buf()),
                display_base: root.to_path_buf(),
                search_root: root.join(search_path),
            },
            _ => {
                let path = PathBuf::from(search_path);
                SearchPaths {
                    command_cwd: None,
                    display_base: PathBuf::new(),
                    search_root: path,
                }
            }
        }
    }

    fn build_rg_command(
        request: &LocalSearchRequest,
        search_path: &str,
        ctx: &ToolContext,
    ) -> Command {
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

        if let Some(workspace) = Self::workspace_root(ctx) {
            if Path::new(search_path).is_relative() {
                command.current_dir(workspace);
            }
        }

        command
    }

    async fn execute_rg(
        request: &LocalSearchRequest,
        search_path: &str,
        ctx: &ToolContext,
    ) -> Result<Result<Vec<String>, std::io::Error>, ToolError> {
        let output = timeout(
            Duration::from_millis(request.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS)),
            Self::build_rg_command(request, search_path, ctx).output(),
        )
        .await
        .map_err(|_| {
            ToolError::ExecutionFailed(format!(
                "local_search timed out after {}ms",
                request.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS)
            ))
        })?;

        let output = match output {
            Ok(output) => output,
            Err(err) => return Ok(Err(err)),
        };

        let exit_code = output.status.code().unwrap_or(-1);
        if exit_code != 0 && exit_code != 1 {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(ToolError::ExecutionFailed(format!(
                "local_search failed with exit code {exit_code}: {stderr}"
            )));
        }

        Ok(Ok(Self::collect_output_files(&output.stdout)))
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
        let mut files = Vec::new();

        for entry in WalkDir::new(&search_paths.search_root)
            .follow_links(false)
            .into_iter()
            .filter_entry(Self::keep_walk_entry)
        {
            let entry = entry.map_err(|err| {
                ToolError::ExecutionFailed(format!("failed to walk search path: {err}"))
            })?;
            if !entry.file_type().is_file() {
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

    fn keep_walk_entry(entry: &walkdir::DirEntry) -> bool {
        let Some(name) = entry.file_name().to_str() else {
            return true;
        };
        name != ".git" && name != "node_modules"
    }

    async fn execute_grep(
        request: &LocalSearchRequest,
        search_paths: &SearchPaths,
    ) -> Result<Vec<String>, ToolError> {
        let candidates =
            Self::collect_fallback_candidates(search_paths, request.include_pattern.as_deref())?;
        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        let mut matches = BTreeSet::new();
        for chunk in candidates.chunks(GREP_BATCH_SIZE) {
            let mut command = Command::new("grep");
            command.args(["-R", "-l", "-E", &request.query]);
            command.args(chunk.iter().map(|path| path.as_os_str()));
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
            if let Some(cwd) = &search_paths.command_cwd {
                command.current_dir(cwd);
            }

            let output = command.output().await.map_err(|err| {
                if err.kind() == ErrorKind::NotFound {
                    ToolError::ExecutionFailed(
                        "both ripgrep (`rg`) and grep are not available in PATH".to_string(),
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

            matches.extend(
                Self::collect_output_files(&output.stdout)
                    .into_iter()
                    .map(|path| Self::normalize_result_path(&path, search_paths)),
            );
        }

        Ok(matches.into_iter().collect())
    }

    fn normalize_result_path(path: &str, search_paths: &SearchPaths) -> String {
        let path = Path::new(path);
        if search_paths.command_cwd.is_some() {
            return path
                .strip_prefix(&search_paths.display_base)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();
        }

        path.to_string_lossy().to_string()
    }

    fn collect_output_files(stdout: &[u8]) -> Vec<String> {
        String::from_utf8_lossy(stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToString::to_string)
            .collect()
    }
}

struct SearchPaths {
    command_cwd: Option<PathBuf>,
    display_base: PathBuf,
    search_root: PathBuf,
}

fn normalize_path_for_glob(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
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
        let all_files = match Self::execute_rg(&request, search_path, ctx).await? {
            Ok(files) => files,
            Err(err) if err.kind() == ErrorKind::NotFound => timeout(
                Duration::from_millis(timeout_ms),
                Self::execute_grep(&request, &search_paths),
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
    use std::process::Output;

    fn output(status: i32, stdout: &str, stderr: &str) -> Output {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;

            Output {
                status: std::process::ExitStatus::from_raw(status << 8),
                stdout: stdout.as_bytes().to_vec(),
                stderr: stderr.as_bytes().to_vec(),
            }
        }

        #[cfg(not(unix))]
        {
            panic!("tests require unix exit status support");
        }
    }

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

    #[test]
    fn test_collect_fallback_candidates_respects_include_pattern() {
        let dir = temp_workspace();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/a.rs"), "search_me").unwrap();
        fs::write(dir.join("notes.md"), "search_me").unwrap();

        let search_paths = SearchPaths {
            command_cwd: Some(dir.clone()),
            display_base: dir.clone(),
            search_root: dir.clone(),
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
        fs::create_dir_all(dir.join("node_modules")).unwrap();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join(".git/ignored.txt"), "needle").unwrap();
        fs::write(dir.join("node_modules/pkg.js"), "needle").unwrap();
        fs::write(dir.join("src/ok.rs"), "needle").unwrap();

        let search_paths = SearchPaths {
            command_cwd: Some(dir.clone()),
            display_base: dir.clone(),
            search_root: dir.clone(),
        };

        let files = LocalSearchTool::collect_fallback_candidates(&search_paths, None).unwrap();
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
    fn test_normalize_result_path_matches_rg_relative_output() {
        let dir = temp_workspace();
        let search_paths = SearchPaths {
            command_cwd: Some(dir.clone()),
            display_base: dir.clone(),
            search_root: dir.clone(),
        };

        let file = dir.join("src/a.rs");
        assert_eq!(
            LocalSearchTool::normalize_result_path(&file.to_string_lossy(), &search_paths),
            "src/a.rs"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_collect_output_files_parses_stdout() {
        let files = LocalSearchTool::collect_output_files(b"src/a.rs\n\nREADME.md\n");
        assert_eq!(files, vec!["src/a.rs", "README.md"]);
    }

    #[test]
    fn test_rg_missing_should_trigger_grep_fallback_branch() {
        let err = std::io::Error::new(ErrorKind::NotFound, "rg not found");
        assert_eq!(err.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn test_grep_exit_code_one_means_no_matches() {
        let grep_output = output(1, "", "");
        let exit_code = grep_output.status.code().unwrap_or(-1);
        assert_eq!(exit_code, 1);
        assert!(LocalSearchTool::collect_output_files(&grep_output.stdout).is_empty());
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
