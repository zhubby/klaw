use async_trait::async_trait;
use klaw_archive::{ArchiveMediaKind, ArchiveService, open_default_archive_service};
use klaw_config::{AppConfig, LocalAttachmentConfig};
use klaw_util::{default_data_dir, workspace_dir};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput, ToolSignal};

pub struct ChannelAttachmentTool {
    local_attachments: LocalAttachmentConfig,
    workspace_root: PathBuf,
}

impl ChannelAttachmentTool {
    pub async fn open_default(config: &AppConfig) -> Result<Self, ToolError> {
        let root = default_data_dir()
            .ok_or_else(|| ToolError::ExecutionFailed("failed to resolve home dir".to_string()))?;
        let workspace = workspace_dir(&root);
        std::fs::create_dir_all(&workspace).map_err(|err| {
            ToolError::ExecutionFailed(format!(
                "failed to ensure workspace `{}`: {err}",
                workspace.display()
            ))
        })?;
        let workspace_root = std::fs::canonicalize(&workspace).map_err(|err| {
            ToolError::ExecutionFailed(format!(
                "failed to resolve workspace `{}`: {err}",
                workspace.display()
            ))
        })?;
        Ok(Self {
            local_attachments: config.tools.channel_attachment.local_attachments.clone(),
            workspace_root,
        })
    }

    fn normalize_kind(
        requested: OutboundAttachmentRequestKind,
        media_kind: ArchiveMediaKind,
    ) -> &'static str {
        match requested {
            OutboundAttachmentRequestKind::Auto => match media_kind {
                ArchiveMediaKind::Image => "image",
                _ => "file",
            },
            OutboundAttachmentRequestKind::Image => "image",
            OutboundAttachmentRequestKind::File => "file",
        }
    }

    fn parse_request(args: Value) -> Result<ChannelAttachmentRequest, ToolError> {
        let Value::Object(mut object) = args else {
            return Err(ToolError::InvalidArgs(
                "request must be an object".to_string(),
            ));
        };

        let source = parse_source(&mut object)?;
        let mut request: ChannelAttachmentRequest =
            serde_json::from_value(Value::Object(object))
                .map_err(|err| ToolError::InvalidArgs(format!("invalid request: {err}")))?;
        request.source = source;
        Ok(request)
    }

    fn local_policy_for_session(
        &self,
        session_key: &str,
        config: &LocalAttachmentConfig,
    ) -> Result<LocalAttachmentPolicy, ToolError> {
        let (_channel, _account_id) = parse_session_channel(session_key).ok_or_else(|| {
            ToolError::ExecutionFailed(format!(
                "failed to resolve channel attachment policy from session_key `{session_key}`"
            ))
        })?;

        Ok(LocalAttachmentPolicy {
            workspace_root: self.workspace_root.clone(),
            allowlist: config
                .allowlist
                .iter()
                .map(|path| PathBuf::from(path.trim()))
                .collect(),
            max_bytes: config.max_bytes,
        })
    }
}

fn parse_session_channel(session_key: &str) -> Option<(&str, &str)> {
    let mut segments = session_key.split(':');
    let channel = segments.next()?.trim();
    let account_id = segments.next()?.trim();
    (!channel.is_empty() && !account_id.is_empty()).then_some((channel, account_id))
}

fn parse_source(
    object: &mut Map<String, Value>,
) -> Result<OutboundAttachmentRequestSource, ToolError> {
    let archive_id = object.remove("archive_id");
    let path = object.remove("path");
    match (archive_id, path) {
        (Some(value), None) => {
            let archive_id = parse_archive_id_value(value)?;
            object.insert("archive_id".to_string(), Value::String(archive_id.clone()));
            Ok(OutboundAttachmentRequestSource::ArchiveId { archive_id })
        }
        (None, Some(value)) => {
            let path = parse_local_path_value(value)?;
            object.insert("path".to_string(), Value::String(path.clone()));
            Ok(OutboundAttachmentRequestSource::LocalPath { path })
        }
        (Some(_), Some(_)) => Err(ToolError::InvalidArgs(
            "exactly one of `archive_id` or `path` must be provided".to_string(),
        )),
        (None, None) => Err(ToolError::InvalidArgs(
            "missing attachment source: provide either `archive_id` or `path`".to_string(),
        )),
    }
}

fn parse_archive_id_value(value: Value) -> Result<String, ToolError> {
    let archive_id = match value {
        Value::String(value) => value.trim().to_string(),
        Value::Number(number) => {
            return Err(ToolError::InvalidArgs(format!(
                "`archive_id` must be the exact archive id string like `arch_123`, not an attachment number such as `{number}`"
            )));
        }
        other => {
            return Err(ToolError::InvalidArgs(format!(
                "`archive_id` must be a string like `arch_123`, got {other}"
            )));
        }
    };
    if archive_id.is_empty() {
        return Err(ToolError::InvalidArgs(
            "`archive_id` cannot be empty; pass the exact archive id string from the attachment context".to_string(),
        ));
    }
    Ok(archive_id)
}

fn parse_local_path_value(value: Value) -> Result<String, ToolError> {
    let path = match value {
        Value::String(value) => value.trim().to_string(),
        other => {
            return Err(ToolError::InvalidArgs(format!(
                "`path` must be an absolute filesystem path string, got {other}"
            )));
        }
    };
    if path.is_empty() {
        return Err(ToolError::InvalidArgs("`path` cannot be empty".to_string()));
    }
    if !path.starts_with('/') {
        return Err(ToolError::InvalidArgs(
            "`path` must be an absolute filesystem path".to_string(),
        ));
    }
    Ok(path)
}

#[derive(Debug, Clone)]
struct LocalAttachmentPolicy {
    workspace_root: PathBuf,
    allowlist: Vec<PathBuf>,
    max_bytes: u64,
}

impl LocalAttachmentPolicy {
    fn path_allowed(&self, candidate: &Path) -> bool {
        if candidate.starts_with(&self.workspace_root) {
            return true;
        }
        self.allowlist
            .iter()
            .any(|entry| candidate.starts_with(entry))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelAttachmentRequest {
    #[serde(skip)]
    source: OutboundAttachmentRequestSource,
    #[serde(default)]
    kind: OutboundAttachmentRequestKind,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    caption: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    archive_id: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Clone)]
enum OutboundAttachmentRequestSource {
    ArchiveId { archive_id: String },
    LocalPath { path: String },
}

impl Default for OutboundAttachmentRequestSource {
    fn default() -> Self {
        Self::ArchiveId {
            archive_id: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OutboundAttachmentRequestKind {
    #[default]
    Auto,
    Image,
    File,
}

#[async_trait]
impl Tool for ChannelAttachmentTool {
    fn name(&self) -> &str {
        "channel_attachment"
    }

    fn description(&self) -> &str {
        "Queue one file for delivery back to the current chat channel. Use either an exact `archive_id` string such as `arch_123`, or an absolute `path` that stays inside the workspace or the channel's configured local attachment allowlist."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Send one attachment back to the current chat. Provide exactly one source: either `archive_id` for archived files, or `path` for a local file that is inside the workspace or a configured allowlist. Prefer `kind=image` for screenshots or images that should render inline; use `kind=file` for documents, zip files, and other generic files.",
            "properties": {
                "archive_id": {
                    "type": "string",
                    "description": "Exact archive record id to send, e.g. `arch_123`. Copy it verbatim from the archive tool or current attachment context. Do not pass an attachment number like `1`."
                },
                "path": {
                    "type": "string",
                    "description": "Absolute local file path to send. The path must point to an existing file, not a directory, and must be inside the workspace or the channel's configured local attachment allowlist."
                },
                "kind": {
                    "type": "string",
                    "enum": ["auto", "image", "file"],
                    "default": "auto",
                    "description": "How the chat channel should send the attachment."
                },
                "filename": {
                    "type": "string",
                    "description": "Optional override filename shown to users."
                },
                "caption": {
                    "type": "string",
                    "description": "Optional short caption. For images this is shown before the image when supported."
                }
            },
            "anyOf": [
                { "required": ["archive_id"] },
                { "required": ["path"] }
            ],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Messaging
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let mut request = Self::parse_request(args)?;
        if let Some(filename) = request.filename.as_mut() {
            *filename = filename.trim().to_string();
            if filename.is_empty() {
                request.filename = None;
            }
        }
        if let Some(caption) = request.caption.as_mut() {
            *caption = caption.trim().to_string();
            if caption.is_empty() {
                request.caption = None;
            }
        }

        let (kind, source_for_signal, display_name) = match &request.source {
            OutboundAttachmentRequestSource::ArchiveId { archive_id } => {
                let archive = open_default_archive_service().await.map_err(|err| {
                    ToolError::ExecutionFailed(format!("failed to open archive service: {err}"))
                })?;
                let record = archive.get(archive_id).await.map_err(|err| {
                    ToolError::ExecutionFailed(format!("failed to load archive record: {err}"))
                })?;
                let kind = Self::normalize_kind(request.kind, record.media_kind);
                let display_name = request
                    .filename
                    .clone()
                    .or(record.original_filename.clone())
                    .unwrap_or_else(|| archive_id.clone());
                (kind, (Some(archive_id.as_str()), None), display_name)
            }
            OutboundAttachmentRequestSource::LocalPath { path } => {
                let policy =
                    self.local_policy_for_session(&ctx.session_key, &self.local_attachments)?;
                let canonical_path = std::fs::canonicalize(path).map_err(|err| {
                    ToolError::ExecutionFailed(format!(
                        "failed to resolve local attachment path `{path}`: {err}"
                    ))
                })?;
                let metadata = std::fs::metadata(&canonical_path).map_err(|err| {
                    ToolError::ExecutionFailed(format!(
                        "failed to read local attachment metadata `{}`: {err}",
                        canonical_path.display()
                    ))
                })?;
                if !metadata.is_file() {
                    return Err(ToolError::InvalidArgs(format!(
                        "`path` must point to a file, not `{}`",
                        canonical_path.display()
                    )));
                }
                let size_bytes = metadata.len();
                if size_bytes == 0 {
                    return Err(ToolError::InvalidArgs(format!(
                        "local attachment `{}` has no content",
                        canonical_path.display()
                    )));
                }
                if size_bytes > policy.max_bytes {
                    return Err(ToolError::InvalidArgs(format!(
                        "local attachment `{}` exceeds the configured max_bytes limit ({} > {})",
                        canonical_path.display(),
                        size_bytes,
                        policy.max_bytes
                    )));
                }
                if !policy.path_allowed(&canonical_path) {
                    return Err(ToolError::InvalidArgs(format!(
                        "local attachment `{}` is outside the workspace and the configured allowlist",
                        canonical_path.display()
                    )));
                }
                let display_name = request
                    .filename
                    .clone()
                    .or_else(|| {
                        canonical_path
                            .file_name()
                            .and_then(|value| value.to_str())
                            .map(ToOwned::to_owned)
                    })
                    .unwrap_or_else(|| canonical_path.display().to_string());
                (
                    match request.kind {
                        OutboundAttachmentRequestKind::Auto => "file",
                        OutboundAttachmentRequestKind::Image => "image",
                        OutboundAttachmentRequestKind::File => "file",
                    },
                    (None, Some(canonical_path.display().to_string())),
                    display_name,
                )
            }
        };

        let signal = ToolSignal::channel_attachment(
            kind,
            source_for_signal.0,
            source_for_signal.1.as_deref(),
            request.filename.as_deref(),
            request.caption.as_deref(),
        );

        Ok(ToolOutput {
            content_for_model: match &request.source {
                OutboundAttachmentRequestSource::ArchiveId { archive_id } => format!(
                    "Queued attachment for channel delivery: archive_id={archive_id}; kind={kind}; filename={display_name}"
                ),
                OutboundAttachmentRequestSource::LocalPath { .. } => format!(
                    "Queued attachment for channel delivery: path={}; kind={kind}; filename={display_name}",
                    source_for_signal.1.as_deref().unwrap_or("")
                ),
            },
            content_for_user: None,
            signals: vec![signal],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_kind_uses_archive_media_when_auto() {
        assert_eq!(
            ChannelAttachmentTool::normalize_kind(
                OutboundAttachmentRequestKind::Auto,
                ArchiveMediaKind::Image
            ),
            "image"
        );
        assert_eq!(
            ChannelAttachmentTool::normalize_kind(
                OutboundAttachmentRequestKind::Auto,
                ArchiveMediaKind::Other
            ),
            "file"
        );
    }

    #[test]
    fn parse_request_rejects_numeric_archive_id() {
        let error = ChannelAttachmentTool::parse_request(json!({
            "archive_id": 1,
            "kind": "file"
        }))
        .expect_err("numeric archive id should be rejected");

        assert!(
            error
                .to_string()
                .contains("not an attachment number such as `1`")
        );
    }

    #[test]
    fn parse_request_accepts_local_path() {
        let request = ChannelAttachmentTool::parse_request(json!({
            "path": "/tmp/demo.txt",
            "kind": "file"
        }))
        .expect("path request should parse");

        assert!(matches!(
            request.source,
            OutboundAttachmentRequestSource::LocalPath { ref path } if path == "/tmp/demo.txt"
        ));
    }
}
