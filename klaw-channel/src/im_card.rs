use crate::ChannelResponse;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const IM_CARD_METADATA_KEY: &str = "im.card";
const APPROVAL_ID_METADATA_KEY: &str = "approval.id";
const APPROVAL_SIGNAL_METADATA_KEY: &str = "approval.signal";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImCardKind {
    Approval,
    QuestionSingleSelect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImCardActionKind {
    Approve,
    Reject,
    OpenUrl,
    SubmitCommand,
}

impl ImCardActionKind {
    #[must_use]
    pub fn default_label(self) -> &'static str {
        match self {
            Self::Approve => "Approve",
            Self::Reject => "Reject",
            Self::OpenUrl => "Open",
            Self::SubmitCommand => "Select",
        }
    }

    #[must_use]
    pub fn approval_verb(self) -> Option<&'static str> {
        match self {
            Self::Approve => Some("approve"),
            Self::Reject => Some("reject"),
            Self::OpenUrl | Self::SubmitCommand => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImCardAction {
    pub kind: ImCardActionKind,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
}

impl ImCardAction {
    #[must_use]
    pub fn approval(kind: ImCardActionKind, approval_id: impl Into<String>) -> Self {
        Self {
            kind,
            label: None,
            value: Some(approval_id.into()),
            url: None,
            command: None,
        }
    }

    #[must_use]
    pub fn label_or_default(&self) -> &str {
        self.label
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| self.kind.default_label())
    }

    #[must_use]
    pub fn approval_id(&self) -> Option<&str> {
        match self.kind {
            ImCardActionKind::Approve | ImCardActionKind::Reject => self
                .value
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
            ImCardActionKind::OpenUrl | ImCardActionKind::SubmitCommand => None,
        }
    }

    #[must_use]
    pub fn callback_token(&self, separator: char) -> Option<String> {
        match self.kind {
            ImCardActionKind::Approve | ImCardActionKind::Reject => {
                let approval_id = self.approval_id()?;
                let verb = self.kind.approval_verb()?;
                Some(format!("{verb}{separator}{approval_id}"))
            }
            ImCardActionKind::SubmitCommand => self
                .command
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|command| {
                    format!(
                        "command{separator}{}",
                        URL_SAFE_NO_PAD.encode(command.as_bytes())
                    )
                }),
            ImCardActionKind::OpenUrl => None,
        }
    }

    #[must_use]
    pub fn to_runtime_command(&self) -> Option<String> {
        match self.kind {
            ImCardActionKind::Approve | ImCardActionKind::Reject => {
                let approval_id = self.approval_id()?;
                let verb = self.kind.approval_verb()?;
                Some(format!("/{verb} {approval_id}"))
            }
            ImCardActionKind::SubmitCommand => self
                .command
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
            ImCardActionKind::OpenUrl => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImCard {
    pub kind: ImCardKind,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub actions: Vec<ImCardAction>,
    #[serde(default)]
    pub fallback_text: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

impl ImCard {
    #[must_use]
    pub fn title_or<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(fallback)
    }

    #[must_use]
    pub fn body_or<'a>(&'a self, fallback: &'a str) -> &'a str {
        let body = self.body.trim();
        if body.is_empty() { fallback } else { body }
    }

    #[must_use]
    pub fn fallback_text_or<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.fallback_text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(fallback)
    }

    #[must_use]
    pub fn approval_id(&self) -> Option<&str> {
        self.actions
            .iter()
            .find_map(ImCardAction::approval_id)
            .or_else(|| {
                self.metadata
                    .get("approval_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            })
    }

    #[must_use]
    pub fn command_preview(&self) -> Option<&str> {
        self.metadata
            .get("command_preview")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}

#[must_use]
pub fn resolve_im_card(output: &ChannelResponse) -> Option<ImCard> {
    output
        .metadata
        .get(IM_CARD_METADATA_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value::<ImCard>(value).ok())
        .or_else(|| resolve_approval_card(output))
}

#[must_use]
pub fn parse_im_card_action_token(value: &str) -> Option<ImCardAction> {
    let trimmed = value.trim();
    for (prefix, kind) in [
        ("approve:", ImCardActionKind::Approve),
        ("approve_", ImCardActionKind::Approve),
        ("approve-", ImCardActionKind::Approve),
        ("reject:", ImCardActionKind::Reject),
        ("reject_", ImCardActionKind::Reject),
        ("reject-", ImCardActionKind::Reject),
    ] {
        if trimmed.len() <= prefix.len() {
            continue;
        }
        if !trimmed
            .get(..prefix.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
        {
            continue;
        }
        let approval_id = trimmed[prefix.len()..].trim();
        if approval_id.is_empty() {
            continue;
        }
        return Some(ImCardAction::approval(kind, approval_id.to_string()));
    }
    if let Some(encoded) = trimmed
        .strip_prefix("command:")
        .or_else(|| trimmed.strip_prefix("command_"))
        .or_else(|| trimmed.strip_prefix("command-"))
    {
        let decoded = URL_SAFE_NO_PAD.decode(encoded.trim()).ok()?;
        let command = String::from_utf8(decoded).ok()?;
        let command = command.trim();
        if command.is_empty() {
            return None;
        }
        return Some(ImCardAction {
            kind: ImCardActionKind::SubmitCommand,
            label: None,
            value: None,
            url: None,
            command: Some(command.to_string()),
        });
    }
    None
}

fn resolve_approval_card(output: &ChannelResponse) -> Option<ImCard> {
    let approval_id = extract_approval_id(output)?;
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "approval_id".to_string(),
        Value::String(approval_id.clone()),
    );
    if let Some(command_preview) = extract_approval_command_preview(output) {
        metadata.insert(
            "command_preview".to_string(),
            Value::String(command_preview),
        );
    }
    Some(ImCard {
        kind: ImCardKind::Approval,
        title: None,
        body: output.content.trim().to_string(),
        actions: vec![
            ImCardAction::approval(ImCardActionKind::Approve, approval_id.clone()),
            ImCardAction::approval(ImCardActionKind::Reject, approval_id),
        ],
        fallback_text: normalize_optional_string(Some(output.content.as_str())),
        metadata,
    })
}

fn extract_approval_id(output: &ChannelResponse) -> Option<String> {
    if let Some(approval_id) = output
        .metadata
        .get(APPROVAL_ID_METADATA_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(approval_id.to_string());
    }
    if let Some(approval_id) = output
        .metadata
        .get(APPROVAL_SIGNAL_METADATA_KEY)
        .and_then(Value::as_object)
        .and_then(|value| value.get("approval_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(approval_id.to_string());
    }
    extract_shell_approval_id(&output.content)
}

fn extract_approval_command_preview(output: &ChannelResponse) -> Option<String> {
    output
        .metadata
        .get(APPROVAL_SIGNAL_METADATA_KEY)
        .and_then(Value::as_object)
        .and_then(|value| value.get("command_preview"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn extract_shell_approval_id(content: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<Value>(content) {
        if let Some(token) = value
            .pointer("/approval/id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            return Some(token.to_string());
        }
        for key in ["approval_id", "approvalId"] {
            if let Some(token) = value
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
            {
                return Some(token.to_string());
            }
        }
        if let Some(token) = value
            .pointer("/approvalId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            return Some(token.to_string());
        }
    }

    let marker = "approval_id=";
    if let Some(idx) = content.find(marker) {
        let rest = &content[idx + marker.len()..];
        let token = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
            .collect::<String>();
        if !token.is_empty() {
            return Some(token);
        }
    }

    extract_uuid_like_approval_id(content)
}

fn extract_uuid_like_approval_id(content: &str) -> Option<String> {
    let lowered = content.to_ascii_lowercase();
    let hinted = lowered.contains("approval id")
        || lowered.contains("approval_id")
        || content.contains("审批 ID")
        || content.contains("审批id")
        || content.contains("审批单")
        || lowered.contains("批准id")
        || content.contains("批准 ID");
    if !hinted {
        return None;
    }

    content
        .split(|ch: char| ch.is_whitespace() || ",.;:，。；：()[]{}<>\"'`".contains(ch))
        .filter_map(normalize_uuid_token)
        .find(|token| is_uuid_like(token))
}

fn normalize_uuid_token(token: &str) -> Option<String> {
    let trimmed = token.trim_matches(|ch: char| {
        ch.is_whitespace()
            || matches!(
                ch,
                ',' | '.'
                    | ';'
                    | ':'
                    | '，'
                    | '。'
                    | '；'
                    | '：'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '<'
                    | '>'
                    | '"'
                    | '\''
                    | '`'
            )
    });
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed
        .chars()
        .map(|ch| match ch {
            '–' | '—' | '−' => '-',
            _ => ch,
        })
        .collect::<String>();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn is_uuid_like(token: &str) -> bool {
    let segments = token.split('-').collect::<Vec<_>>();
    if segments.len() != 5 {
        return false;
    }
    let expected = [8, 4, 4, 4, 12];
    segments.iter().zip(expected).all(|(segment, len)| {
        segment.len() == len && segment.chars().all(|ch| ch.is_ascii_hexdigit())
    })
}

fn normalize_optional_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_im_card_prefers_explicit_metadata() {
        let output = ChannelResponse {
            content: "approval_id=from-content".to_string(),
            reasoning: None,
            metadata: BTreeMap::from([(
                IM_CARD_METADATA_KEY.to_string(),
                serde_json::json!({
                    "kind": "approval",
                    "title": "Need approval",
                    "body": "Please review",
                    "actions": [
                        {
                            "kind": "approve",
                            "label": "Ship it",
                            "value": "approval-1"
                        },
                        {
                            "kind": "reject",
                            "value": "approval-1"
                        }
                    ],
                    "metadata": {
                        "command_preview": "python3 -c \"print(1)\""
                    }
                }),
            )]),
            attachments: Vec::new(),
        };

        let card = resolve_im_card(&output).expect("card");
        assert_eq!(card.title.as_deref(), Some("Need approval"));
        assert_eq!(card.approval_id(), Some("approval-1"));
        assert_eq!(card.command_preview(), Some("python3 -c \"print(1)\""));
        assert_eq!(card.actions[0].label_or_default(), "Ship it");
    }

    #[test]
    fn resolve_im_card_falls_back_to_approval_signal() {
        let output = ChannelResponse {
            content: "This shell command requires approval.".to_string(),
            reasoning: None,
            metadata: BTreeMap::from([(
                APPROVAL_SIGNAL_METADATA_KEY.to_string(),
                serde_json::json!({
                    "approval_id": "approval-2",
                    "command_preview": "python3 -c \"print(1)\""
                }),
            )]),
            attachments: Vec::new(),
        };

        let card = resolve_im_card(&output).expect("card");
        assert_eq!(card.kind, ImCardKind::Approval);
        assert_eq!(card.approval_id(), Some("approval-2"));
        assert_eq!(card.command_preview(), Some("python3 -c \"print(1)\""));
        assert_eq!(
            card.actions[0].to_runtime_command().as_deref(),
            Some("/approve approval-2")
        );
    }

    #[test]
    fn resolve_im_card_falls_back_to_natural_language_uuid() {
        let output = ChannelResponse {
            content:
                "我已经请求批准来执行浏览器自动化任务。批准ID: 3a24e1d4-9c94-4ee1-ac16-1f750ca78acf"
                    .to_string(),
            reasoning: None,
            metadata: BTreeMap::new(),
            attachments: Vec::new(),
        };

        let card = resolve_im_card(&output).expect("card");
        assert_eq!(
            card.approval_id(),
            Some("3a24e1d4-9c94-4ee1-ac16-1f750ca78acf")
        );
    }

    #[test]
    fn parse_im_card_action_token_supports_multiple_separators() {
        let approve = parse_im_card_action_token("approve:approval-1").expect("approve");
        let reject = parse_im_card_action_token("reject_approval-2").expect("reject");
        let reject_dash = parse_im_card_action_token("reject-approval-3").expect("reject");

        assert_eq!(
            approve.to_runtime_command().as_deref(),
            Some("/approve approval-1")
        );
        assert_eq!(
            approve.callback_token(':').as_deref(),
            Some("approve:approval-1")
        );
        assert_eq!(
            reject.to_runtime_command().as_deref(),
            Some("/reject approval-2")
        );
        assert_eq!(
            reject_dash.to_runtime_command().as_deref(),
            Some("/reject approval-3")
        );
    }

    #[test]
    fn parse_im_card_action_token_supports_encoded_command_payloads() {
        let action = ImCardAction {
            kind: ImCardActionKind::SubmitCommand,
            label: Some("Option A".to_string()),
            value: None,
            url: None,
            command: Some("/card_answer question-1 option-a".to_string()),
        };

        let token = action.callback_token(':').expect("callback token");
        let parsed = parse_im_card_action_token(&token).expect("parsed action");

        assert_eq!(parsed.kind, ImCardActionKind::SubmitCommand);
        assert_eq!(
            parsed.to_runtime_command().as_deref(),
            Some("/card_answer question-1 option-a")
        );
    }

    #[test]
    fn resolve_im_card_preserves_question_single_select_cards() {
        let output = ChannelResponse {
            content: "fallback".to_string(),
            reasoning: None,
            metadata: BTreeMap::from([(
                IM_CARD_METADATA_KEY.to_string(),
                serde_json::json!({
                    "kind": "question_single_select",
                    "title": "Pick one",
                    "body": "Choose the best option",
                    "actions": [
                        {
                            "kind": "submit_command",
                            "label": "A",
                            "command": "/card_answer q1 a"
                        },
                        {
                            "kind": "submit_command",
                            "label": "B",
                            "command": "/card_answer q1 b"
                        }
                    ]
                }),
            )]),
            attachments: Vec::new(),
        };

        let card = resolve_im_card(&output).expect("card");
        assert_eq!(card.kind, ImCardKind::QuestionSingleSelect);
        assert_eq!(card.title.as_deref(), Some("Pick one"));
        assert_eq!(card.actions.len(), 2);
        assert_eq!(
            card.actions[0].to_runtime_command().as_deref(),
            Some("/card_answer q1 a")
        );
    }
}
