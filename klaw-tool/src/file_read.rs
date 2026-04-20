use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use klaw_config::FileReadConfig;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileReadRequest {
    path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TruncatedBy {
    Lines,
    Bytes,
}

struct TruncationResult {
    content: String,
    truncated: bool,
    truncated_by: Option<TruncatedBy>,
    total_lines: usize,
    total_bytes: usize,
    output_lines: usize,
    #[allow(dead_code)]
    output_bytes: usize,
    first_line_exceeds_limit: bool,
}

fn detect_image_mime_type(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "tiff" | "tif" => "image/tiff",
        "webp" => "image/webp",
        "avif" => "image/avif",
        _ => return None,
    };
    Some(mime.to_string())
}

fn is_likely_binary(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let check_len = bytes.len().min(8192);
    let sample = &bytes[..check_len];
    let null_count = sample.iter().filter(|&&b| b == 0).count();
    null_count > check_len / 10
}

fn truncate_content(
    lines: &[String],
    total_bytes: usize,
    offset: usize,
    limit: Option<usize>,
    max_lines: usize,
    max_bytes: usize,
) -> TruncationResult {
    let start = offset.saturating_sub(1);
    if start >= lines.len() {
        return TruncationResult {
            content: String::new(),
            truncated: false,
            truncated_by: None,
            total_lines: lines.len(),
            total_bytes,
            output_lines: 0,
            output_bytes: 0,
            first_line_exceeds_limit: false,
        };
    }

    let effective_limit = limit.unwrap_or(max_lines);
    let _end = (start + effective_limit).min(lines.len());

    let mut first_line_exceeds_limit = false;
    if let Some(first_line) = lines.get(start) {
        if first_line.len() > max_bytes {
            first_line_exceeds_limit = true;
        }
    }

    if first_line_exceeds_limit {
        return TruncationResult {
            content: String::new(),
            truncated: true,
            truncated_by: Some(TruncatedBy::Bytes),
            total_lines: lines.len(),
            total_bytes,
            output_lines: 0,
            output_bytes: 0,
            first_line_exceeds_limit: true,
        };
    }

    let mut selected_lines: Vec<String> = Vec::new();
    let mut current_bytes: usize = 0;
    let mut truncated_by: Option<TruncatedBy> = None;

    for (i, line) in lines.iter().enumerate().skip(start) {
        let line_with_num = format!("{}: {}", i + 1, line);
        let line_bytes = line_with_num.len();

        if selected_lines.len() >= effective_limit {
            truncated_by = Some(TruncatedBy::Lines);
            break;
        }

        if current_bytes + line_bytes > max_bytes {
            truncated_by = Some(TruncatedBy::Bytes);
            break;
        }

        current_bytes += line_bytes;
        selected_lines.push(line_with_num);
    }

    let output_bytes = current_bytes;
    let output_lines = selected_lines.len();
    let truncated = truncated_by.is_some();
    let content = selected_lines.join("\n");

    TruncationResult {
        content,
        truncated,
        truncated_by,
        total_lines: lines.len(),
        total_bytes,
        output_lines,
        output_bytes,
        first_line_exceeds_limit: false,
    }
}

pub struct FileReadTool {
    max_lines: usize,
    max_bytes: usize,
    auto_resize_images: bool,
    read_ops: Box<dyn ReadOperations>,
}

impl FileReadTool {
    pub fn new(config: &FileReadConfig) -> Self {
        Self {
            max_lines: config.max_lines,
            max_bytes: config.max_bytes,
            auto_resize_images: config.auto_resize_images,
            read_ops: Box::new(LocalFsReadOperations),
        }
    }

    pub fn with_ops(config: &FileReadConfig, ops: Box<dyn ReadOperations>) -> Self {
        Self {
            max_lines: config.max_lines,
            max_bytes: config.max_bytes,
            auto_resize_images: config.auto_resize_images,
            read_ops: ops,
        }
    }

    fn resolve_path(&self, path_str: &str, workspace: Option<&str>) -> PathBuf {
        let path = PathBuf::from(path_str);
        if path.is_absolute() {
            path
        } else if let Some(ws) = workspace {
            PathBuf::from(ws).join(path)
        } else {
            path
        }
    }

    fn parse_request(&self, args: Value) -> Result<FileReadRequest, ToolError> {
        serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs(e.to_string()))
    }

    async fn handle_text_file(
        &self,
        path: &Path,
        request: &FileReadRequest,
        _workspace: Option<&str>,
    ) -> Result<ToolOutput, ToolError> {
        let bytes = self
            .read_ops
            .read_file(path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to read file: {e}")))?;

        if is_likely_binary(&bytes) {
            let display_path = path.display();
            let size = bytes.len();
            return Ok(ToolOutput {
                content_for_model: format!(
                    "File appears to be binary ({size} bytes). Use the shell tool with `xxd`, `hexdump`, or `file` commands to inspect binary files.\nPath: {display_path}"
                ),
                content_for_user: Some(format!("Binary file: {display_path} ({size} bytes)")),
                media: Vec::new(),
                signals: Vec::new(),
            });
        }

        let content = String::from_utf8(bytes.clone())
            .map_err(|e| ToolError::ExecutionFailed(format!("file contains invalid UTF-8: {e}")))?;

        let lines: Vec<String> = content.lines().map(str::to_string).collect();
        let total_bytes = bytes.len();

        let result = truncate_content(
            &lines,
            total_bytes,
            request.offset.unwrap_or(1),
            request.limit,
            self.max_lines,
            self.max_bytes,
        );

        let display_path = path.display();
        let model_content = if result.first_line_exceeds_limit {
            format!(
                "The first line of this file exceeds the byte limit ({max_bytes} bytes). \
                 Use the shell tool with `sed` and `head -c` to read portions of this file.\n\
                 Path: {display_path}\n\
                 Total lines: {total_lines}\n\
                 Total bytes: {total_bytes}",
                max_bytes = self.max_bytes,
                total_lines = result.total_lines,
                total_bytes = result.total_bytes,
            )
        } else {
            let mut content = result.content.clone();
            if result.truncated {
                let last_line = result.output_lines + request.offset.unwrap_or(1).saturating_sub(1);
                let reason = match result.truncated_by {
                    Some(TruncatedBy::Lines) => "line limit",
                    Some(TruncatedBy::Bytes) => "byte limit",
                    None => "limit",
                };
                content.push_str(&format!(
                    "\n\n--- Content truncated ({}). Showing {} of {} total lines. Use `offset: {}` to continue reading. ---",
                    reason,
                    result.output_lines,
                    result.total_lines,
                    last_line + 1,
                ));
            }
            content
        };

        let user_content = format!(
            "Read file: {} ({} lines{})",
            display_path,
            result.total_lines,
            if result.truncated {
                format!(
                    ", showing {} of {}",
                    result.output_lines, result.total_lines
                )
            } else {
                String::new()
            }
        );

        Ok(ToolOutput {
            content_for_model: model_content,
            content_for_user: Some(user_content),
            media: Vec::new(),
            signals: Vec::new(),
        })
    }

    async fn handle_image_file(
        &self,
        path: &Path,
        mime_type: &str,
    ) -> Result<ToolOutput, ToolError> {
        let bytes = self
            .read_ops
            .read_file(path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to read image: {e}")))?;

        let mut base64_data =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);

        let final_mime = if self.auto_resize_images {
            match resize_image(&bytes, mime_type) {
                Some(resized) => {
                    base64_data = resized.data;
                    resized.mime_type
                }
                None => {
                    let display_path = path.display();
                    return Ok(ToolOutput {
                        content_for_model: format!(
                            "Read image file [{mime_type}]\n[Image omitted: could not be resized for display. Path: {display_path}]"
                        ),
                        content_for_user: Some(format!(
                            "Image file: {display_path} ({mime_type}, resize failed)"
                        )),
                        media: Vec::new(),
                        signals: Vec::new(),
                    });
                }
            }
        } else {
            mime_type.to_string()
        };

        let data_uri = format!("data:{final_mime};base64,{base64_data}");
        let display_path = path.display();

        let model_content =
            format!("Read image file [{final_mime}]\nPath: {display_path}\nImage data included.",);
        let user_content = Some(format!("Image: {display_path} ({final_mime})"));

        Ok(ToolOutput {
            content_for_model: model_content,
            content_for_user: user_content,
            media: vec![klaw_llm::LlmMedia {
                mime_type: Some(final_mime),
                url: data_uri,
            }],
            signals: Vec::new(),
        })
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file from the local filesystem. Supports text files with offset/limit for paginated reading and automatic truncation to protect the context window. Also supports reading image files (PNG, JPEG, GIF, WebP, etc.) as base64-encoded data for multimodal models. Use this tool instead of shell commands like 'cat' for reading files."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Read a file from the filesystem. Returns file content with line numbers. Automatically truncates large files (2000 lines / 50KB) with continuation hints. Supports image file reading for multimodal models.",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file to read. Relative paths are resolved against the workspace directory."
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (1-indexed). Use this to continue reading a truncated file. Defaults to 1.",
                    "minimum": 1
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read. Defaults to 2000.",
                    "minimum": 1
                }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::FilesystemRead
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let request = self.parse_request(args)?;
        let path_str = request.path.trim().to_string();
        if path_str.is_empty() {
            return Err(ToolError::InvalidArgs("path must not be empty".to_string()));
        }

        let workspace = ctx.metadata.get("workspace").and_then(|v| v.as_str());
        let path = self.resolve_path(&path_str, workspace);

        self.read_ops
            .access(&path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("file not accessible: {e}")))?;

        let mime_type = self.read_ops.detect_image_mime_type(&path).await;
        if let Some(ref mime) = mime_type {
            self.handle_image_file(&path, mime).await
        } else {
            self.handle_text_file(&path, &request, workspace).await
        }
    }
}

impl Default for FileReadTool {
    fn default() -> Self {
        Self::new(&FileReadConfig::default())
    }
}

struct ResizedImage {
    data: String,
    mime_type: String,
}

fn resize_image(bytes: &[u8], original_mime: &str) -> Option<ResizedImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = (img.width(), img.height());
    if w <= 2000 && h <= 2000 {
        let output_format = match original_mime {
            "image/png" => image::ImageFormat::Png,
            "image/jpeg" => image::ImageFormat::Jpeg,
            "image/webp" => image::ImageFormat::WebP,
            _ => image::ImageFormat::Png,
        };
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), output_format)
            .ok()?;
        let data = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &buf);
        return Some(ResizedImage {
            data,
            mime_type: original_mime.to_string(),
        });
    }

    let ratio = (2000.0 / w.max(h) as f64).min(1.0);
    let new_w = (w as f64 * ratio) as u32;
    let new_h = (h as f64 * ratio) as u32;
    let resized = img.thumbnail(new_w.max(1), new_h.max(1));

    let output_format = match original_mime {
        "image/png" => image::ImageFormat::Png,
        "image/jpeg" => image::ImageFormat::Jpeg,
        "image/webp" => image::ImageFormat::WebP,
        _ => image::ImageFormat::Png,
    };

    let mut buf = Vec::new();
    resized
        .write_to(&mut std::io::Cursor::new(&mut buf), output_format)
        .ok()?;
    let data = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &buf);

    Some(ResizedImage {
        data,
        mime_type: original_mime.to_string(),
    })
}

#[async_trait]
pub trait ReadOperations: Send + Sync {
    fn read_file(
        &self,
        path: &Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, String>> + Send + '_>>;
    fn access(
        &self,
        path: &Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + '_>>;
    fn detect_image_mime_type(
        &self,
        path: &Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send + '_>>;
}

pub struct LocalFsReadOperations;

#[async_trait]
impl ReadOperations for LocalFsReadOperations {
    fn read_file(
        &self,
        path: &Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, String>> + Send + '_>>
    {
        let path = path.to_path_buf();
        Box::pin(async move { fs::read(&path).await.map_err(|e| e.to_string()) })
    }

    fn access(
        &self,
        path: &Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + '_>> {
        let path = path.to_path_buf();
        Box::pin(async move {
            fs::metadata(&path).await.map_err(|e| e.to_string())?;
            Ok(())
        })
    }

    fn detect_image_mime_type(
        &self,
        path: &Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send + '_>> {
        let mime = detect_image_mime_type(path);
        Box::pin(async move { mime })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_lines(count: usize) -> Vec<String> {
        (1..=count).map(|i| format!("line {i}")).collect()
    }

    #[test]
    fn test_truncate_basic() {
        let lines = make_lines(100);
        let result = truncate_content(&lines, 0, 1, None, 2000, 50 * 1024);
        assert!(!result.truncated);
        assert_eq!(result.total_lines, 100);
        assert_eq!(result.output_lines, 100);
    }

    #[test]
    fn test_truncate_by_lines_limit() {
        let lines = make_lines(3000);
        let result = truncate_content(&lines, 0, 1, None, 2000, 50 * 1024 * 1024);
        assert!(result.truncated);
        assert_eq!(result.truncated_by, Some(TruncatedBy::Lines));
        assert_eq!(result.output_lines, 2000);
    }

    #[test]
    fn test_truncate_with_offset() {
        let lines = make_lines(100);
        let result = truncate_content(&lines, 0, 50, None, 2000, 50 * 1024 * 1024);
        assert!(!result.truncated);
        assert_eq!(result.output_lines, 51);
        assert!(result.content.starts_with("50: line 50"));
    }

    #[test]
    fn test_truncate_offset_beyond_end() {
        let lines = make_lines(10);
        let result = truncate_content(&lines, 0, 999, None, 2000, 50 * 1024);
        assert!(!result.truncated);
        assert_eq!(result.output_lines, 0);
        assert!(result.content.is_empty());
    }

    #[test]
    fn test_first_line_exceeds_limit() {
        let long_line = "x".repeat(100_000);
        let lines = vec![long_line];
        let result = truncate_content(&lines, 0, 1, None, 2000, 50 * 1024);
        assert!(result.first_line_exceeds_limit);
        assert!(result.content.is_empty());
    }

    #[test]
    fn test_truncate_by_bytes() {
        let lines: Vec<String> = (1..=3000)
            .map(|i| format!("this is a somewhat long line number {i} with content"))
            .collect();
        let result = truncate_content(&lines, 0, 1, None, 2000, 1024);
        assert!(result.truncated);
        assert!(result.output_lines < 2000);
    }

    #[test]
    fn test_detect_image_mime_type() {
        assert_eq!(
            detect_image_mime_type(Path::new("test.png")),
            Some("image/png".to_string())
        );
        assert_eq!(
            detect_image_mime_type(Path::new("photo.JPG")),
            Some("image/jpeg".to_string())
        );
        assert_eq!(
            detect_image_mime_type(Path::new("animation.gif")),
            Some("image/gif".to_string())
        );
        assert_eq!(detect_image_mime_type(Path::new("doc.txt")), None);
        assert_eq!(detect_image_mime_type(Path::new("noext")), None);
    }

    #[test]
    fn test_is_likely_binary() {
        assert!(is_likely_binary(&[0u8; 100]));
        assert!(!is_likely_binary(b"hello world\n".repeat(100).as_slice()));
        assert!(!is_likely_binary(b""));
    }

    #[test]
    fn test_parse_request_valid() {
        let tool = FileReadTool::default();
        let args = json!({
            "path": "/tmp/test.txt",
            "offset": 10,
            "limit": 50
        });
        let req = tool.parse_request(args).unwrap();
        assert_eq!(req.path, "/tmp/test.txt");
        assert_eq!(req.offset, Some(10));
        assert_eq!(req.limit, Some(50));
    }

    #[test]
    fn test_parse_request_minimal() {
        let tool = FileReadTool::default();
        let args = json!({ "path": "foo.rs" });
        let req = tool.parse_request(args).unwrap();
        assert_eq!(req.path, "foo.rs");
        assert!(req.offset.is_none());
        assert!(req.limit.is_none());
    }

    #[test]
    fn test_parse_request_unknown_field_rejected() {
        let tool = FileReadTool::default();
        let args = json!({ "path": "test.rs", "unknown": true });
        let result = tool.parse_request(args);
        assert!(result.is_err());
    }

    #[test]
    fn test_file_read_config_default() {
        let config = FileReadConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_lines, 2000);
        assert_eq!(config.max_bytes, 50 * 1024);
        assert!(config.auto_resize_images);
    }
}
