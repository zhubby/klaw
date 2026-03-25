use chrono::{Local, TimeZone};

pub fn format_timestamp_millis(timestamp_ms: i64) -> String {
    Local
        .timestamp_millis_opt(timestamp_ms)
        .single()
        .map(format_local_datetime)
        .unwrap_or_else(|| timestamp_ms.to_string())
}

pub fn format_optional_timestamp_millis(timestamp_ms: Option<i64>) -> String {
    timestamp_ms
        .map(format_timestamp_millis)
        .unwrap_or_default()
}

pub fn format_timestamp_seconds(timestamp_secs: u64) -> String {
    i64::try_from(timestamp_secs)
        .ok()
        .and_then(|secs| Local.timestamp_opt(secs, 0).single())
        .map(format_local_datetime)
        .unwrap_or_else(|| timestamp_secs.to_string())
}

fn format_local_datetime(dt: chrono::DateTime<Local>) -> String {
    dt.format("%Y/%m/%d %H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_millis_in_local_timezone() {
        let timestamp_ms = 1_735_689_600_000_i64;
        let expected = Local
            .timestamp_millis_opt(timestamp_ms)
            .single()
            .expect("test timestamp should be valid")
            .format("%Y/%m/%d %H:%M:%S")
            .to_string();
        assert_eq!(format_timestamp_millis(timestamp_ms), expected);
    }

    #[test]
    fn formats_seconds_in_local_timezone() {
        let timestamp_secs = 1_735_689_600_u64;
        let timestamp_secs_i64 =
            i64::try_from(timestamp_secs).expect("test timestamp should fit in i64");
        let expected = Local
            .timestamp_opt(timestamp_secs_i64, 0)
            .single()
            .expect("test timestamp should be valid")
            .format("%Y/%m/%d %H:%M:%S")
            .to_string();
        assert_eq!(format_timestamp_seconds(timestamp_secs), expected);
    }
}
