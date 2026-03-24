use crate::{ArchiveError, sniff::SniffedMedia};
use hex::encode as hex_encode;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use tokio::{
    fs::{self, File},
    io::{AsyncReadExt, AsyncWriteExt},
};
use uuid::Uuid;

pub const SNIFF_HEADER_BYTES: usize = 64;
const COPY_BUFFER_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone)]
pub struct FileFingerprint {
    pub content_sha256: String,
    pub size_bytes: i64,
    pub header: Vec<u8>,
}

pub async fn fingerprint_path(path: &Path) -> Result<FileFingerprint, ArchiveError> {
    let mut file = File::open(path)
        .await
        .map_err(|err| ArchiveError::read_file(path, err))?;
    let mut hasher = Sha256::new();
    let mut size_bytes = 0_i64;
    let mut header = Vec::new();
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];

    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|err| ArchiveError::read_file(path, err))?;
        if read == 0 {
            break;
        }
        if header.len() < SNIFF_HEADER_BYTES {
            let remaining = SNIFF_HEADER_BYTES - header.len();
            header.extend_from_slice(&buffer[..read.min(remaining)]);
        }
        hasher.update(&buffer[..read]);
        size_bytes += read as i64;
    }

    Ok(FileFingerprint {
        content_sha256: hex_encode(hasher.finalize()),
        size_bytes,
        header,
    })
}

pub fn fingerprint_bytes(bytes: &[u8]) -> FileFingerprint {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let header = bytes.iter().take(SNIFF_HEADER_BYTES).copied().collect();
    FileFingerprint {
        content_sha256: hex_encode(hasher.finalize()),
        size_bytes: bytes.len() as i64,
        header,
    }
}

pub async fn write_archived_copy(
    archives_dir: &Path,
    source_path: &Path,
    sniffed: &SniffedMedia,
) -> Result<String, ArchiveError> {
    let (target_dir, rel_prefix) = ensure_archive_day_dir(archives_dir).await?;
    let file_id = Uuid::new_v4().to_string();
    let file_name = archive_file_name(&file_id, sniffed.extension.as_deref());
    let temp_path = target_dir.join(format!("{file_name}.tmp"));
    let final_path = target_dir.join(&file_name);
    let rel_path = format!("{rel_prefix}/{file_name}");

    let mut source = File::open(source_path)
        .await
        .map_err(|err| ArchiveError::read_file(source_path, err))?;
    let mut dest = File::create(&temp_path)
        .await
        .map_err(|err| ArchiveError::write_file(&temp_path, err))?;
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];

    loop {
        let read = source
            .read(&mut buffer)
            .await
            .map_err(|err| ArchiveError::read_file(source_path, err))?;
        if read == 0 {
            break;
        }
        dest.write_all(&buffer[..read])
            .await
            .map_err(|err| ArchiveError::write_file(&temp_path, err))?;
    }
    dest.flush()
        .await
        .map_err(|err| ArchiveError::write_file(&temp_path, err))?;
    drop(dest);
    fs::rename(&temp_path, &final_path)
        .await
        .map_err(|err| ArchiveError::rename_file(&temp_path, &final_path, err))?;
    Ok(rel_path)
}

pub async fn write_archived_bytes(
    archives_dir: &Path,
    bytes: &[u8],
    sniffed: &SniffedMedia,
) -> Result<String, ArchiveError> {
    let (target_dir, rel_prefix) = ensure_archive_day_dir(archives_dir).await?;
    let file_id = Uuid::new_v4().to_string();
    let file_name = archive_file_name(&file_id, sniffed.extension.as_deref());
    let temp_path = target_dir.join(format!("{file_name}.tmp"));
    let final_path = target_dir.join(&file_name);
    let rel_path = format!("{rel_prefix}/{file_name}");

    let mut dest = File::create(&temp_path)
        .await
        .map_err(|err| ArchiveError::write_file(&temp_path, err))?;
    dest.write_all(bytes)
        .await
        .map_err(|err| ArchiveError::write_file(&temp_path, err))?;
    dest.flush()
        .await
        .map_err(|err| ArchiveError::write_file(&temp_path, err))?;
    drop(dest);
    fs::rename(&temp_path, &final_path)
        .await
        .map_err(|err| ArchiveError::rename_file(&temp_path, &final_path, err))?;
    Ok(rel_path)
}

pub fn archive_absolute_path(root_dir: &Path, storage_rel_path: &str) -> PathBuf {
    root_dir.join(storage_rel_path)
}

fn archive_file_name(id: &str, extension: Option<&str>) -> String {
    match extension {
        Some(extension) if !extension.is_empty() => format!("{id}.{extension}"),
        _ => id.to_string(),
    }
}

async fn ensure_archive_day_dir(archives_dir: &Path) -> Result<(PathBuf, String), ArchiveError> {
    let date = OffsetDateTime::now_utc().date().to_string();
    let target_dir = archives_dir.join(&date);
    fs::create_dir_all(&target_dir)
        .await
        .map_err(|err| ArchiveError::write_file(&target_dir, err))?;
    Ok((target_dir, format!("archives/{date}")))
}
