use crate::ArchiveMediaKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SniffedMedia {
    pub media_kind: ArchiveMediaKind,
    pub mime_type: Option<String>,
    pub extension: Option<String>,
}

pub fn sniff_media(header: &[u8], fallback_extension: Option<&str>) -> SniffedMedia {
    if header.starts_with(b"%PDF-") {
        return sniffed(ArchiveMediaKind::Pdf, "application/pdf", "pdf");
    }
    if header.len() >= 3 && header[..3] == [0xFF, 0xD8, 0xFF] {
        return sniffed(ArchiveMediaKind::Image, "image/jpeg", "jpg");
    }
    if header.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return sniffed(ArchiveMediaKind::Image, "image/png", "png");
    }
    if header.starts_with(b"GIF87a") || header.starts_with(b"GIF89a") {
        return sniffed(ArchiveMediaKind::Image, "image/gif", "gif");
    }
    if header.starts_with(b"BM") {
        return sniffed(ArchiveMediaKind::Image, "image/bmp", "bmp");
    }
    if header.len() >= 12 && &header[..4] == b"RIFF" && &header[8..12] == b"WEBP" {
        return sniffed(ArchiveMediaKind::Image, "image/webp", "webp");
    }
    if header.len() >= 12 && &header[4..8] == b"ftyp" {
        let brand = &header[8..12];
        if brand == b"M4A " || brand == b"M4B " || brand == b"M4P " {
            return sniffed(ArchiveMediaKind::Audio, "audio/mp4", "m4a");
        }
        if brand == b"qt  " {
            return sniffed(ArchiveMediaKind::Video, "video/quicktime", "mov");
        }
        return sniffed(ArchiveMediaKind::Video, "video/mp4", "mp4");
    }
    if header.len() >= 12 && &header[..4] == b"RIFF" && &header[8..12] == b"WAVE" {
        return sniffed(ArchiveMediaKind::Audio, "audio/wav", "wav");
    }
    if header.len() >= 12 && &header[..4] == b"RIFF" && &header[8..12] == b"AVI " {
        return sniffed(ArchiveMediaKind::Video, "video/x-msvideo", "avi");
    }
    if header.starts_with(b"OggS") {
        return sniffed(ArchiveMediaKind::Audio, "audio/ogg", "ogg");
    }
    if header.starts_with(b"ID3") || is_mp3_frame(header) {
        return sniffed(ArchiveMediaKind::Audio, "audio/mpeg", "mp3");
    }
    if header.len() >= 4
        && header[0] == 0xFF
        && (header[1] & 0xF6 == 0xF0 || header[1] & 0xF6 == 0xF2)
    {
        return sniffed(ArchiveMediaKind::Audio, "audio/aac", "aac");
    }
    if header.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        if header.windows(4).any(|window| window == b"webm") {
            return sniffed(ArchiveMediaKind::Video, "video/webm", "webm");
        }
        return sniffed(ArchiveMediaKind::Video, "video/x-matroska", "mkv");
    }

    fallback_from_extension(fallback_extension)
}

fn sniffed(media_kind: ArchiveMediaKind, mime_type: &str, extension: &str) -> SniffedMedia {
    SniffedMedia {
        media_kind,
        mime_type: Some(mime_type.to_string()),
        extension: Some(extension.to_string()),
    }
}

fn fallback_from_extension(extension: Option<&str>) -> SniffedMedia {
    match extension.and_then(normalize_extension) {
        Some(ext) if ext == "pdf" => sniffed(ArchiveMediaKind::Pdf, "application/pdf", "pdf"),
        Some(ext) if ext == "jpg" || ext == "jpeg" => {
            sniffed(ArchiveMediaKind::Image, "image/jpeg", "jpg")
        }
        Some(ext) if ext == "png" => sniffed(ArchiveMediaKind::Image, "image/png", "png"),
        Some(ext) if ext == "gif" => sniffed(ArchiveMediaKind::Image, "image/gif", "gif"),
        Some(ext) if ext == "bmp" => sniffed(ArchiveMediaKind::Image, "image/bmp", "bmp"),
        Some(ext) if ext == "webp" => sniffed(ArchiveMediaKind::Image, "image/webp", "webp"),
        Some(ext) if ext == "mp4" => sniffed(ArchiveMediaKind::Video, "video/mp4", "mp4"),
        Some(ext) if ext == "mov" => sniffed(ArchiveMediaKind::Video, "video/quicktime", "mov"),
        Some(ext) if ext == "avi" => sniffed(ArchiveMediaKind::Video, "video/x-msvideo", "avi"),
        Some(ext) if ext == "mkv" => sniffed(ArchiveMediaKind::Video, "video/x-matroska", "mkv"),
        Some(ext) if ext == "webm" => sniffed(ArchiveMediaKind::Video, "video/webm", "webm"),
        Some(ext) if ext == "mp3" => sniffed(ArchiveMediaKind::Audio, "audio/mpeg", "mp3"),
        Some(ext) if ext == "wav" => sniffed(ArchiveMediaKind::Audio, "audio/wav", "wav"),
        Some(ext) if ext == "ogg" => sniffed(ArchiveMediaKind::Audio, "audio/ogg", "ogg"),
        Some(ext) if ext == "m4a" => sniffed(ArchiveMediaKind::Audio, "audio/mp4", "m4a"),
        Some(ext) if ext == "aac" => sniffed(ArchiveMediaKind::Audio, "audio/aac", "aac"),
        Some(other) => SniffedMedia {
            media_kind: ArchiveMediaKind::Other,
            mime_type: None,
            extension: Some(other),
        },
        None => SniffedMedia {
            media_kind: ArchiveMediaKind::Other,
            mime_type: None,
            extension: None,
        },
    }
}

pub fn normalize_extension(extension: &str) -> Option<String> {
    let trimmed = extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    match trimmed.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        true if !trimmed.is_empty() => Some(trimmed),
        _ => None,
    }
}

fn is_mp3_frame(header: &[u8]) -> bool {
    header.len() >= 2 && header[0] == 0xFF && (header[1] & 0xE0) == 0xE0
}

#[cfg(test)]
mod tests {
    use super::sniff_media;
    use crate::ArchiveMediaKind;

    #[test]
    fn sniffs_pdf() {
        let sniffed = sniff_media(b"%PDF-1.7", None);
        assert_eq!(sniffed.media_kind, ArchiveMediaKind::Pdf);
        assert_eq!(sniffed.extension.as_deref(), Some("pdf"));
    }

    #[test]
    fn sniffs_png() {
        let sniffed = sniff_media(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A], None);
        assert_eq!(sniffed.media_kind, ArchiveMediaKind::Image);
        assert_eq!(sniffed.mime_type.as_deref(), Some("image/png"));
    }

    #[test]
    fn falls_back_to_extension() {
        let sniffed = sniff_media(b"plain bytes", Some("mp3"));
        assert_eq!(sniffed.media_kind, ArchiveMediaKind::Audio);
        assert_eq!(sniffed.extension.as_deref(), Some("mp3"));
    }
}
