use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Arc;
use thiserror::Error;

/// 工具分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    /// 系统级工具。
    System,
    /// 数据访问工具。
    Data,
    /// 运行时辅助工具。
    Runtime,
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
}

/// 工具错误定义。
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("invalid args: {0}")]
    InvalidArgs(String),
    #[error("execution failed: {0}")]
    ExecutionFailed(String),
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
#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// 注册工具。
    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        self.tools.insert(tool.name().to_string(), Arc::new(tool));
    }

    /// 按名称获取工具。
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// 列出已注册工具名称。
    pub fn list(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }
}

/// 本地回显工具。
pub struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Echoes the provided text argument."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" }
            },
            "required": ["text"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Runtime
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing `text`".to_string()))?;

        Ok(ToolOutput {
            content_for_model: format!("EchoTool: {text}"),
            content_for_user: Some(text.to_string()),
        })
    }
}
