use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn parse_im_command(input: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    let rest = trimmed.trim_start_matches('/').trim();
    if rest.is_empty() {
        return None;
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let command = parts.next()?.trim();
    let arg = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Some((command, arg))
}

pub(super) fn first_arg_token(arg: Option<&str>) -> Option<&str> {
    arg.and_then(|raw| raw.split_whitespace().next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn second_arg_token(arg: Option<&str>) -> Option<&str> {
    arg.and_then(|raw| raw.split_whitespace().nth(1))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
