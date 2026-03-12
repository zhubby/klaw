use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub fn relative_or_absolute_jsonl(root_dir: &Path, file_path: &Path) -> String {
    if let Ok(relative) = file_path.strip_prefix(root_dir) {
        return relative.to_string_lossy().to_string();
    }
    file_path.to_string_lossy().to_string()
}

pub fn now_ms() -> i64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch");
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}
