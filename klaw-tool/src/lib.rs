pub mod apply_patch;
pub mod approval;
pub mod archive;
pub mod channel_attachment;
pub mod cron_manager;
pub mod geo;
pub mod heartbeat_manager;
pub mod local_search;
pub mod memory;
pub mod shell;
pub mod skills_manager;
pub mod skills_registry;
pub mod sub_agent;
pub mod terminal_multiplexers;
pub mod voice;
pub mod web_fetch;
pub mod web_search;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use thiserror::Error;

pub use apply_patch::ApplyPatchTool;
pub use approval::ApprovalTool;
pub use archive::ArchiveTool;
pub use channel_attachment::ChannelAttachmentTool;
pub use cron_manager::CronManagerTool;
pub use geo::GeoTool;
pub use heartbeat_manager::HeartbeatManagerTool;
pub use local_search::LocalSearchTool;
pub use memory::MemoryTool;
pub use shell::ShellTool;
pub use skills_manager::SkillsManagerTool;
pub use skills_registry::SkillsRegistryTool;
pub use sub_agent::{SubAgentAuditSink, SubAgentTool};
pub use terminal_multiplexers::TerminalMultiplexerTool;
pub use voice::VoiceTool;
pub use web_fetch::WebFetchTool;
pub use web_search::WebSearchTool;

/// 工具分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    /// Read-only filesystem operations (read, list, glob).
    FilesystemRead,
    /// Write/modify filesystem operations (write, edit, delete).
    FilesystemWrite,
    /// Read-only network operations (web search, fetch).
    NetworkRead,
    /// Network operations that modify external state (HTTP POST, API calls).
    NetworkWrite,
    /// Shell command execution and process spawning.
    Shell,
    /// Hardware/peripheral operations (USB, serial, GPIO).
    Hardware,
    /// Memory read/write operations (workspace memory, long-term memory).
    Memory,
    /// Messaging operations (send messages via channels).
    Messaging,
    /// Destructive or high-risk operations (cron delete, etc.).
    Destructive,
}

/// 工具执行上下文。
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// 当前会话键。
    pub session_key: String,
    /// 上下文扩展信息。
    pub metadata: BTreeMap<String, serde_json::Value>,
}

/// 工具执行输出。
#[derive(Debug, Clone)]
pub struct ToolOutput {
    /// 返回给模型的内容。
    pub content_for_model: String,
    /// 可直接给用户的可读内容。
    pub content_for_user: Option<String>,
    /// 成功执行后仍需上抛给 runtime 的结构化信号。
    pub signals: Vec<ToolSignal>,
}

/// 工具侧发出的结构化信号，可由 runtime/channel 消费。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolSignal {
    pub kind: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

impl ToolSignal {
    pub fn approval_required(
        approval_id: &str,
        tool_name: &str,
        session_key: &str,
        risk_level: Option<&str>,
        command_preview: Option<&str>,
    ) -> Self {
        let mut payload = serde_json::json!({
            "approval_id": approval_id,
            "tool_name": tool_name,
            "session_key": session_key,
        });
        if let Some(risk_level) = risk_level.map(str::trim).filter(|value| !value.is_empty()) {
            payload["risk_level"] = serde_json::Value::String(risk_level.to_string());
        }
        if let Some(command_preview) = command_preview
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            payload["command_preview"] = serde_json::Value::String(command_preview.to_string());
        }
        Self {
            kind: "approval_required".to_string(),
            payload,
        }
    }

    pub fn stop_current_turn(reason: Option<&str>, source: Option<&str>) -> Self {
        let mut payload = serde_json::json!({});
        if let Some(reason) = reason.map(str::trim).filter(|value| !value.is_empty()) {
            payload["reason"] = serde_json::Value::String(reason.to_string());
        }
        if let Some(source) = source.map(str::trim).filter(|value| !value.is_empty()) {
            payload["source"] = serde_json::Value::String(source.to_string());
        }
        Self {
            kind: "stop".to_string(),
            payload,
        }
    }

    pub fn channel_attachment(
        kind: &str,
        archive_id: Option<&str>,
        path: Option<&str>,
        filename: Option<&str>,
        caption: Option<&str>,
    ) -> Self {
        let mut payload = serde_json::json!({
            "kind": kind.trim(),
        });
        if let Some(archive_id) = archive_id.map(str::trim).filter(|value| !value.is_empty()) {
            payload["archive_id"] = serde_json::Value::String(archive_id.to_string());
        }
        if let Some(path) = path.map(str::trim).filter(|value| !value.is_empty()) {
            payload["path"] = serde_json::Value::String(path.to_string());
        }
        if let Some(filename) = filename.map(str::trim).filter(|value| !value.is_empty()) {
            payload["filename"] = serde_json::Value::String(filename.to_string());
        }
        if let Some(caption) = caption.map(str::trim).filter(|value| !value.is_empty()) {
            payload["caption"] = serde_json::Value::String(caption.to_string());
        }
        Self {
            kind: "channel_attachment".to_string(),
            payload,
        }
    }
}

/// 工具错误定义。
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("invalid args: {0}")]
    InvalidArgs(String),
    #[error("execution failed: {0}")]
    ExecutionFailed(String),
    #[error("execution failed: {message}")]
    StructuredExecutionFailed {
        message: String,
        code: String,
        details: Option<serde_json::Value>,
        retryable: bool,
        signals: Vec<ToolSignal>,
    },
}

impl ToolError {
    #[must_use]
    pub fn structured_execution_failed(
        message: impl Into<String>,
        code: impl Into<String>,
        details: Option<serde_json::Value>,
        retryable: bool,
        signals: Vec<ToolSignal>,
    ) -> Self {
        Self::StructuredExecutionFailed {
            message: message.into(),
            code: code.into(),
            details,
            retryable,
            signals,
        }
    }

    #[must_use]
    pub fn code(&self) -> &str {
        match self {
            Self::InvalidArgs(_) => "invalid_args",
            Self::ExecutionFailed(_) => "execution_failed",
            Self::StructuredExecutionFailed { code, .. } => code.as_str(),
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::InvalidArgs(message) | Self::ExecutionFailed(message) => message.as_str(),
            Self::StructuredExecutionFailed { message, .. } => message.as_str(),
        }
    }

    #[must_use]
    pub fn details(&self) -> Option<&serde_json::Value> {
        match self {
            Self::StructuredExecutionFailed { details, .. } => details.as_ref(),
            _ => None,
        }
    }

    #[must_use]
    pub fn retryable(&self) -> bool {
        match self {
            Self::StructuredExecutionFailed { retryable, .. } => *retryable,
            _ => false,
        }
    }

    #[must_use]
    pub fn signals(&self) -> &[ToolSignal] {
        match self {
            Self::StructuredExecutionFailed { signals, .. } => signals.as_slice(),
            _ => &[],
        }
    }
}

/// 工具统一抽象。
#[async_trait]
pub trait Tool: Send + Sync {
    /// 工具名称。
    fn name(&self) -> &str;
    /// 工具描述。
    fn description(&self) -> &str;
    /// 参数 schema。
    fn parameters(&self) -> serde_json::Value;
    /// 工具分类。
    fn category(&self) -> ToolCategory;
    /// 执行工具逻辑。
    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError>;
}

/// 工具注册表。
#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: Arc<RwLock<BTreeMap<String, Arc<dyn Tool>>>>,
}

impl ToolRegistry {
    /// 注册工具。
    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        self.register_shared(Arc::new(tool));
    }

    /// 注册共享工具实例。
    pub fn register_shared(&mut self, tool: Arc<dyn Tool>) {
        self.tools
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .insert(tool.name().to_string(), tool);
    }

    /// 按名称获取工具。
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .get(name)
            .cloned()
    }

    /// 列出已注册工具名称。
    pub fn list(&self) -> Vec<String> {
        self.tools
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    /// 注销工具。
    pub fn unregister(&self, name: &str) -> bool {
        self.tools
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .remove(name)
            .is_some()
    }

    /// 注销多个工具。
    pub fn unregister_many(&self, names: &[&str]) -> usize {
        let mut guard = self.tools.write().unwrap_or_else(|err| err.into_inner());
        let mut count = 0;
        for name in names {
            if guard.remove(*name).is_some() {
                count += 1;
            }
        }
        count
    }
}
