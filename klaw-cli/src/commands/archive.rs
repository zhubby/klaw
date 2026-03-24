use clap::{Args, Subcommand, ValueEnum};
use klaw_archive::{
    ArchiveIngestInput, ArchiveMediaKind, ArchiveQuery, ArchiveService, ArchiveSourceKind,
    SqliteArchiveService, open_default_archive_service,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Debug, Args)]
pub struct ArchiveCommand {
    #[command(subcommand)]
    pub command: ArchiveSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum ArchiveSubcommands {
    /// List archived records from archive.db.
    List(ArchiveListCommand),
    /// Show one archive record by ID.
    Get(ArchiveGetCommand),
    /// Push a local file into the archive store.
    Push(ArchivePushCommand),
    /// Pull an archived file back to a local path.
    Pull(ArchivePullCommand),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ArchiveSourceArg {
    UserUpload,
    ChannelInbound,
    ModelGenerated,
}

impl From<ArchiveSourceArg> for ArchiveSourceKind {
    fn from(value: ArchiveSourceArg) -> Self {
        match value {
            ArchiveSourceArg::UserUpload => ArchiveSourceKind::UserUpload,
            ArchiveSourceArg::ChannelInbound => ArchiveSourceKind::ChannelInbound,
            ArchiveSourceArg::ModelGenerated => ArchiveSourceKind::ModelGenerated,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ArchiveMediaArg {
    Pdf,
    Image,
    Video,
    Audio,
    Other,
}

impl From<ArchiveMediaArg> for ArchiveMediaKind {
    fn from(value: ArchiveMediaArg) -> Self {
        match value {
            ArchiveMediaArg::Pdf => ArchiveMediaKind::Pdf,
            ArchiveMediaArg::Image => ArchiveMediaKind::Image,
            ArchiveMediaArg::Video => ArchiveMediaKind::Video,
            ArchiveMediaArg::Audio => ArchiveMediaKind::Audio,
            ArchiveMediaArg::Other => ArchiveMediaKind::Other,
        }
    }
}

#[derive(Debug, Args)]
pub struct ArchiveListCommand {
    /// Optional session key filter.
    #[arg(long)]
    pub session_key: Option<String>,
    /// Optional chat ID filter.
    #[arg(long)]
    pub chat_id: Option<String>,
    /// Optional source kind filter.
    #[arg(long, value_enum)]
    pub source_kind: Option<ArchiveSourceArg>,
    /// Optional media kind filter.
    #[arg(long, value_enum)]
    pub media_kind: Option<ArchiveMediaArg>,
    /// Optional filename filter (fuzzy match).
    #[arg(long)]
    pub filename: Option<String>,
    /// Max rows to return.
    #[arg(long, default_value_t = 20)]
    pub limit: i64,
    /// Row offset for pagination.
    #[arg(long, default_value_t = 0)]
    pub offset: i64,
}

#[derive(Debug, Args)]
pub struct ArchiveGetCommand {
    /// Exact archive record ID.
    #[arg(long)]
    pub archive_id: String,
}

#[derive(Debug, Args)]
pub struct ArchivePushCommand {
    /// Local source file path to archive.
    #[arg(long)]
    pub path: PathBuf,
    /// Logical source kind for the archived file.
    #[arg(long, value_enum, default_value_t = ArchiveSourceArg::UserUpload)]
    pub source_kind: ArchiveSourceArg,
    /// Optional original filename override.
    #[arg(long)]
    pub filename: Option<String>,
    /// Optional declared MIME type.
    #[arg(long)]
    pub mime_type: Option<String>,
    /// Optional session key for indexing.
    #[arg(long)]
    pub session_key: Option<String>,
    /// Optional channel for indexing.
    #[arg(long)]
    pub channel: Option<String>,
    /// Optional chat ID for indexing.
    #[arg(long)]
    pub chat_id: Option<String>,
    /// Optional source message ID.
    #[arg(long)]
    pub message_id: Option<String>,
    /// Optional JSON object metadata string.
    #[arg(long)]
    pub metadata_json: Option<String>,
}

#[derive(Debug, Args)]
pub struct ArchivePullCommand {
    /// Archive record ID to download.
    #[arg(long)]
    pub archive_id: String,
    /// Output file path. If omitted, writes to the current directory using the stored filename.
    #[arg(long)]
    pub output: Option<PathBuf>,
}

impl ArchiveCommand {
    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let service = open_default_archive_service().await?;
        match self.command {
            ArchiveSubcommands::List(cmd) => cmd.run(&service).await?,
            ArchiveSubcommands::Get(cmd) => cmd.run(&service).await?,
            ArchiveSubcommands::Push(cmd) => cmd.run(&service).await?,
            ArchiveSubcommands::Pull(cmd) => cmd.run(&service).await?,
        }
        Ok(())
    }
}

impl ArchiveListCommand {
    async fn run(self, service: &SqliteArchiveService) -> Result<(), Box<dyn std::error::Error>> {
        let records = service
            .find(ArchiveQuery {
                session_key: self.session_key,
                chat_id: self.chat_id,
                source_kind: self.source_kind.map(Into::into),
                media_kind: self.media_kind.map(Into::into),
                filename: self.filename,
                limit: self.limit,
                offset: self.offset,
            })
            .await?;

        if records.is_empty() {
            println!("No archive records.");
            return Ok(());
        }

        for record in records {
            println!(
                "{} source_kind={} media_kind={} size_bytes={} created_at_ms={} storage_rel_path={} original_filename={}",
                record.id,
                record.source_kind.as_str(),
                record.media_kind.as_str(),
                record.size_bytes,
                record.created_at_ms,
                record.storage_rel_path,
                record.original_filename.as_deref().unwrap_or("-"),
            );
        }
        Ok(())
    }
}

impl ArchiveGetCommand {
    async fn run(self, service: &SqliteArchiveService) -> Result<(), Box<dyn std::error::Error>> {
        let record = service.get(&self.archive_id).await?;
        println!("id={}", record.id);
        println!("source_kind={}", record.source_kind.as_str());
        println!("media_kind={}", record.media_kind.as_str());
        println!("mime_type={}", record.mime_type.as_deref().unwrap_or("-"));
        println!("extension={}", record.extension.as_deref().unwrap_or("-"));
        println!(
            "original_filename={}",
            record.original_filename.as_deref().unwrap_or("-")
        );
        println!("content_sha256={}", record.content_sha256);
        println!("size_bytes={}", record.size_bytes);
        println!("storage_rel_path={}", record.storage_rel_path);
        println!(
            "session_key={}",
            record.session_key.as_deref().unwrap_or("-")
        );
        println!("channel={}", record.channel.as_deref().unwrap_or("-"));
        println!("chat_id={}", record.chat_id.as_deref().unwrap_or("-"));
        println!("message_id={}", record.message_id.as_deref().unwrap_or("-"));
        println!("created_at_ms={}", record.created_at_ms);
        println!("metadata_json={}", record.metadata_json);
        Ok(())
    }
}

impl ArchivePushCommand {
    async fn run(self, service: &SqliteArchiveService) -> Result<(), Box<dyn std::error::Error>> {
        let metadata = parse_metadata_json(self.metadata_json.as_deref())?;
        let filename = self.filename.or_else(|| {
            self.path
                .file_name()
                .and_then(|name| name.to_str())
                .map(ToOwned::to_owned)
        });
        let record = service
            .ingest_path(
                ArchiveIngestInput {
                    source_kind: self.source_kind.into(),
                    filename,
                    declared_mime_type: self.mime_type,
                    session_key: self.session_key,
                    channel: self.channel,
                    chat_id: self.chat_id,
                    message_id: self.message_id,
                    metadata,
                },
                &self.path,
            )
            .await?;

        println!("archived_id={}", record.id);
        println!("storage_rel_path={}", record.storage_rel_path);
        println!("media_kind={}", record.media_kind.as_str());
        println!("size_bytes={}", record.size_bytes);
        Ok(())
    }
}

impl ArchivePullCommand {
    async fn run(self, service: &SqliteArchiveService) -> Result<(), Box<dyn std::error::Error>> {
        let blob = service.open_download(&self.archive_id).await?;
        let output_path = resolve_output_path(self.output, &blob.record);
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).await?;
            }
        }
        fs::write(&output_path, &blob.bytes).await?;
        println!("archive_id={}", blob.record.id);
        println!("written_to={}", output_path.display());
        println!("size_bytes={}", blob.bytes.len());
        Ok(())
    }
}

fn parse_metadata_json(raw: Option<&str>) -> Result<Value, Box<dyn std::error::Error>> {
    let Some(raw) = raw else {
        return Ok(Value::Object(Default::default()));
    };
    let value: Value = serde_json::from_str(raw)?;
    if !value.is_object() {
        return Err("metadata_json must be a JSON object".into());
    }
    Ok(value)
}

fn resolve_output_path(provided: Option<PathBuf>, record: &klaw_archive::ArchiveRecord) -> PathBuf {
    if let Some(path) = provided {
        return path;
    }

    let fallback_name = record
        .original_filename
        .clone()
        .or_else(|| {
            record
                .extension
                .as_deref()
                .map(|ext| format!("{}.{}", record.id, ext))
        })
        .unwrap_or_else(|| record.id.clone());
    Path::new(".").join(fallback_name)
}

#[cfg(test)]
mod tests {
    use super::{
        ArchiveMediaArg, ArchivePullCommand, ArchivePushCommand, ArchiveSourceArg,
        parse_metadata_json, resolve_output_path,
    };
    use klaw_archive::{ArchiveMediaKind, ArchiveRecord, ArchiveSourceKind};
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn metadata_parser_defaults_to_empty_object() {
        let value = parse_metadata_json(None).expect("metadata should parse");
        assert_eq!(value, json!({}));
    }

    #[test]
    fn metadata_parser_rejects_non_object() {
        let err = parse_metadata_json(Some("[]")).expect_err("array should fail");
        assert!(err.to_string().contains("metadata_json"));
    }

    #[test]
    fn resolve_output_path_uses_filename_by_default() {
        let record = ArchiveRecord {
            id: "a1".to_string(),
            source_kind: ArchiveSourceKind::UserUpload,
            media_kind: ArchiveMediaKind::Pdf,
            mime_type: Some("application/pdf".to_string()),
            extension: Some("pdf".to_string()),
            original_filename: Some("file.pdf".to_string()),
            content_sha256: "sha".to_string(),
            size_bytes: 1,
            storage_rel_path: "archives/2026-03-13/a1.pdf".to_string(),
            session_key: None,
            channel: None,
            chat_id: None,
            message_id: None,
            metadata_json: "{}".to_string(),
            created_at_ms: 1,
        };
        assert_eq!(
            resolve_output_path(None, &record),
            PathBuf::from("./file.pdf")
        );
    }

    #[test]
    fn clap_value_enum_maps_work() {
        let _ = ArchivePushCommand {
            path: PathBuf::from("a.txt"),
            source_kind: ArchiveSourceArg::UserUpload,
            filename: None,
            mime_type: None,
            session_key: None,
            channel: None,
            chat_id: None,
            message_id: None,
            metadata_json: None,
        };
        let _ = ArchivePullCommand {
            archive_id: "a1".to_string(),
            output: None,
        };
        assert_eq!(
            ArchiveMediaKind::from(ArchiveMediaArg::Audio),
            ArchiveMediaKind::Audio
        );
    }
}
