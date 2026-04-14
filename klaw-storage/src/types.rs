use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRecord {
    pub ts_ms: i64,
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_json: Option<String>,
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
            metadata_json: None,
            message_id,
        }
    }

    #[must_use]
    pub fn with_metadata_json(mut self, metadata_json: Option<String>) -> Self {
        self.metadata_json = metadata_json;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionIndex {
    pub session_key: String,
    pub chat_id: String,
    pub channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_session_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_provider: Option<String>,
    #[serde(default)]
    pub model_provider_explicit: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub model_explicit: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_metadata_json: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub last_message_at_ms: i64,
    pub turn_count: i64,
    pub jsonl_path: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionSortOrder {
    UpdatedAtAsc,
    #[default]
    UpdatedAtDesc,
}

impl SessionSortOrder {
    #[must_use]
    pub fn sql_order_by(self) -> &'static str {
        match self {
            Self::UpdatedAtAsc => "updated_at_ms ASC, session_key ASC",
            Self::UpdatedAtDesc => "updated_at_ms DESC, session_key DESC",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SessionCompressionState {
    pub last_compressed_len: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_json: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmUsageSource {
    ProviderReported,
    EstimatedLocal,
}

impl LlmUsageSource {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderReported => "provider_reported",
            Self::EstimatedLocal => "estimated_local",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "provider_reported" => Some(Self::ProviderReported),
            "estimated_local" => Some(Self::EstimatedLocal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmUsageRecord {
    pub id: String,
    pub session_key: String,
    pub chat_id: String,
    pub turn_index: i64,
    pub request_seq: i64,
    pub provider: String,
    pub model: String,
    pub wire_api: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub cached_input_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub source: LlmUsageSource,
    pub provider_request_id: Option<String>,
    pub provider_response_id: Option<String>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewLlmUsageRecord {
    pub id: String,
    pub session_key: String,
    pub chat_id: String,
    pub turn_index: i64,
    pub request_seq: i64,
    pub provider: String,
    pub model: String,
    pub wire_api: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub cached_input_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub source: LlmUsageSource,
    pub provider_request_id: Option<String>,
    pub provider_response_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LlmUsageSummary {
    pub request_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub cached_input_tokens: i64,
    pub reasoning_tokens: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmAuditStatus {
    Success,
    Failed,
}

impl LlmAuditStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "success" => Some(Self::Success),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmAuditSortOrder {
    RequestedAtAsc,
    RequestedAtDesc,
}

impl Default for LlmAuditSortOrder {
    fn default() -> Self {
        Self::RequestedAtDesc
    }
}

impl LlmAuditSortOrder {
    #[must_use]
    pub fn sql_order_by(self) -> &'static str {
        match self {
            Self::RequestedAtAsc => "requested_at_ms ASC, created_at_ms ASC",
            Self::RequestedAtDesc => "requested_at_ms DESC, created_at_ms DESC",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAuditRecord {
    pub id: String,
    pub session_key: String,
    pub chat_id: String,
    pub turn_index: i64,
    pub request_seq: i64,
    pub provider: String,
    pub model: String,
    pub wire_api: String,
    pub status: LlmAuditStatus,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub provider_request_id: Option<String>,
    pub provider_response_id: Option<String>,
    pub request_body_json: String,
    pub response_body_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_json: Option<String>,
    pub requested_at_ms: i64,
    pub responded_at_ms: Option<i64>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmAuditSummaryRecord {
    pub id: String,
    pub session_key: String,
    pub chat_id: String,
    pub turn_index: i64,
    pub request_seq: i64,
    pub provider: String,
    pub model: String,
    pub wire_api: String,
    pub status: LlmAuditStatus,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub provider_request_id: Option<String>,
    pub provider_response_id: Option<String>,
    pub requested_at_ms: i64,
    pub responded_at_ms: Option<i64>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewLlmAuditRecord {
    pub id: String,
    pub session_key: String,
    pub chat_id: String,
    pub turn_index: i64,
    pub request_seq: i64,
    pub provider: String,
    pub model: String,
    pub wire_api: String,
    pub status: LlmAuditStatus,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub provider_request_id: Option<String>,
    pub provider_response_id: Option<String>,
    pub request_body_json: String,
    pub response_body_json: Option<String>,
    pub metadata_json: Option<String>,
    pub requested_at_ms: i64,
    pub responded_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LlmAuditQuery {
    pub session_key: Option<String>,
    pub provider: Option<String>,
    pub requested_from_ms: Option<i64>,
    pub requested_to_ms: Option<i64>,
    pub limit: i64,
    pub offset: i64,
    pub sort_order: LlmAuditSortOrder,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LlmAuditFilterOptionsQuery {
    pub requested_from_ms: Option<i64>,
    pub requested_to_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LlmAuditFilterOptions {
    pub session_keys: Vec<String>,
    pub providers: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolAuditStatus {
    Success,
    Failed,
}

impl ToolAuditStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "success" => Some(Self::Success),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolAuditSortOrder {
    StartedAtAsc,
    StartedAtDesc,
}

impl Default for ToolAuditSortOrder {
    fn default() -> Self {
        Self::StartedAtDesc
    }
}

impl ToolAuditSortOrder {
    #[must_use]
    pub fn sql_order_by(self) -> &'static str {
        match self {
            Self::StartedAtAsc => "started_at_ms ASC, created_at_ms ASC",
            Self::StartedAtDesc => "started_at_ms DESC, created_at_ms DESC",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAuditRecord {
    pub id: String,
    pub session_key: String,
    pub chat_id: String,
    pub turn_index: i64,
    pub request_seq: i64,
    pub tool_call_seq: i64,
    pub tool_name: String,
    pub status: ToolAuditStatus,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub retryable: Option<bool>,
    pub approval_required: bool,
    pub arguments_json: String,
    pub result_content: String,
    pub error_details_json: Option<String>,
    pub signals_json: Option<String>,
    pub metadata_json: Option<String>,
    pub started_at_ms: i64,
    pub finished_at_ms: i64,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewToolAuditRecord {
    pub id: String,
    pub session_key: String,
    pub chat_id: String,
    pub turn_index: i64,
    pub request_seq: i64,
    pub tool_call_seq: i64,
    pub tool_name: String,
    pub status: ToolAuditStatus,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub retryable: Option<bool>,
    pub approval_required: bool,
    pub arguments_json: String,
    pub result_content: String,
    pub error_details_json: Option<String>,
    pub signals_json: Option<String>,
    pub metadata_json: Option<String>,
    pub started_at_ms: i64,
    pub finished_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ToolAuditQuery {
    pub session_key: Option<String>,
    pub tool_name: Option<String>,
    pub started_from_ms: Option<i64>,
    pub started_to_ms: Option<i64>,
    pub limit: i64,
    pub offset: i64,
    pub sort_order: ToolAuditSortOrder,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ToolAuditFilterOptionsQuery {
    pub started_from_ms: Option<i64>,
    pub started_to_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ToolAuditFilterOptions {
    pub session_keys: Vec<String>,
    pub tool_names: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WebhookEventStatus {
    Accepted,
    Processed,
    Failed,
}

impl WebhookEventStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Processed => "processed",
            Self::Failed => "failed",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "accepted" => Some(Self::Accepted),
            "processed" => Some(Self::Processed),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WebhookEventSortOrder {
    ReceivedAtAsc,
    ReceivedAtDesc,
}

impl Default for WebhookEventSortOrder {
    fn default() -> Self {
        Self::ReceivedAtDesc
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEventRecord {
    pub id: String,
    pub source: String,
    pub event_type: String,
    pub session_key: String,
    pub chat_id: String,
    pub sender_id: String,
    pub content: String,
    pub payload_json: Option<String>,
    pub metadata_json: Option<String>,
    pub status: WebhookEventStatus,
    pub error_message: Option<String>,
    pub response_summary: Option<String>,
    pub received_at_ms: i64,
    pub processed_at_ms: Option<i64>,
    pub remote_addr: Option<String>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewWebhookEventRecord {
    pub id: String,
    pub source: String,
    pub event_type: String,
    pub session_key: String,
    pub chat_id: String,
    pub sender_id: String,
    pub content: String,
    pub payload_json: Option<String>,
    pub metadata_json: Option<String>,
    pub status: WebhookEventStatus,
    pub error_message: Option<String>,
    pub response_summary: Option<String>,
    pub received_at_ms: i64,
    pub processed_at_ms: Option<i64>,
    pub remote_addr: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateWebhookEventResult {
    pub status: WebhookEventStatus,
    pub error_message: Option<String>,
    pub response_summary: Option<String>,
    pub processed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct WebhookEventQuery {
    pub source: Option<String>,
    pub event_type: Option<String>,
    pub session_key: Option<String>,
    pub status: Option<WebhookEventStatus>,
    pub received_from_ms: Option<i64>,
    pub received_to_ms: Option<i64>,
    pub limit: i64,
    pub offset: i64,
    pub sort_order: WebhookEventSortOrder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookAgentRecord {
    pub id: String,
    pub hook_id: String,
    pub session_key: String,
    pub chat_id: String,
    pub sender_id: String,
    pub content: String,
    pub payload_json: Option<String>,
    pub metadata_json: Option<String>,
    pub status: WebhookEventStatus,
    pub error_message: Option<String>,
    pub response_summary: Option<String>,
    pub received_at_ms: i64,
    pub processed_at_ms: Option<i64>,
    pub remote_addr: Option<String>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewWebhookAgentRecord {
    pub id: String,
    pub hook_id: String,
    pub session_key: String,
    pub chat_id: String,
    pub sender_id: String,
    pub content: String,
    pub payload_json: Option<String>,
    pub metadata_json: Option<String>,
    pub status: WebhookEventStatus,
    pub error_message: Option<String>,
    pub response_summary: Option<String>,
    pub received_at_ms: i64,
    pub processed_at_ms: Option<i64>,
    pub remote_addr: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateWebhookAgentResult {
    pub status: WebhookEventStatus,
    pub error_message: Option<String>,
    pub response_summary: Option<String>,
    pub processed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct WebhookAgentQuery {
    pub hook_id: Option<String>,
    pub session_key: Option<String>,
    pub status: Option<WebhookEventStatus>,
    pub received_from_ms: Option<i64>,
    pub received_to_ms: Option<i64>,
    pub limit: i64,
    pub offset: i64,
    pub sort_order: WebhookEventSortOrder,
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
pub enum PendingQuestionStatus {
    Pending,
    Answered,
    Expired,
}

impl PendingQuestionStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Answered => "answered",
            Self::Expired => "expired",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "answered" => Some(Self::Answered),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingQuestionRecord {
    pub id: String,
    pub session_key: String,
    pub channel: String,
    pub chat_id: String,
    pub title: Option<String>,
    pub question_text: String,
    pub options_json: String,
    pub status: PendingQuestionStatus,
    pub selected_option_id: Option<String>,
    pub answered_by: Option<String>,
    pub expires_at_ms: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub answered_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewPendingQuestionRecord {
    pub id: String,
    pub session_key: String,
    pub channel: String,
    pub chat_id: String,
    pub title: Option<String>,
    pub question_text: String,
    pub options_json: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatJob {
    pub id: String,
    pub session_key: String,
    pub channel: String,
    pub chat_id: String,
    pub enabled: bool,
    pub every: String,
    pub prompt: String,
    pub silent_ack_token: String,
    pub recent_messages_limit: i64,
    pub timezone: String,
    pub next_run_at_ms: i64,
    pub last_run_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewHeartbeatJob {
    pub id: String,
    pub session_key: String,
    pub channel: String,
    pub chat_id: String,
    pub enabled: bool,
    pub every: String,
    pub prompt: String,
    pub silent_ack_token: String,
    pub recent_messages_limit: i64,
    pub timezone: String,
    pub next_run_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateHeartbeatJobPatch {
    pub session_key: Option<String>,
    pub channel: Option<String>,
    pub chat_id: Option<String>,
    pub every: Option<String>,
    pub prompt: Option<String>,
    pub silent_ack_token: Option<String>,
    pub recent_messages_limit: Option<i64>,
    pub timezone: Option<String>,
    pub next_run_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HeartbeatTaskStatus {
    Pending,
    Running,
    Success,
    Failed,
}

impl HeartbeatTaskStatus {
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
pub struct HeartbeatTaskRun {
    pub id: String,
    pub heartbeat_id: String,
    pub scheduled_at_ms: i64,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
    pub status: HeartbeatTaskStatus,
    pub attempt: i64,
    pub error_message: Option<String>,
    pub published_message_id: Option<String>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewHeartbeatTaskRun {
    pub id: String,
    pub heartbeat_id: String,
    pub scheduled_at_ms: i64,
    pub status: HeartbeatTaskStatus,
    pub attempt: i64,
    pub created_at_ms: i64,
}
