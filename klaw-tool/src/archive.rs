use async_trait::async_trait;
use klaw_archive::{open_default_archive_service, ArchiveRecord, ArchiveService};
use klaw_config::AppConfig;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tokio::fs;

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

const META_CURRENT_ATTACHMENTS_KEY: &str = "agent.current_attachments";
const DEFAULT_READ_MAX_CHARS: usize = 16_000;
const MAX_READ_MAX_CHARS: usize = 200_000;

pub struct ArchiveTool {
    service: Arc<dyn ArchiveService>,
    storage_root_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchiveRequest {
    action: String,
    #[serde(default)]
    archive_id: Option<String>,
    #[serde(default)]
    destination_path: Option<String>,
    #[serde(default)]
    max_chars: Option<usize>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CurrentAttachment {
    archive_id: String,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    storage_rel_path: Option<String>,
    #[serde(default)]
    size_bytes: Option<i64>,
    access: String,
    recommended_workflow: String,
    #[serde(default)]
    source_kind: Option<String>,
    #[serde(default)]
    message_id: Option<String>,
}

impl ArchiveTool {
    pub async fn open_default(config: &AppConfig) -> Result<Self, ToolError> {
        let service = open_default_archive_service().await.map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to open archive service: {err}"))
        })?;
        Ok(Self {
            service: Arc::new(service),
            storage_root_dir: config.storage.root_dir.clone(),
        })
    }

    fn parse_request(args: Value) -> Result<ArchiveRequest, ToolError> {
        let mut request: ArchiveRequest = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("invalid request: {err}")))?;
        request.action = request.action.trim().to_string();
        if request.action.is_empty() {
            return Err(ToolError::InvalidArgs(
                "`action` cannot be empty".to_string(),
            ));
        }
        if let Some(archive_id) = request.archive_id.as_mut() {
            *archive_id = archive_id.trim().to_string();
            if archive_id.is_empty() {
                return Err(ToolError::InvalidArgs(
                    "`archive_id` cannot be empty".to_string(),
                ));
            }
        }
        if let Some(destination_path) = request.destination_path.as_mut() {
            *destination_path = destination_path.trim().to_string();
            if destination_path.is_empty() {
                return Err(ToolError::InvalidArgs(
                    "`destination_path` cannot be empty".to_string(),
                ));
            }
        }
        if let Some(max_chars) = request.max_chars {
            if max_chars == 0 || max_chars > MAX_READ_MAX_CHARS {
                return Err(ToolError::InvalidArgs(format!(
                    "`max_chars` must be between 1 and {MAX_READ_MAX_CHARS}"
                )));
            }
        }
        Ok(request)
    }

    fn require_archive_id(request: &ArchiveRequest) -> Result<&str, ToolError> {
        request
            .archive_id
            .as_deref()
            .ok_or_else(|| ToolError::InvalidArgs("missing `archive_id`".to_string()))
    }

    fn current_attachments(ctx: &ToolContext) -> Result<Vec<CurrentAttachment>, ToolError> {
        match ctx.metadata.get(META_CURRENT_ATTACHMENTS_KEY) {
            Some(Value::Array(items)) => items
                .iter()
                .cloned()
                .map(|item| {
                    serde_json::from_value(item).map_err(|err| {
                        ToolError::ExecutionFailed(format!(
                            "failed to parse current attachment metadata: {err}"
                        ))
                    })
                })
                .collect(),
            Some(_) => Err(ToolError::ExecutionFailed(
                "current attachment metadata must be an array".to_string(),
            )),
            None => Ok(Vec::new()),
        }
    }

    fn resolve_workspace_root(&self, ctx: &ToolContext) -> Result<PathBuf, ToolError> {
        if let Some(workspace) = ctx.metadata.get("workspace").and_then(Value::as_str) {
            return std::fs::canonicalize(workspace).map_err(|err| {
                ToolError::ExecutionFailed(format!("invalid workspace path: {err}"))
            });
        }

        let root = if let Some(root) = self
            .storage_root_dir
            .as_deref()
            .map(str::trim)
            .filter(|root| !root.is_empty())
        {
            PathBuf::from(root)
        } else {
            let home = std::env::var("HOME").map_err(|err| {
                ToolError::ExecutionFailed(format!("failed to resolve home dir: {err}"))
            })?;
            PathBuf::from(home).join(".klaw")
        };
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).map_err(|err| {
            ToolError::ExecutionFailed(format!(
                "failed to ensure workspace `{}`: {err}",
                workspace.display()
            ))
        })?;
        std::fs::canonicalize(&workspace).map_err(|err| {
            ToolError::ExecutionFailed(format!(
                "failed to resolve workspace `{}`: {err}",
                workspace.display()
            ))
        })
    }

    fn default_copy_name(record: &ArchiveRecord) -> String {
        if let Some(filename) = record
            .original_filename
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return filename.to_string();
        }

        match record
            .extension
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(ext) => format!("archive-{}.{}", record.id, ext),
            None => format!("archive-{}", record.id),
        }
    }

    fn resolve_destination_path(base: &Path, destination: &str) -> Result<PathBuf, ToolError> {
        let raw = PathBuf::from(destination);
        if raw.is_absolute() {
            return Err(ToolError::InvalidArgs(
                "`destination_path` must be relative to workspace".to_string(),
            ));
        }
        if raw.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(ToolError::InvalidArgs(
                "`destination_path` cannot escape workspace".to_string(),
            ));
        }
        Ok(base.join(raw))
    }

    fn archive_record_to_json(record: &ArchiveRecord) -> Value {
        json!({
            "id": record.id,
            "source_kind": record.source_kind,
            "media_kind": record.media_kind,
            "mime_type": record.mime_type,
            "extension": record.extension,
            "original_filename": record.original_filename,
            "content_sha256": record.content_sha256,
            "size_bytes": record.size_bytes,
            "storage_rel_path": record.storage_rel_path,
            "session_key": record.session_key,
            "channel": record.channel,
            "chat_id": record.chat_id,
            "message_id": record.message_id,
            "created_at_ms": record.created_at_ms,
        })
    }
}

#[async_trait]
impl Tool for ArchiveTool {
    fn name(&self) -> &str {
        "archive"
    }

    fn description(&self) -> &str {
        "Inspect archived attachments for the current conversation. Archives are read-only source material. Use this tool to list current archived attachments, inspect archive metadata, read text-like archived files, or copy an archived file into workspace before editing."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Read-only archive access and copy-to-workspace operations. Never modify files under archives/ directly; use copy_to_workspace first if you need to transform a file.",
            "oneOf": [
                {
                    "description": "List archived attachments from the current user message context.",
                    "properties": {
                        "action": { "const": "list_current_attachments" }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                },
                {
                    "description": "Inspect one archive record by archive id.",
                    "properties": {
                        "action": { "const": "get" },
                        "archive_id": {
                            "type": "string",
                            "description": "Archive record id, usually taken from the current attachment summary."
                        }
                    },
                    "required": ["action", "archive_id"],
                    "additionalProperties": false
                },
                {
                    "description": "Read a text-like archived file without modifying it. Use for markdown, code, JSON, plain text, and other UTF-8 text content.",
                    "properties": {
                        "action": { "const": "read_text" },
                        "archive_id": { "type": "string" },
                        "max_chars": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": MAX_READ_MAX_CHARS,
                            "default": DEFAULT_READ_MAX_CHARS
                        }
                    },
                    "required": ["action", "archive_id"],
                    "additionalProperties": false
                },
                {
                    "description": "Copy an archived file into workspace so later tools can safely edit or transform the copied file.",
                    "properties": {
                        "action": { "const": "copy_to_workspace" },
                        "archive_id": { "type": "string" },
                        "destination_path": {
                            "type": "string",
                            "description": "Optional relative path inside workspace for the copied file. If omitted, a filename is derived from archive metadata."
                        }
                    },
                    "required": ["action", "archive_id"],
                    "additionalProperties": false
                }
            ]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::FilesystemWrite
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let request = Self::parse_request(args)?;
        let payload = match request.action.as_str() {
            "list_current_attachments" => json!({
                "action": request.action,
                "attachments": Self::current_attachments(ctx)?,
                "archives_are_read_only": true,
                "workflow": "copy_to_workspace_before_edit",
            }),
            "get" => {
                let archive_id = Self::require_archive_id(&request)?;
                let record = self.service.get(archive_id).await.map_err(|err| {
                    ToolError::ExecutionFailed(format!("failed to get archive record: {err}"))
                })?;
                json!({
                    "action": request.action,
                    "record": Self::archive_record_to_json(&record),
                    "archives_are_read_only": true,
                    "workflow": "copy_to_workspace_before_edit",
                })
            }
            "read_text" => {
                let archive_id = Self::require_archive_id(&request)?;
                let max_chars = request.max_chars.unwrap_or(DEFAULT_READ_MAX_CHARS);
                let blob = self
                    .service
                    .open_download(archive_id)
                    .await
                    .map_err(|err| {
                        ToolError::ExecutionFailed(format!("failed to open archive file: {err}"))
                    })?;
                let text = std::str::from_utf8(&blob.bytes).map_err(|_| {
                    ToolError::ExecutionFailed(
                        "archive file is not UTF-8 text; keep it read-only or copy it into workspace before using other file tools".to_string(),
                    )
                })?;
                let mut truncated = false;
                let content: String = text.chars().take(max_chars + 1).collect::<String>();
                let content = if content.chars().count() > max_chars {
                    truncated = true;
                    content.chars().take(max_chars).collect::<String>()
                } else {
                    content
                };
                json!({
                    "action": request.action,
                    "record": Self::archive_record_to_json(&blob.record),
                    "absolute_path": blob.absolute_path.display().to_string(),
                    "content": content,
                    "truncated": truncated,
                    "archives_are_read_only": true,
                })
            }
            "copy_to_workspace" => {
                let archive_id = Self::require_archive_id(&request)?;
                let blob = self
                    .service
                    .open_download(archive_id)
                    .await
                    .map_err(|err| {
                        ToolError::ExecutionFailed(format!("failed to open archive file: {err}"))
                    })?;
                let workspace = self.resolve_workspace_root(ctx)?;
                let destination_name = request
                    .destination_path
                    .clone()
                    .unwrap_or_else(|| Self::default_copy_name(&blob.record));
                let destination = Self::resolve_destination_path(&workspace, &destination_name)?;
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent).await.map_err(|err| {
                        ToolError::ExecutionFailed(format!(
                            "failed to create workspace directory `{}`: {err}",
                            parent.display()
                        ))
                    })?;
                }
                fs::write(&destination, &blob.bytes).await.map_err(|err| {
                    ToolError::ExecutionFailed(format!(
                        "failed to write copied archive file `{}`: {err}",
                        destination.display()
                    ))
                })?;
                let relative_path = destination
                    .strip_prefix(&workspace)
                    .ok()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| destination.display().to_string());
                json!({
                    "action": request.action,
                    "archive_id": archive_id,
                    "source_storage_rel_path": blob.record.storage_rel_path,
                    "workspace_path": destination.display().to_string(),
                    "workspace_rel_path": relative_path,
                    "next_step": "edit_or_transform_the_workspace_copy_only",
                })
            }
            _ => return Err(ToolError::InvalidArgs(
                "`action` must be one of list_current_attachments/get/read_text/copy_to_workspace"
                    .to_string(),
            )),
        };

        Ok(ToolOutput {
            content_for_model: serde_json::to_string_pretty(&payload).map_err(|err| {
                ToolError::ExecutionFailed(format!("failed to serialize archive response: {err}"))
            })?,
            content_for_user: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_archive::{
        ArchiveBlob, ArchiveError, ArchiveIngestInput, ArchiveMediaKind, ArchiveQuery,
        ArchiveSourceKind,
    };
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeArchiveService {
        record: Mutex<Option<ArchiveRecord>>,
        bytes: Mutex<Vec<u8>>,
        absolute_path: Mutex<Option<PathBuf>>,
    }

    #[async_trait]
    impl ArchiveService for FakeArchiveService {
        async fn ingest_path(
            &self,
            _input: ArchiveIngestInput,
            _source_path: &Path,
        ) -> Result<ArchiveRecord, ArchiveError> {
            unreachable!()
        }

        async fn ingest_bytes(
            &self,
            _input: ArchiveIngestInput,
            _bytes: &[u8],
        ) -> Result<ArchiveRecord, ArchiveError> {
            unreachable!()
        }

        async fn find(&self, _query: ArchiveQuery) -> Result<Vec<ArchiveRecord>, ArchiveError> {
            unreachable!()
        }

        async fn get(&self, _archive_id: &str) -> Result<ArchiveRecord, ArchiveError> {
            Ok(self
                .record
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .clone()
                .expect("record should exist"))
        }

        async fn open_download(&self, _archive_id: &str) -> Result<ArchiveBlob, ArchiveError> {
            Ok(ArchiveBlob {
                record: self
                    .record
                    .lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .clone()
                    .expect("record should exist"),
                absolute_path: self
                    .absolute_path
                    .lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .clone()
                    .expect("absolute path should exist"),
                bytes: self
                    .bytes
                    .lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .clone(),
            })
        }
    }

    fn temp_root(prefix: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "klaw-archive-tool-{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("temp root");
        root
    }

    fn base_ctx() -> ToolContext {
        ToolContext {
            session_key: "im:chat-1".to_string(),
            metadata: std::collections::BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn list_current_attachments_reads_context_metadata() {
        let tool = ArchiveTool {
            service: Arc::new(FakeArchiveService::default()),
            storage_root_dir: None,
        };
        let mut ctx = base_ctx();
        ctx.metadata.insert(
            META_CURRENT_ATTACHMENTS_KEY.to_string(),
            json!([{
                "archive_id": "arch-1",
                "filename": "report.pdf",
                "storage_rel_path": "archives/2026-03-20/arch-1.pdf",
                "access": "read_only",
                "recommended_workflow": "copy_to_workspace_before_edit"
            }]),
        );
        let output = tool
            .execute(json!({"action": "list_current_attachments"}), &ctx)
            .await
            .expect("list current attachments");
        assert!(output
            .content_for_model
            .contains("\"archive_id\": \"arch-1\""));
        assert!(output
            .content_for_model
            .contains("\"archives_are_read_only\": true"));
    }

    #[tokio::test]
    async fn copy_to_workspace_writes_workspace_copy() {
        let root = temp_root("copy");
        let source = root.join("archives").join("2026-03-20").join("arch-1.txt");
        std::fs::create_dir_all(source.parent().expect("parent")).expect("archive dir");
        std::fs::write(&source, "hello archive").expect("write source");
        let service = FakeArchiveService {
            record: Mutex::new(Some(ArchiveRecord {
                id: "arch-1".to_string(),
                source_kind: ArchiveSourceKind::ChannelInbound,
                media_kind: ArchiveMediaKind::Other,
                mime_type: Some("text/plain".to_string()),
                extension: Some("txt".to_string()),
                original_filename: Some("notes.txt".to_string()),
                content_sha256: "hash".to_string(),
                size_bytes: 13,
                storage_rel_path: "archives/2026-03-20/arch-1.txt".to_string(),
                session_key: None,
                channel: None,
                chat_id: None,
                message_id: None,
                metadata_json: "{}".to_string(),
                created_at_ms: 0,
            })),
            bytes: Mutex::new(b"hello archive".to_vec()),
            absolute_path: Mutex::new(Some(source)),
        };
        let tool = ArchiveTool {
            service: Arc::new(service),
            storage_root_dir: Some(root.to_string_lossy().to_string()),
        };

        let output = tool
            .execute(
                json!({"action": "copy_to_workspace", "archive_id": "arch-1"}),
                &base_ctx(),
            )
            .await
            .expect("copy to workspace");
        let copied = root.join("workspace").join("notes.txt");
        assert_eq!(
            std::fs::read_to_string(copied).expect("copied file"),
            "hello archive"
        );
        assert!(output
            .content_for_model
            .contains("\"next_step\": \"edit_or_transform_the_workspace_copy_only\""));
    }
}
