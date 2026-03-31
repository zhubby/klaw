use super::client::DingtalkApiClient;
use super::config::DingtalkChannelConfig;
use crate::{
    LocalAttachmentPolicy, OutboundAttachment, OutboundAttachmentKind,
    outbound::resolve_outbound_attachment,
};
use klaw_archive::open_default_archive_service;
use std::path::Path;
use tracing::{debug, warn};

pub(super) async fn deliver_dingtalk_attachments(
    client: DingtalkApiClient,
    config: DingtalkChannelConfig,
    session_webhook: String,
    chat_id: String,
    attachments: Vec<OutboundAttachment>,
    local_policy: LocalAttachmentPolicy,
) {
    let archive_service = match open_default_archive_service().await {
        Ok(service) => service,
        Err(error) => {
            warn!(
                chat_id,
                error = %error,
                "failed to open archive service for dingtalk outbound attachments"
            );
            return;
        }
    };

    let access_token = match client
        .fetch_access_token(&config.client_id, &config.client_secret)
        .await
    {
        Ok(token) => token,
        Err(error) => {
            warn!(
                chat_id,
                error = %error,
                "failed to fetch dingtalk access token for outbound attachments"
            );
            return;
        }
    };

    for attachment in &attachments {
        let resolved =
            match resolve_outbound_attachment(&archive_service, &local_policy, attachment).await {
                Ok(resolved) => resolved,
                Err(error) => {
                    warn!(
                        chat_id,
                        source = ?attachment.source,
                        error = %error,
                        "failed to resolve dingtalk outbound attachment"
                    );
                    continue;
                }
            };
        debug!(
            chat_id,
            source = resolved.source_label.as_str(),
            kind = ?resolved.kind,
            filename = resolved.filename.as_str(),
            mime_type = resolved.mime_type.as_deref().unwrap_or("unknown"),
            size_bytes = resolved.bytes.len(),
            "resolved dingtalk outbound attachment"
        );

        let result = match resolved.kind {
            OutboundAttachmentKind::Image => {
                debug!(
                    chat_id,
                    source = resolved.source_label.as_str(),
                    filename = resolved.filename.as_str(),
                    size_bytes = resolved.bytes.len(),
                    "uploading dingtalk image attachment"
                );
                let media_id = match client
                    .upload_media(
                        &access_token,
                        &resolved.bytes,
                        "image",
                        &resolved.filename,
                        resolved.mime_type.as_deref(),
                    )
                    .await
                {
                    Ok(media_id) => media_id,
                    Err(error) => {
                        warn!(
                            chat_id,
                            source = resolved.source_label.as_str(),
                            error = %error,
                            "failed to upload dingtalk image attachment"
                        );
                        continue;
                    }
                };
                debug!(
                    chat_id,
                    source = resolved.source_label.as_str(),
                    media_id = media_id.as_str(),
                    "uploaded dingtalk image attachment"
                );
                debug!(
                    chat_id,
                    source = resolved.source_label.as_str(),
                    media_id = media_id.as_str(),
                    "sending dingtalk image attachment message"
                );
                client
                    .send_session_webhook_image_markdown(
                        &session_webhook,
                        &config.bot_title,
                        &media_id,
                        resolved.caption.as_deref(),
                    )
                    .await
            }
            OutboundAttachmentKind::File => match supported_dingtalk_file_type(
                &resolved.filename,
                resolved.mime_type.as_deref(),
            ) {
                Some(file_type) => {
                    debug!(
                        chat_id,
                        source = resolved.source_label.as_str(),
                        filename = resolved.filename.as_str(),
                        size_bytes = resolved.bytes.len(),
                        file_type,
                        "uploading dingtalk file attachment"
                    );
                    let media_id = match client
                        .upload_media(
                            &access_token,
                            &resolved.bytes,
                            "file",
                            &resolved.filename,
                            resolved.mime_type.as_deref(),
                        )
                        .await
                    {
                        Ok(media_id) => media_id,
                        Err(error) => {
                            warn!(
                                chat_id,
                                source = resolved.source_label.as_str(),
                                error = %error,
                                "failed to upload dingtalk file attachment"
                            );
                            continue;
                        }
                    };
                    debug!(
                        chat_id,
                        source = resolved.source_label.as_str(),
                        media_id = media_id.as_str(),
                        "uploaded dingtalk file attachment"
                    );
                    debug!(
                        chat_id,
                        source = resolved.source_label.as_str(),
                        media_id = media_id.as_str(),
                        file_type,
                        "sending dingtalk file attachment message"
                    );
                    client
                        .send_session_webhook_file(
                            &session_webhook,
                            &media_id,
                            file_type,
                            &resolved.filename,
                        )
                        .await
                }
                None => {
                    warn!(
                        chat_id,
                        source = resolved.source_label.as_str(),
                        filename = resolved.filename.as_str(),
                        mime_type = resolved.mime_type.as_deref().unwrap_or("unknown"),
                        "skipping unsupported dingtalk file attachment type"
                    );
                    client
                        .send_session_webhook_markdown(
                            &session_webhook,
                            &config.bot_title,
                            &build_unsupported_file_attachment_markdown(
                                &resolved.filename,
                                resolved.caption.as_deref(),
                            ),
                        )
                        .await
                }
            },
        };

        if let Err(error) = result {
            warn!(
                chat_id,
                source = resolved.source_label.as_str(),
                error = %error,
                "failed to send dingtalk outbound attachment"
            );
        } else {
            debug!(
                chat_id,
                source = resolved.source_label.as_str(),
                "sent dingtalk outbound attachment"
            );
        }
    }
}

pub(super) fn infer_dingtalk_file_type(filename: &str, mime_type: Option<&str>) -> String {
    let extension = Path::new(filename.trim())
        .extension()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_start_matches('.').to_ascii_lowercase());
    if let Some(extension) = extension {
        return extension;
    }

    let mime_subtype = mime_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.split('/').nth(1))
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .rsplit('+')
                .next()
                .unwrap_or(value)
                .to_ascii_lowercase()
        });
    mime_subtype.unwrap_or_else(|| "bin".to_string())
}

pub(super) fn supported_dingtalk_file_type(
    filename: &str,
    mime_type: Option<&str>,
) -> Option<&'static str> {
    match infer_dingtalk_file_type(filename, mime_type).as_str() {
        "pdf" => Some("pdf"),
        "doc" => Some("doc"),
        "docx" => Some("docx"),
        "xlsx" => Some("xlsx"),
        "zip" => Some("zip"),
        "rar" => Some("rar"),
        _ => None,
    }
}

pub(super) fn build_unsupported_file_attachment_markdown(
    filename: &str,
    caption: Option<&str>,
) -> String {
    let mut lines = Vec::new();
    if let Some(caption) = caption.map(str::trim).filter(|value| !value.is_empty()) {
        lines.push(caption.to_string());
    }
    lines.push(format!(
        "钉钉当前仅支持发送 `pdf/doc/docx/xlsx/zip/rar` 文件，`{}` 无法作为原生文件消息发送。",
        filename.trim()
    ));
    lines.join("\n\n")
}
