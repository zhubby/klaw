use crate::{LlmMessage, LlmUsage, ToolCall, ToolDefinition};
use klaw_util::{default_data_dir, tokenizer_dir};
use serde_json::json;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};
use tokenizers::Tokenizer;

static TOKENIZER_CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<Tokenizer>>>> = OnceLock::new();

pub fn estimate_chat_usage(
    provider: &str,
    model: &str,
    wire_api: &str,
    tokenizer_path: Option<&str>,
    messages: &[LlmMessage],
    tools: &[ToolDefinition],
    response_content: &str,
    reasoning: Option<&str>,
    tool_calls: &[ToolCall],
) -> LlmUsage {
    let request_payload = json!({
        "wire_api": wire_api,
        "messages": messages,
        "tools": tools,
    });
    let response_payload = json!({
        "content": response_content,
        "reasoning": reasoning,
        "tool_calls": tool_calls,
    });
    let request_text = serde_json::to_string(&request_payload).unwrap_or_default();
    let response_text = serde_json::to_string(&response_payload).unwrap_or_default();
    let tokenizer = resolve_tokenizer(provider, model, tokenizer_path);
    let input_tokens = count_tokens(tokenizer.as_deref(), &request_text);
    let output_tokens = count_tokens(tokenizer.as_deref(), &response_text);
    LlmUsage {
        input_tokens,
        output_tokens,
        total_tokens: input_tokens.saturating_add(output_tokens),
        cached_input_tokens: None,
        reasoning_tokens: None,
        provider_request_id: None,
        provider_response_id: None,
    }
}

fn resolve_tokenizer(
    provider: &str,
    model: &str,
    tokenizer_path: Option<&str>,
) -> Option<Arc<Tokenizer>> {
    let explicit = tokenizer_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let candidate = explicit.or_else(|| {
        default_tokenizer_candidates(provider, model)
            .into_iter()
            .find(|path| path.exists())
    })?;
    let cache = TOKENIZER_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(existing) = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&candidate)
        .cloned()
    {
        return Some(existing);
    }

    let tokenizer = Tokenizer::from_file(&candidate).ok().map(Arc::new)?;
    cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(candidate, Arc::clone(&tokenizer));
    Some(tokenizer)
}

fn default_tokenizer_candidates(provider: &str, model: &str) -> Vec<PathBuf> {
    let Some(root) = default_tokenizer_dir() else {
        return Vec::new();
    };
    let provider_part = sanitize_path_segment(provider);
    let model_part = sanitize_path_segment(model);
    vec![
        root.join(&provider_part).join(format!("{model_part}.json")),
        root.join(format!("{model_part}.json")),
    ]
}

fn default_tokenizer_dir() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("KLAW_TOKENIZER_DIR").filter(|value| !value.is_empty())
    {
        return Some(PathBuf::from(explicit));
    }
    default_data_dir().map(tokenizer_dir)
}

fn sanitize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn count_tokens(tokenizer: Option<&Tokenizer>, text: &str) -> u64 {
    if text.trim().is_empty() {
        return 0;
    }
    if let Some(tokenizer) = tokenizer {
        if let Ok(encoding) = tokenizer.encode(text, false) {
            return encoding.len() as u64;
        }
    }
    heuristic_token_count(text)
}

fn heuristic_token_count(text: &str) -> u64 {
    if text.trim().is_empty() {
        return 0;
    }
    let bytes = text.len() as u64;
    let words = text.split_whitespace().count() as u64;
    let punctuation = text.chars().filter(|ch| ch.is_ascii_punctuation()).count() as u64;
    let estimate = bytes
        .saturating_add(3)
        .saturating_div(4)
        .max(words)
        .saturating_add(punctuation.saturating_div(4));
    estimate.max(1)
}

#[cfg(test)]
mod tests {
    use super::heuristic_token_count;

    #[test]
    fn heuristic_token_count_is_non_zero_for_non_empty_text() {
        assert_eq!(heuristic_token_count(""), 0);
        assert!(heuristic_token_count("hello world") >= 2);
    }
}
