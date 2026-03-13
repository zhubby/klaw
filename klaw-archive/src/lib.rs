mod error;
mod fs_store;
mod model;
mod service;
mod sniff;

pub use error::ArchiveError;
pub use model::{
    ArchiveBlob, ArchiveIngestInput, ArchiveMediaKind, ArchiveQuery, ArchiveRecord, ArchiveService,
    ArchiveSourceKind,
};
pub use service::SqliteArchiveService;

pub async fn open_default_archive_service() -> Result<SqliteArchiveService, ArchiveError> {
    SqliteArchiveService::open_default().await
}
