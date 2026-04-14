use super::*;

pub(super) async fn usage_response(
    runtime: &RuntimeBundle,
    session_key: &str,
) -> Result<ChannelResponse, Box<dyn Error>> {
    let sessions = session_manager(runtime);
    let session_usage = sessions.sum_llm_usage_by_session(session_key).await?;
    let latest_turn = sessions
        .list_llm_usage(
            session_key,
            klaw_session::SessionListQuery {
                limit: Some(1),
                ..Default::default()
            },
        )
        .await?
        .into_iter()
        .next()
        .map(|record| record.turn_index);

    let mut lines = vec![
        "📊 **Usage**".to_string(),
        String::new(),
        format!("Session: `{session_key}`"),
        String::new(),
    ];

    match latest_turn {
        Some(turn_index) => {
            let turn_usage = sessions
                .sum_llm_usage_by_turn(session_key, turn_index)
                .await?;
            lines.push(format!("**Latest turn:** #{turn_index}"));
            push_usage_summary(&mut lines, &turn_usage);
        }
        None => {
            lines.push("**Latest turn:** no token usage recorded yet".to_string());
        }
    }

    lines.push(String::new());
    lines.push("**Session total:**".to_string());
    push_usage_summary(&mut lines, &session_usage);

    Ok(channel_response(lines.join("\n"), None, BTreeMap::new()))
}

fn push_usage_summary(lines: &mut Vec<String>, usage: &klaw_storage::LlmUsageSummary) {
    lines.push(format!("- requests: `{}`", usage.request_count));
    lines.push(format!("- input_tokens: `{}`", usage.input_tokens));
    lines.push(format!("- output_tokens: `{}`", usage.output_tokens));
    lines.push(format!("- total_tokens: `{}`", usage.total_tokens));
    lines.push(format!(
        "- cached_input_tokens: `{}`",
        usage.cached_input_tokens
    ));
    lines.push(format!("- reasoning_tokens: `{}`", usage.reasoning_tokens));
}
