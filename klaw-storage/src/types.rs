use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRecord {
    pub ts_ms: i64,
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

impl ChatRecord {
    pub fn new(
        role: impl Into<String>,
        content: impl Into<String>,
        message_id: Option<String>,
    ) -> Self {
        Self {
            ts_ms: crate::util::now_ms(),
            role: role.into(),
            content: content.into(),
            message_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionIndex {
    pub session_key: String,
    pub chat_id: String,
    pub channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_session_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub last_message_at_ms: i64,
    pub turn_count: i64,
    pub jsonl_path: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
    Consumed,
}

impl ApprovalStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
            Self::Consumed => "consumed",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "approved" => Some(Self::Approved),
            "rejected" => Some(Self::Rejected),
            "expired" => Some(Self::Expired),
            "consumed" => Some(Self::Consumed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub id: String,
    pub session_key: String,
    pub tool_name: String,
    pub command_hash: String,
    pub command_preview: String,
    pub command_text: String,
    pub risk_level: String,
    pub status: ApprovalStatus,
    pub requested_by: String,
    pub approved_by: Option<String>,
    pub justification: Option<String>,
    pub expires_at_ms: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub consumed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewApprovalRecord {
    pub id: String,
    pub session_key: String,
    pub tool_name: String,
    pub command_hash: String,
    pub command_preview: String,
    pub command_text: String,
    pub risk_level: String,
    pub requested_by: String,
    pub justification: Option<String>,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CronScheduleKind {
    Cron,
    Every,
}

impl CronScheduleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cron => "cron",
            Self::Every => "every",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "cron" => Some(Self::Cron),
            "every" => Some(Self::Every),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub schedule_kind: CronScheduleKind,
    pub schedule_expr: String,
    pub payload_json: String,
    pub enabled: bool,
    pub timezone: String,
    pub next_run_at_ms: i64,
    pub last_run_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewCronJob {
    pub id: String,
    pub name: String,
    pub schedule_kind: CronScheduleKind,
    pub schedule_expr: String,
    pub payload_json: String,
    pub enabled: bool,
    pub timezone: String,
    pub next_run_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateCronJobPatch {
    pub name: Option<String>,
    pub schedule_kind: Option<CronScheduleKind>,
    pub schedule_expr: Option<String>,
    pub payload_json: Option<String>,
    pub timezone: Option<String>,
    pub next_run_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CronTaskStatus {
    Pending,
    Running,
    Success,
    Failed,
}

impl CronTaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Success => "success",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "success" => Some(Self::Success),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronTaskRun {
    pub id: String,
    pub cron_id: String,
    pub scheduled_at_ms: i64,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
    pub status: CronTaskStatus,
    pub attempt: i64,
    pub error_message: Option<String>,
    pub published_message_id: Option<String>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewCronTaskRun {
    pub id: String,
    pub cron_id: String,
    pub scheduled_at_ms: i64,
    pub status: CronTaskStatus,
    pub attempt: i64,
    pub created_at_ms: i64,
}
