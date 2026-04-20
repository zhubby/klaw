use async_trait::async_trait;
use klaw_config::AppConfig;
use klaw_storage::{
    DefaultSessionStore, NewPendingQuestionRecord, PendingQuestionRecord, PendingQuestionStatus,
    SessionStorage,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput, ToolSignal};

const DEFAULT_EXPIRES_IN_MINUTES: i64 = 60;
const MAX_EXPIRES_IN_MINUTES: i64 = 7 * 24 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AskQuestionOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AskQuestionRecord {
    pub id: String,
    pub session_key: String,
    pub channel: String,
    pub chat_id: String,
    pub title: Option<String>,
    pub question_text: String,
    pub options: Vec<AskQuestionOption>,
    pub status: PendingQuestionStatus,
    pub selected_option_id: Option<String>,
    pub answered_by: Option<String>,
    pub expires_at_ms: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub answered_at_ms: Option<i64>,
}

impl AskQuestionRecord {
    #[must_use]
    pub fn selected_option(&self) -> Option<&AskQuestionOption> {
        let selected_id = self.selected_option_id.as_deref()?;
        self.options.iter().find(|option| option.id == selected_id)
    }
}

#[derive(Debug, Clone)]
pub struct AskQuestionAnswerOutcome {
    pub question: AskQuestionRecord,
    pub updated: bool,
}

#[derive(Clone)]
pub struct SqliteAskQuestionManager {
    store: DefaultSessionStore,
}

impl SqliteAskQuestionManager {
    #[must_use]
    pub fn from_store(store: DefaultSessionStore) -> Self {
        Self { store }
    }

    pub async fn create_question(
        &self,
        session_key: &str,
        title: Option<String>,
        question_text: String,
        options: Vec<AskQuestionOption>,
        expires_in_minutes: Option<i64>,
    ) -> Result<AskQuestionRecord, ToolError> {
        let session =
            self.store.get_session(session_key).await.map_err(|err| {
                ToolError::ExecutionFailed(format!("failed to load session: {err}"))
            })?;
        let question_text = normalize_non_empty(&question_text, "question")?;
        let title = normalize_optional(title);
        let options = normalize_options(options)?;
        let expires_in_minutes = expires_in_minutes.unwrap_or(DEFAULT_EXPIRES_IN_MINUTES);
        if !(1..=MAX_EXPIRES_IN_MINUTES).contains(&expires_in_minutes) {
            return Err(ToolError::InvalidArgs(format!(
                "`expires_in_minutes` must be between 1 and {MAX_EXPIRES_IN_MINUTES}"
            )));
        }
        let options_json = serde_json::to_string(&options).map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to encode options: {err}"))
        })?;
        let record = self
            .store
            .create_pending_question(&NewPendingQuestionRecord {
                id: Uuid::new_v4().to_string(),
                session_key: session_key.to_string(),
                channel: session.channel,
                chat_id: session.chat_id,
                title,
                question_text,
                options_json,
                expires_at_ms: now_ms() + expires_in_minutes * 60_000,
            })
            .await
            .map_err(|err| {
                ToolError::ExecutionFailed(format!("failed to create pending question: {err}"))
            })?;
        record_from_storage(record)
    }

    pub async fn get_question(&self, question_id: &str) -> Result<AskQuestionRecord, ToolError> {
        let question_id = normalize_non_empty(question_id, "question_id")?;
        let record = self
            .store
            .get_pending_question(&question_id)
            .await
            .map_err(|err| ToolError::ExecutionFailed(format!("failed to load question: {err}")))?;
        record_from_storage(record)
    }

    pub async fn answer_question(
        &self,
        question_id: &str,
        option_id: &str,
        answered_by: Option<&str>,
        answered_at_ms: i64,
    ) -> Result<AskQuestionAnswerOutcome, ToolError> {
        let question_id = normalize_non_empty(question_id, "question_id")?;
        let option_id = normalize_non_empty(option_id, "option_id")?;
        let existing = self.get_question(&question_id).await?;
        if existing.status != PendingQuestionStatus::Pending {
            return Ok(AskQuestionAnswerOutcome {
                question: existing,
                updated: false,
            });
        }
        if existing.expires_at_ms < answered_at_ms {
            let expired = self
                .store
                .update_pending_question_answer(
                    &question_id,
                    PendingQuestionStatus::Expired,
                    None,
                    None,
                    None,
                )
                .await
                .map_err(|err| {
                    ToolError::ExecutionFailed(format!("failed to expire question: {err}"))
                })?;
            return Ok(AskQuestionAnswerOutcome {
                question: record_from_storage(expired)?,
                updated: false,
            });
        }
        if !existing.options.iter().any(|option| option.id == option_id) {
            return Err(ToolError::InvalidArgs(format!(
                "invalid option_id `{option_id}` for question `{question_id}`"
            )));
        }
        let updated = self
            .store
            .update_pending_question_answer(
                &question_id,
                PendingQuestionStatus::Answered,
                Some(&option_id),
                answered_by,
                Some(answered_at_ms),
            )
            .await
            .map_err(|err| {
                ToolError::ExecutionFailed(format!("failed to update question answer: {err}"))
            })?;
        Ok(AskQuestionAnswerOutcome {
            question: record_from_storage(updated)?,
            updated: true,
        })
    }
}

pub struct AskQuestionTool {
    manager: SqliteAskQuestionManager,
}

impl AskQuestionTool {
    #[must_use]
    pub fn with_store(_config: &AppConfig, store: DefaultSessionStore) -> Self {
        Self {
            manager: SqliteAskQuestionManager::from_store(store),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AskQuestionRequest {
    #[serde(default)]
    title: Option<String>,
    question: String,
    options: Vec<AskQuestionOption>,
    #[serde(default)]
    expires_in_minutes: Option<i64>,
}

#[derive(Debug, Serialize)]
struct AskQuestionToolResponse {
    question_id: String,
    status: &'static str,
    title: Option<String>,
    question: String,
    options: Vec<AskQuestionOption>,
    expires_at_ms: i64,
}

#[async_trait]
impl Tool for AskQuestionTool {
    fn name(&self) -> &str {
        "ask_question"
    }

    fn description(&self) -> &str {
        "Use this tool when you need the user to choose one option before you can continue. Best for clarifying ambiguous instructions, gathering a concrete preference, or getting a decision between implementation directions while you work. Only use it for single-select questions with predefined options; do not use it for open-ended questions or free-form text input. If you recommend an option, put it first and append `(Recommended)` to its label."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Ask the user a single-select question when work is blocked on a decision among predefined options. Good for requirement clarification, implementation trade-offs, or explicit preference selection. Do not use this for open-ended questions or anything that needs free-form text.",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Optional short card title shown above the question."
                },
                "question": {
                    "type": "string",
                    "description": "A concise question that asks the user to choose exactly one option."
                },
                "options": {
                    "type": "array",
                    "description": "Selectable options for a single choice. Prefer 2-5 short, distinct options. If one option is recommended, place it first and append `(Recommended)` to its label.",
                    "minItems": 2,
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Stable machine-friendly option ID used in the structured answer, for example `openai`."
                            },
                            "label": {
                                "type": "string",
                                "description": "Short user-visible option label shown on the card."
                            }
                        },
                        "required": ["id", "label"],
                        "additionalProperties": false
                    }
                },
                "expires_in_minutes": {
                    "type": "integer",
                    "description": "Optional TTL for the pending question.",
                    "minimum": 1,
                    "maximum": 10080,
                    "default": 60
                }
            },
            "required": ["question", "options"],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Messaging
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let request: AskQuestionRequest = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("invalid request: {err}")))?;
        let created = self
            .manager
            .create_question(
                &ctx.session_key,
                request.title,
                request.question,
                request.options,
                request.expires_in_minutes,
            )
            .await?;
        let response = AskQuestionToolResponse {
            question_id: created.id.clone(),
            status: "pending",
            title: created.title.clone(),
            question: created.question_text.clone(),
            options: created.options.clone(),
            expires_at_ms: created.expires_at_ms,
        };
        let content_for_model = serde_json::to_string_pretty(&response).map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to serialize output: {err}"))
        })?;
        let actions = created
            .options
            .iter()
            .map(|option| {
                json!({
                    "kind": "submit_command",
                    "label": option.label,
                    "command": format!("/card_answer {} {}", created.id, option.id),
                })
            })
            .collect::<Vec<_>>();
        Ok(ToolOutput {
            content_for_model,
            content_for_user: None,
            media: Vec::new(),
            signals: vec![
                ToolSignal::im_card(json!({
                    "kind": "question_single_select",
                    "title": created.title,
                    "body": created.question_text,
                    "actions": actions,
                    "metadata": {
                        "question_id": created.id,
                    }
                })),
                ToolSignal::stop_current_turn(
                    Some("awaiting ask_question user selection"),
                    Some("ask_question"),
                ),
            ],
        })
    }
}

fn now_ms() -> i64 {
    (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

fn normalize_non_empty(value: &str, field: &str) -> Result<String, ToolError> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(ToolError::InvalidArgs(format!("`{field}` cannot be empty")));
    }
    Ok(normalized.to_string())
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|item| {
        let trimmed = item.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn normalize_options(options: Vec<AskQuestionOption>) -> Result<Vec<AskQuestionOption>, ToolError> {
    if options.len() < 2 {
        return Err(ToolError::InvalidArgs(
            "`options` must contain at least 2 entries".to_string(),
        ));
    }
    let mut normalized = Vec::with_capacity(options.len());
    for option in options {
        let id = normalize_non_empty(&option.id, "options[].id")?;
        if id.contains(char::is_whitespace) {
            return Err(ToolError::InvalidArgs(format!(
                "`options[].id` must not contain whitespace: `{id}`"
            )));
        }
        if normalized
            .iter()
            .any(|existing: &AskQuestionOption| existing.id == id)
        {
            return Err(ToolError::InvalidArgs(format!(
                "duplicate option id: `{id}`"
            )));
        }
        normalized.push(AskQuestionOption {
            id,
            label: normalize_non_empty(&option.label, "options[].label")?,
        });
    }
    Ok(normalized)
}

fn record_from_storage(record: PendingQuestionRecord) -> Result<AskQuestionRecord, ToolError> {
    let options =
        serde_json::from_str::<Vec<AskQuestionOption>>(&record.options_json).map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to decode stored options: {err}"))
        })?;
    Ok(AskQuestionRecord {
        id: record.id,
        session_key: record.session_key,
        channel: record.channel,
        chat_id: record.chat_id,
        title: record.title,
        question_text: record.question_text,
        options,
        status: record.status,
        selected_option_id: record.selected_option_id,
        answered_by: record.answered_by,
        expires_at_ms: record.expires_at_ms,
        created_at_ms: record.created_at_ms,
        updated_at_ms: record.updated_at_ms,
        answered_at_ms: record.answered_at_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_storage::{SessionStorage, StoragePaths};
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn create_store() -> DefaultSessionStore {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("klaw-ask-question-test-{}-{suffix}", now_ms()));
        DefaultSessionStore::open(StoragePaths::from_root(root))
            .await
            .expect("store should open")
    }

    async fn create_manager() -> SqliteAskQuestionManager {
        let store = create_store().await;
        store
            .touch_session("telegram:test", "chat-1", "telegram")
            .await
            .expect("session should exist");
        SqliteAskQuestionManager::from_store(store)
    }

    #[tokio::test]
    async fn create_question_persists_pending_record() {
        let manager = create_manager().await;
        let question = manager
            .create_question(
                "telegram:test",
                Some("Choose provider".to_string()),
                "Which provider should I use?".to_string(),
                vec![
                    AskQuestionOption {
                        id: "openai".to_string(),
                        label: "OpenAI".to_string(),
                    },
                    AskQuestionOption {
                        id: "anthropic".to_string(),
                        label: "Anthropic".to_string(),
                    },
                ],
                Some(5),
            )
            .await
            .expect("question should be created");
        assert_eq!(question.channel, "telegram");
        assert_eq!(question.chat_id, "chat-1");
        assert_eq!(question.status, PendingQuestionStatus::Pending);
        assert_eq!(question.options.len(), 2);
    }

    #[tokio::test]
    async fn answer_question_marks_record_answered() {
        let manager = create_manager().await;
        let question = manager
            .create_question(
                "telegram:test",
                None,
                "Pick one".to_string(),
                vec![
                    AskQuestionOption {
                        id: "a".to_string(),
                        label: "A".to_string(),
                    },
                    AskQuestionOption {
                        id: "b".to_string(),
                        label: "B".to_string(),
                    },
                ],
                Some(5),
            )
            .await
            .expect("question should be created");
        let outcome = manager
            .answer_question(&question.id, "b", Some("channel-user"), now_ms())
            .await
            .expect("question should be answered");
        assert!(outcome.updated);
        assert_eq!(outcome.question.status, PendingQuestionStatus::Answered);
        assert_eq!(outcome.question.selected_option_id.as_deref(), Some("b"));
        assert_eq!(
            outcome
                .question
                .selected_option()
                .map(|option| option.label.as_str()),
            Some("B")
        );
    }

    #[tokio::test]
    async fn answer_question_rejects_unknown_option() {
        let manager = create_manager().await;
        let question = manager
            .create_question(
                "telegram:test",
                None,
                "Pick one".to_string(),
                vec![
                    AskQuestionOption {
                        id: "a".to_string(),
                        label: "A".to_string(),
                    },
                    AskQuestionOption {
                        id: "b".to_string(),
                        label: "B".to_string(),
                    },
                ],
                Some(5),
            )
            .await
            .expect("question should be created");
        let err = manager
            .answer_question(&question.id, "c", Some("channel-user"), now_ms())
            .await
            .expect_err("invalid option should fail");
        assert!(err.to_string().contains("invalid option_id"));
    }

    #[test]
    fn request_rejects_allow_multiple_field() {
        let err = serde_json::from_value::<AskQuestionRequest>(json!({
            "question": "Pick one",
            "options": [
                { "id": "a", "label": "A" },
                { "id": "b", "label": "B" }
            ],
            "allow_multiple": true
        }))
        .expect_err("allow_multiple should be rejected as an unknown field");

        assert!(err.to_string().contains("unknown field `allow_multiple`"));
    }
}
