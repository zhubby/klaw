use async_trait::async_trait;
use klaw_archive::{
    ArchiveMediaKind, ArchiveService, SqliteArchiveService, open_default_archive_service,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

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

    fn parse_request(args: Value) -> Result<ChannelAttachmentRequest, ToolError> {
        let Value::Object(mut object) = args else {
            return Err(ToolError::InvalidArgs(
                "request must be an object".to_string(),
            ));
        };

        let archive_id = parse_archive_id(&mut object)?;
        let mut request: ChannelAttachmentRequest =
            serde_json::from_value(Value::Object(object)).map_err(|err| {
                ToolError::InvalidArgs(format!("invalid request: {err}"))
            })?;
        request.archive_id = archive_id;
        Ok(request)
    }
}

fn parse_archive_id(object: &mut Map<String, Value>) -> Result<String, ToolError> {
    let Some(value) = object.remove("archive_id") else {
        return Err(ToolError::InvalidArgs(
            "`archive_id` is required and must be the exact archive id string like `arch_123`, not an attachment number such as `1`".to_string(),
        ));
    };

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

    object.insert("archive_id".to_string(), Value::String(archive_id.clone()));
    Ok(archive_id)
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
        "Queue one archived file for delivery back to the current chat channel. Use this only after you already have an exact `archive_id` string such as `arch_123`; do not pass attachment numbers like `1` or local filesystem paths."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Send one archived attachment back to the current chat. Prefer `kind=image` for screenshots or images that should render inline; use `kind=file` for documents, zip files, and other generic files. If omitted, `kind=auto` uses the archive media type. `archive_id` must be the literal archive id string returned by the archive or attachment context, not a numeric list index.",
            "properties": {
                "archive_id": {
                    "type": "string",
                    "description": "Exact archive record id to send, e.g. `arch_123`. Copy it verbatim from the archive tool or from current message attachments. Do not pass an attachment number like `1`."
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

        assert!(error
            .to_string()
            .contains("not an attachment number such as `1`"));
    }
}
