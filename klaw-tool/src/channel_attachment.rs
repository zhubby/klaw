use async_trait::async_trait;
use klaw_archive::{
    ArchiveMediaKind, ArchiveService, SqliteArchiveService, open_default_archive_service,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput, ToolSignal};

pub struct ChannelAttachmentTool {
    archive: SqliteArchiveService,
}

impl ChannelAttachmentTool {
    pub async fn open_default() -> Result<Self, ToolError> {
        let archive = open_default_archive_service().await.map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to open archive service: {err}"))
        })?;
        Ok(Self { archive })
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
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelAttachmentRequest {
    archive_id: String,
    #[serde(default)]
    kind: OutboundAttachmentRequestKind,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    caption: Option<String>,
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
        "Queue one archived file for delivery back to the current chat channel. Use this after you already have an `archive_id` and want Telegram or DingTalk to send the file or display the image in-chat."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Send one archived attachment back to the current chat. Prefer `kind=image` for screenshots or images that should render inline; use `kind=file` for documents, zip files, and other generic files. If omitted, `kind=auto` uses the archive media type.",
            "properties": {
                "archive_id": {
                    "type": "string",
                    "description": "Exact archive record id to send, e.g. `arch_123`. Get this from the archive tool or from current message attachments."
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
            "required": ["archive_id"],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Messaging
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let mut request: ChannelAttachmentRequest = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("invalid request: {err}")))?;
        request.archive_id = request.archive_id.trim().to_string();
        if request.archive_id.is_empty() {
            return Err(ToolError::InvalidArgs(
                "`archive_id` cannot be empty".to_string(),
            ));
        }
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

        let record = self.archive.get(&request.archive_id).await.map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to load archive record: {err}"))
        })?;

        let kind = Self::normalize_kind(request.kind, record.media_kind);
        let signal = ToolSignal::channel_attachment(
            &request.archive_id,
            kind,
            request.filename.as_deref(),
            request.caption.as_deref(),
        );
        let filename = request
            .filename
            .clone()
            .or(record.original_filename.clone())
            .unwrap_or_else(|| request.archive_id.clone());

        Ok(ToolOutput {
            content_for_model: format!(
                "Queued attachment for channel delivery: archive_id={}; kind={}; filename={}",
                request.archive_id, kind, filename
            ),
            content_for_user: None,
            signals: vec![signal],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
