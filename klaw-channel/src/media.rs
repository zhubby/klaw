use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
use klaw_archive::{ArchiveIngestInput, ArchiveRecord, ArchiveService, ArchiveSourceKind};
use klaw_core::{MediaReference, MediaSourceKind};
use serde_json::Value;
use std::collections::BTreeMap;

pub const DEFAULT_INLINE_MEDIA_MAX_BYTES: usize = 20 * 1024 * 1024;

pub struct ArchiveMediaIngestContext<'a> {
    pub session_key: &'a str,
    pub channel: &'a str,
    pub chat_id: &'a str,
    pub message_id: &'a str,
}

pub fn first_string_value(value: &Value, pointers: &[&str]) -> Option<String> {
    pointers.iter().find_map(|pointer| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
    })
}

pub fn first_object_string_value(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
    })
}

pub fn attach_declared_media_metadata(
    metadata: &mut BTreeMap<String, Value>,
    mime_type: Option<&str>,
    file_extension: Option<&str>,
    mime_key: &str,
    extension_key: &str,
) {
    if let Some(mime_type) = mime_type {
        metadata.insert(mime_key.to_string(), Value::String(mime_type.to_string()));
    }
    if let Some(file_extension) = file_extension {
        metadata.insert(
            extension_key.to_string(),
            Value::String(file_extension.to_string()),
        );
    }
}

pub fn build_media_reference(
    source_kind: MediaSourceKind,
    message_id: &str,
    filename: Option<String>,
    mime_type: Option<String>,
    metadata: BTreeMap<String, Value>,
) -> MediaReference {
    MediaReference {
        source_kind,
        filename,
        mime_type,
        remote_url: None,
        bytes: None,
        message_id: Some(message_id.to_string()),
        metadata,
    }
}

fn archive_source_kind_from_media(source_kind: MediaSourceKind) -> ArchiveSourceKind {
    match source_kind {
        MediaSourceKind::UserUpload => ArchiveSourceKind::UserUpload,
        MediaSourceKind::ChannelInbound => ArchiveSourceKind::ChannelInbound,
        MediaSourceKind::ModelGenerated => ArchiveSourceKind::ModelGenerated,
    }
}

pub fn resolve_metadata_value_candidates(
    metadata: &BTreeMap<String, Value>,
    candidates: &[(&str, &'static str)],
) -> Vec<(String, &'static str)> {
    let mut out = Vec::new();
    for (key, source_label) in candidates {
        if let Some(code) = metadata
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            out.push((code.to_string(), *source_label));
        }
    }
    out
}

pub async fn ingest_media_reference_bytes(
    archive_service: &dyn ArchiveService,
    context: ArchiveMediaIngestContext<'_>,
    media: &mut MediaReference,
    bytes: &[u8],
    inline_media_max_bytes: usize,
    inline_flag_key: &str,
    inline_skipped_bytes_key: &str,
) -> Result<ArchiveRecord, Box<dyn std::error::Error>> {
    let metadata = serde_json::to_value(media.metadata.clone()).unwrap_or(Value::Null);
    let ingest_input = ArchiveIngestInput {
        source_kind: archive_source_kind_from_media(media.source_kind),
        filename: media.filename.clone(),
        declared_mime_type: media.mime_type.clone(),
        session_key: Some(context.session_key.to_string()),
        channel: Some(context.channel.to_string()),
        chat_id: Some(context.chat_id.to_string()),
        message_id: Some(context.message_id.to_string()),
        metadata,
    };
    let record = archive_service.ingest_bytes(ingest_input, bytes).await?;

    media.message_id = Some(context.message_id.to_string());
    if media.filename.is_none() {
        media.filename = record.original_filename.clone();
    }
    if media.mime_type.is_none() {
        media.mime_type = record.mime_type.clone();
    }

    if bytes.len() <= inline_media_max_bytes {
        let mime_for_inline = media
            .mime_type
            .clone()
            .or_else(|| record.mime_type.clone())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        media.remote_url = Some(format!(
            "data:{mime_for_inline};base64,{}",
            BASE64_STANDARD.encode(bytes)
        ));
        media
            .metadata
            .insert(inline_flag_key.to_string(), Value::Bool(true));
    } else {
        media
            .metadata
            .insert(inline_flag_key.to_string(), Value::Bool(false));
        media.metadata.insert(
            inline_skipped_bytes_key.to_string(),
            Value::from(bytes.len() as i64),
        );
    }

    media
        .metadata
        .insert("archive.id".to_string(), Value::String(record.id.clone()));
    media.metadata.insert(
        "archive.storage_rel_path".to_string(),
        Value::String(record.storage_rel_path.clone()),
    );
    media.metadata.insert(
        "archive.size_bytes".to_string(),
        Value::from(record.size_bytes),
    );
    if let Some(mime_type) = record.mime_type.clone() {
        media
            .metadata
            .insert("archive.mime_type".to_string(), Value::String(mime_type));
    }

    Ok(record)
}
