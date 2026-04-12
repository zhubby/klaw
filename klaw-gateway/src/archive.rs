use axum::{
    Json,
    body::Bytes,
    extract::{Multipart, Path, Query, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use klaw_archive::{
    ArchiveIngestInput, ArchiveMediaKind, ArchiveQuery, ArchiveRecord, ArchiveSourceKind,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info};

use crate::state::GatewayState;

#[derive(Debug, Serialize)]
pub struct ArchiveUploadResponse {
    pub success: bool,
    pub record: Option<ArchiveRecord>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ArchiveListQuery {
    pub session_key: Option<String>,
    pub chat_id: Option<String>,
    pub source_kind: Option<String>,
    pub media_kind: Option<String>,
    pub filename: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Serialize)]
pub struct ArchiveListResponse {
    pub success: bool,
    pub records: Vec<ArchiveRecord>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ArchiveGetResponse {
    pub success: bool,
    pub record: Option<ArchiveRecord>,
    pub error: Option<String>,
}

pub async fn archive_upload_handler(
    State(state): State<Arc<GatewayState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let Some(ref archive_state) = state.archive else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ArchiveUploadResponse {
                success: false,
                record: None,
                error: Some("archive service not available".to_string()),
            }),
        );
    };

    let mut filename: Option<String> = None;
    let mut bytes: Option<Bytes> = None;
    let mut session_key: Option<String> = None;
    let mut channel: Option<String> = None;
    let mut chat_id: Option<String> = None;
    let mut message_id: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "file" => {
                filename = field.file_name().map(|s| s.to_string());
                match field.bytes().await {
                    Ok(data) => bytes = Some(data),
                    Err(err) => {
                        error!("failed to read file field: {err}");
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(ArchiveUploadResponse {
                                success: false,
                                record: None,
                                error: Some(format!("failed to read file: {err}")),
                            }),
                        );
                    }
                }
            }
            "session_key" => {
                if let Ok(data) = field.text().await {
                    session_key = Some(data);
                }
            }
            "channel" => {
                if let Ok(data) = field.text().await {
                    channel = Some(data);
                }
            }
            "chat_id" => {
                if let Ok(data) = field.text().await {
                    chat_id = Some(data);
                }
            }
            "message_id" => {
                if let Ok(data) = field.text().await {
                    message_id = Some(data);
                }
            }
            _ => {}
        }
    }

    let Some(file_bytes) = bytes else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ArchiveUploadResponse {
                success: false,
                record: None,
                error: Some("missing file field".to_string()),
            }),
        );
    };

    let input = ArchiveIngestInput {
        source_kind: ArchiveSourceKind::UserUpload,
        filename,
        declared_mime_type: None,
        session_key,
        channel,
        chat_id,
        message_id,
        metadata: serde_json::json!({}),
    };

    match archive_state.service.ingest_bytes(input, &file_bytes).await {
        Ok(record) => {
            info!(
                archive_id = %record.id,
                size_bytes = record.size_bytes,
                media_kind = ?record.media_kind,
                "file uploaded successfully"
            );
            (
                StatusCode::OK,
                Json(ArchiveUploadResponse {
                    success: true,
                    record: Some(record),
                    error: None,
                }),
            )
        }
        Err(err) => {
            error!("failed to ingest file: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ArchiveUploadResponse {
                    success: false,
                    record: None,
                    error: Some(err.to_string()),
                }),
            )
        }
    }
}

pub async fn archive_download_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(ref archive_state) = state.archive else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            [
                (header::CONTENT_TYPE, "application/json".to_string()),
                (header::CONTENT_DISPOSITION, String::new()),
            ],
            br#"{"error": "archive service not available"}"#.to_vec(),
        );
    };

    match archive_state.service.open_download(&id).await {
        Ok(blob) => {
            let content_type = blob
                .record
                .mime_type
                .clone()
                .unwrap_or_else(|| "application/octet-stream".to_string());
            let filename = blob
                .record
                .original_filename
                .clone()
                .unwrap_or_else(|| "download".to_string());
            let disposition = format!("attachment; filename=\"{filename}\"");

            info!(
                archive_id = %id,
                size_bytes = blob.bytes.len(),
                content_type = %content_type,
                "file downloaded"
            );

            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, content_type),
                    (header::CONTENT_DISPOSITION, disposition),
                ],
                blob.bytes,
            )
        }
        Err(err) => {
            error!("failed to download file {id}: {err}");
            (
                StatusCode::NOT_FOUND,
                [
                    (header::CONTENT_TYPE, "application/json".to_string()),
                    (header::CONTENT_DISPOSITION, String::new()),
                ],
                format!(r#"{{"error": "{}"}}"#, err).into_bytes(),
            )
        }
    }
}

pub async fn archive_list_handler(
    State(state): State<Arc<GatewayState>>,
    Query(params): Query<ArchiveListQuery>,
) -> impl IntoResponse {
    let Some(ref archive_state) = state.archive else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ArchiveListResponse {
                success: false,
                records: vec![],
                error: Some("archive service not available".to_string()),
            }),
        );
    };

    let source_kind = params
        .source_kind
        .as_deref()
        .and_then(ArchiveSourceKind::parse);
    let media_kind = params
        .media_kind
        .as_deref()
        .and_then(ArchiveMediaKind::parse);

    let query = ArchiveQuery {
        session_key: params.session_key,
        chat_id: params.chat_id,
        source_kind,
        media_kind,
        filename: params.filename,
        limit: params.limit,
        offset: params.offset,
    };

    match archive_state.service.find(query).await {
        Ok(records) => {
            info!(count = records.len(), "archive list retrieved");
            (
                StatusCode::OK,
                Json(ArchiveListResponse {
                    success: true,
                    records,
                    error: None,
                }),
            )
        }
        Err(err) => {
            error!("failed to list archives: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ArchiveListResponse {
                    success: false,
                    records: vec![],
                    error: Some(err.to_string()),
                }),
            )
        }
    }
}

pub async fn archive_get_handler(
    State(state): State<Arc<GatewayState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(ref archive_state) = state.archive else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ArchiveGetResponse {
                success: false,
                record: None,
                error: Some("archive service not available".to_string()),
            }),
        );
    };

    match archive_state.service.get(&id).await {
        Ok(record) => {
            info!(archive_id = %id, "archive record retrieved");
            (
                StatusCode::OK,
                Json(ArchiveGetResponse {
                    success: true,
                    record: Some(record),
                    error: None,
                }),
            )
        }
        Err(err) => {
            error!("failed to get archive {id}: {err}");
            (
                StatusCode::NOT_FOUND,
                Json(ArchiveGetResponse {
                    success: false,
                    record: None,
                    error: Some(err.to_string()),
                }),
            )
        }
    }
}
