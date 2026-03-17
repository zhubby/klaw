use time::OffsetDateTime;

pub fn format_timestamp_millis(timestamp_ms: i64) -> String {
    let timestamp_nanos = i128::from(timestamp_ms) * 1_000_000;
    match OffsetDateTime::from_unix_timestamp_nanos(timestamp_nanos) {
        Ok(dt) => format_offset_datetime(dt),
        Err(_) => timestamp_ms.to_string(),
    }
}

pub fn format_optional_timestamp_millis(timestamp_ms: Option<i64>) -> String {
    timestamp_ms
        .map(format_timestamp_millis)
        .unwrap_or_default()
}

pub fn format_timestamp_seconds(timestamp_secs: u64) -> String {
    let timestamp_nanos = i128::from(timestamp_secs) * 1_000_000_000;
    match OffsetDateTime::from_unix_timestamp_nanos(timestamp_nanos) {
        Ok(dt) => format_offset_datetime(dt),
        Err(_) => timestamp_secs.to_string(),
    }
}

fn format_offset_datetime(dt: OffsetDateTime) -> String {
    let month = u8::from(dt.month());
    format!(
        "{:04}/{:02}/{:02} {:02}:{:02}:{:02}",
        dt.year(),
        month,
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second()
    )
}
