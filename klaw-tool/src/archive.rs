use async_trait::async_trait;
use klaw_archive::{ArchiveRecord, ArchiveService, open_default_archive_service};
use klaw_config::AppConfig;
use klaw_storage::{DefaultSessionStore, SessionStorage};
use klaw_util::{default_data_dir, workspace_dir};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tokio::fs;

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

const META_CURRENT_ATTACHMENTS_KEY: &str = "agent.current_attachments";
const DEFAULT_READ_MAX_CHARS: usize = 16_000;
const MAX_READ_MAX_CHARS: usize = 200_000;
const DEFAULT_SESSION_ATTACHMENT_LIMIT: i64 = 50;

pub struct ArchiveTool {
    service: Arc<dyn ArchiveService>,
    session_store: Arc<DefaultSessionStore>,
    storage_root_dir: Option<String>,
}

struct ResolvedSessionScope {
    base_session_key: String,
    session_keys: Vec<String>,
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
    pub async fn open_default(
        config: &AppConfig,
        session_store: DefaultSessionStore,
    ) -> Result<Self, ToolError> {
        let service = open_default_archive_service().await.map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to open archive service: {err}"))
        })?;
        Ok(Self {
            service: Arc::new(service),
            session_store: Arc::new(session_store),
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
            default_data_dir().ok_or_else(|| {
                ToolError::ExecutionFailed("failed to resolve home dir".to_string())
            })?
        };
        let workspace = workspace_dir(&root);
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

    async fn resolve_session_scope(
        &self,
        ctx: &ToolContext,
    ) -> Result<ResolvedSessionScope, ToolError> {
        let from_metadata = ctx
            .metadata
            .get("channel.base_session_key")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let base_session_key = match from_metadata {
            Some(key) => key,
            None => self
                .session_store
                .get_session_by_active_session_key(&ctx.session_key)
                .await
                .map(|base| base.session_key)
                .unwrap_or_else(|_| ctx.session_key.clone()),
        };

        let active_session_key = self
            .session_store
            .get_session(&base_session_key)
            .await
            .ok()
            .and_then(|session| session.active_session_key)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let mut session_keys = vec![base_session_key.clone()];
        if let Some(active_session_key) = active_session_key {
            if active_session_key != base_session_key {
                session_keys.push(active_session_key);
            }
        }
        if ctx.session_key != base_session_key && !session_keys.contains(&ctx.session_key) {
            session_keys.push(ctx.session_key.clone());
        }
        Ok(ResolvedSessionScope {
            base_session_key,
            session_keys,
        })
    }

    async fn session_attachments(
        &self,
        ctx: &ToolContext,
    ) -> Result<(ResolvedSessionScope, Vec<Value>), ToolError> {
        let scope = self.resolve_session_scope(ctx).await?;
        let mut attachments = Vec::new();
        let mut seen_ids = HashSet::new();
        for session_key in &scope.session_keys {
            let remaining =
                DEFAULT_SESSION_ATTACHMENT_LIMIT.saturating_sub(attachments.len() as i64);
            if remaining <= 0 {
                break;
            }
            let records = self
                .service
                .find(klaw_archive::ArchiveQuery {
                    session_key: Some(session_key.clone()),
                    limit: remaining,
                    ..Default::default()
                })
                .await
                .map_err(|err| {
                    ToolError::ExecutionFailed(format!(
                        "failed to list session archive records for `{session_key}`: {err}"
                    ))
                })?;
            for record in records {
                if seen_ids.insert(record.id.clone()) {
                    attachments.push(Self::archive_record_to_json(&record));
                }
            }
        }
        Ok((scope, attachments))
    }
}

#[async_trait]
impl Tool for ArchiveTool {
    fn name(&self) -> &str {
        "archive"
    }

    fn description(&self) -> &str {
        "Inspect archived attachments for the current conversation. Prefer `get` when the current message already includes an `archive_id`. Use `list_current_attachments` only for current-message attachments, and `list_session_attachments` for archived attachments from the current conversation's session chain."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Read-only archive access and copy-to-workspace operations. Prefer `get` when you already have an `archive_id`. Use `list_current_attachments` only to confirm attachments from the current user message, and `list_session_attachments` to inspect archived attachments from the broader current conversation session chain. Never modify files under archives/ directly; use copy_to_workspace first if you need to transform a file.",
            "oneOf": [
                {
                    "description": "Inspect one archive record by archive id. Prefer this when the current message summary already includes an `archive_id`.",
                    "properties": {
                        "action": { "const": "get" },
                        "archive_id": {
                            "type": "string",
                            "description": "Exact archive record id, usually taken from the current attachment summary. Pass the id exactly without extra punctuation."
                        }
                    },
                    "required": ["action", "archive_id"],
                    "additionalProperties": false
                },
                {
                    "description": "List archived attachments from the current user message context only. Do not use this for historical session attachments.",
                    "properties": {
                        "action": { "const": "list_current_attachments" }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                },
                {
                    "description": "List archived attachments from the current session across prior turns. Use this when the user refers to earlier files from the same conversation.",
                    "properties": {
                        "action": { "const": "list_session_attachments" }
                    },
                    "required": ["action"],
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
            "list_current_attachments" => json!({
                "action": request.action,
                "attachments": Self::current_attachments(ctx)?,
                "archives_are_read_only": true,
                "workflow": "copy_to_workspace_before_edit",
                "scope": "current_message_only",
            }),
            "list_session_attachments" => {
                let (scope, attachments) = self.session_attachments(ctx).await?;
                json!({
                    "action": request.action,
                    "attachments": attachments,
                    "archives_are_read_only": true,
                    "scope": "current_session",
                    "session_key": ctx.session_key,
                    "base_session_key": scope.base_session_key,
                    "queried_session_keys": scope.session_keys,
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
                "`action` must be one of get/list_current_attachments/list_session_attachments/read_text/copy_to_workspace"
                    .to_string(),
            )),
        };

        Ok(ToolOutput {
            content_for_model: serde_json::to_string_pretty(&payload).map_err(|err| {
                ToolError::ExecutionFailed(format!("failed to serialize archive response: {err}"))
            })?,
            content_for_user: None,
            signals: Vec::new(),
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
    use klaw_storage::{DefaultSessionStore, SessionStorage, StoragePaths};
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeArchiveService {
        record: Mutex<Option<ArchiveRecord>>,
        find_records: Mutex<Vec<ArchiveRecord>>,
        find_session_keys: Mutex<Vec<Option<String>>>,
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

        async fn find(&self, query: ArchiveQuery) -> Result<Vec<ArchiveRecord>, ArchiveError> {
            self.find_session_keys
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(query.session_key.clone());
            Ok(self
                .find_records
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .clone())
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

        async fn list_session_keys(&self) -> Result<Vec<String>, ArchiveError> {
            Ok(Vec::new())
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

    async fn create_store(prefix: &str) -> DefaultSessionStore {
        DefaultSessionStore::open(StoragePaths::from_root(temp_root(prefix)))
            .await
            .expect("session store should open")
    }

    #[tokio::test]
    async fn list_current_attachments_reads_context_metadata() {
        let tool = ArchiveTool {
            service: Arc::new(FakeArchiveService::default()),
            session_store: Arc::new(create_store("current").await),
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
        assert!(
            output
                .content_for_model
                .contains("\"archive_id\": \"arch-1\"")
        );
        assert!(
            output
                .content_for_model
                .contains("\"archives_are_read_only\": true")
        );
        assert!(
            output
                .content_for_model
                .contains("\"scope\": \"current_message_only\"")
        );
    }

    #[tokio::test]
    async fn list_session_attachments_reads_current_session_records() {
        let service = Arc::new(FakeArchiveService {
            find_records: Mutex::new(vec![ArchiveRecord {
                id: "arch-session-1".to_string(),
                source_kind: ArchiveSourceKind::ChannelInbound,
                media_kind: ArchiveMediaKind::Image,
                mime_type: Some("image/png".to_string()),
                extension: Some("png".to_string()),
                original_filename: Some("screen.png".to_string()),
                content_sha256: "hash-1".to_string(),
                size_bytes: 42,
                storage_rel_path: "archives/2026-03-24/arch-session-1.png".to_string(),
                session_key: Some("im:chat-1".to_string()),
                channel: Some("dingtalk".to_string()),
                chat_id: Some("chat-1".to_string()),
                message_id: Some("msg-1".to_string()),
                metadata_json: "{}".to_string(),
                created_at_ms: 1,
            }]),
            ..Default::default()
        });
        let tool = ArchiveTool {
            service: service.clone(),
            session_store: Arc::new(create_store("session").await),
            storage_root_dir: None,
        };
        let output = tool
            .execute(json!({"action": "list_session_attachments"}), &base_ctx())
            .await
            .expect("list session attachments");
        assert!(
            output
                .content_for_model
                .contains("\"scope\": \"current_session\"")
        );
        assert!(
            output
                .content_for_model
                .contains("\"session_key\": \"im:chat-1\"")
        );
        assert!(
            output
                .content_for_model
                .contains("\"base_session_key\": \"im:chat-1\"")
        );
        assert!(
            output
                .content_for_model
                .contains("\"id\": \"arch-session-1\"")
        );
        let queried = service
            .find_session_keys
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        assert_eq!(queried, vec![Some("im:chat-1".to_string())]);
    }

    #[tokio::test]
    async fn list_session_attachments_includes_base_and_active_session_records() {
        let store = create_store("session-scope").await;
        store
            .get_or_create_session_state(
                "dingtalk:base",
                "chat-1",
                "dingtalk",
                "test-provider",
                "test-model",
            )
            .await
            .expect("base session should exist");
        store
            .get_or_create_session_state(
                "dingtalk:base:active",
                "chat-1",
                "dingtalk",
                "test-provider",
                "test-model",
            )
            .await
            .expect("active session should exist");
        store
            .set_active_session(
                "dingtalk:base",
                "chat-1",
                "dingtalk",
                "dingtalk:base:active",
            )
            .await
            .expect("base session should point to active session");
        let service = Arc::new(FakeArchiveService {
            find_records: Mutex::new(vec![
                ArchiveRecord {
                    id: "arch-base-1".to_string(),
                    source_kind: ArchiveSourceKind::ChannelInbound,
                    media_kind: ArchiveMediaKind::Other,
                    mime_type: Some("application/pdf".to_string()),
                    extension: Some("pdf".to_string()),
                    original_filename: Some("base.pdf".to_string()),
                    content_sha256: "hash-base".to_string(),
                    size_bytes: 10,
                    storage_rel_path: "archives/2026-03-24/base.pdf".to_string(),
                    session_key: Some("dingtalk:base".to_string()),
                    channel: Some("dingtalk".to_string()),
                    chat_id: Some("chat-1".to_string()),
                    message_id: Some("msg-base".to_string()),
                    metadata_json: "{}".to_string(),
                    created_at_ms: 1,
                },
                ArchiveRecord {
                    id: "arch-active-1".to_string(),
                    source_kind: ArchiveSourceKind::ChannelInbound,
                    media_kind: ArchiveMediaKind::Other,
                    mime_type: Some("application/vnd.apple.pages".to_string()),
                    extension: Some("pages".to_string()),
                    original_filename: Some("active.pages".to_string()),
                    content_sha256: "hash-active".to_string(),
                    size_bytes: 11,
                    storage_rel_path: "archives/2026-03-24/active.pages".to_string(),
                    session_key: Some("dingtalk:base:active".to_string()),
                    channel: Some("dingtalk".to_string()),
                    chat_id: Some("chat-1".to_string()),
                    message_id: Some("msg-active".to_string()),
                    metadata_json: "{}".to_string(),
                    created_at_ms: 2,
                },
            ]),
            ..Default::default()
        });
        let tool = ArchiveTool {
            service: service.clone(),
            session_store: Arc::new(store),
            storage_root_dir: None,
        };
        let ctx = ToolContext {
            session_key: "dingtalk:base:active".to_string(),
            metadata: std::collections::BTreeMap::new(),
        };
        let output = tool
            .execute(json!({"action": "list_session_attachments"}), &ctx)
            .await
            .expect("list session attachments should succeed");
        assert!(output.content_for_model.contains("\"id\": \"arch-base-1\""));
        assert!(
            output
                .content_for_model
                .contains("\"id\": \"arch-active-1\"")
        );
        assert!(
            output
                .content_for_model
                .contains("\"base_session_key\": \"dingtalk:base\"")
        );
        assert!(
            output
                .content_for_model
                .contains("\"queried_session_keys\": [")
        );
        let queried = service
            .find_session_keys
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        assert_eq!(
            queried,
            vec![
                Some("dingtalk:base".to_string()),
                Some("dingtalk:base:active".to_string()),
            ]
        );
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
            find_records: Mutex::new(Vec::new()),
            find_session_keys: Mutex::new(Vec::new()),
            bytes: Mutex::new(b"hello archive".to_vec()),
            absolute_path: Mutex::new(Some(source)),
        };
        let tool = ArchiveTool {
            service: Arc::new(service),
            session_store: Arc::new(create_store("copy").await),
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
        assert!(
            output
                .content_for_model
                .contains("\"next_step\": \"edit_or_transform_the_workspace_copy_only\"")
        );
    }
}
