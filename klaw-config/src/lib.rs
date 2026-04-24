use klaw_util::system_timezone_name;
use serde::{Deserialize, Deserializer, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub model_provider: String,
    /// Legacy compatibility field parsed from older configs.
    ///
    /// Default provider/model routing ignores this value. Use
    /// `model_providers.<id>.default_model` for provider defaults and `/model`
    /// for explicit per-session overrides.
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_conversation_history_limit")]
    pub conversation_history_limit: usize,
    pub model_providers: BTreeMap<String, ModelProviderConfig>,
    #[serde(default)]
    pub voice: VoiceConfig,
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub channels: ChannelsConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub acp: AcpConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub cron: CronConfig,
    #[serde(default)]
    pub heartbeat: HeartbeatConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub observability: ObservabilityConfig,
    #[serde(default)]
    pub profiler: ProfilerConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        let model_provider = "openai".to_string();
        let mut model_providers = BTreeMap::new();
        model_providers.insert(model_provider.clone(), ModelProviderConfig::default());
        Self {
            model_provider,
            model: None,
            conversation_history_limit: default_conversation_history_limit(),
            model_providers,
            voice: VoiceConfig::default(),
            gateway: GatewayConfig::default(),
            channels: ChannelsConfig::default(),
            memory: MemoryConfig::default(),
            mcp: McpConfig::default(),
            acp: AcpConfig::default(),
            tools: ToolsConfig::default(),
            cron: CronConfig::default(),
            heartbeat: HeartbeatConfig::default(),
            skills: SkillsConfig::default(),
            storage: StorageConfig::default(),
            observability: ObservabilityConfig::default(),
            profiler: ProfilerConfig::default(),
        }
    }
}

fn default_conversation_history_limit() -> usize {
    40
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    /// Controls whether voice runtime features are active.
    ///
    /// The model-facing `voice` tool is registered only when both `voice.enabled` and
    /// `tools.voice.enabled` are true.
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub stt_provider: SttProviderKind,
    #[serde(default)]
    pub tts_provider: TtsProviderKind,
    #[serde(default = "default_voice_language")]
    pub default_language: String,
    #[serde(default)]
    pub default_voice_id: Option<String>,
    #[serde(default)]
    pub providers: VoiceProvidersConfig,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            stt_provider: SttProviderKind::default(),
            tts_provider: TtsProviderKind::default(),
            default_language: default_voice_language(),
            default_voice_id: None,
            providers: VoiceProvidersConfig::default(),
        }
    }
}

fn default_voice_language() -> String {
    "zh-CN".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SttProviderKind {
    #[default]
    Deepgram,
    Assemblyai,
}

impl SttProviderKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deepgram => "deepgram",
            Self::Assemblyai => "assemblyai",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TtsProviderKind {
    #[default]
    Elevenlabs,
}

impl TtsProviderKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Elevenlabs => "elevenlabs",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoiceProvidersConfig {
    #[serde(default)]
    pub elevenlabs: ElevenLabsVoiceConfig,
    #[serde(default)]
    pub deepgram: DeepgramVoiceConfig,
    #[serde(default)]
    pub assemblyai: AssemblyAiVoiceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElevenLabsVoiceConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_elevenlabs_api_key_env")]
    pub api_key_env: String,
    #[serde(default = "default_elevenlabs_base_url")]
    pub base_url: String,
    #[serde(default = "default_elevenlabs_streaming_base_url")]
    pub streaming_base_url: String,
    #[serde(default = "default_elevenlabs_model")]
    pub default_model: String,
    #[serde(default)]
    pub default_voice_id: Option<String>,
}

impl Default for ElevenLabsVoiceConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_key_env: default_elevenlabs_api_key_env(),
            base_url: default_elevenlabs_base_url(),
            streaming_base_url: default_elevenlabs_streaming_base_url(),
            default_model: default_elevenlabs_model(),
            default_voice_id: None,
        }
    }
}

impl ElevenLabsVoiceConfig {
    #[must_use]
    pub fn resolve_api_key(&self) -> Option<String> {
        self.api_key.clone().or_else(|| {
            (!self.api_key_env.trim().is_empty())
                .then(|| std::env::var(self.api_key_env.trim()).ok())
                .flatten()
        })
    }
}

fn default_elevenlabs_api_key_env() -> String {
    "ELEVENLABS_API_KEY".to_string()
}

fn default_elevenlabs_base_url() -> String {
    "https://api.elevenlabs.io".to_string()
}

fn default_elevenlabs_streaming_base_url() -> String {
    "wss://api.elevenlabs.io".to_string()
}

fn default_elevenlabs_model() -> String {
    "eleven_multilingual_v2".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepgramVoiceConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_deepgram_api_key_env")]
    pub api_key_env: String,
    #[serde(default = "default_deepgram_base_url")]
    pub base_url: String,
    #[serde(default = "default_deepgram_streaming_base_url")]
    pub streaming_base_url: String,
    #[serde(default = "default_deepgram_stt_model")]
    pub stt_model: String,
}

impl Default for DeepgramVoiceConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_key_env: default_deepgram_api_key_env(),
            base_url: default_deepgram_base_url(),
            streaming_base_url: default_deepgram_streaming_base_url(),
            stt_model: default_deepgram_stt_model(),
        }
    }
}

impl DeepgramVoiceConfig {
    #[must_use]
    pub fn resolve_api_key(&self) -> Option<String> {
        self.api_key.clone().or_else(|| {
            (!self.api_key_env.trim().is_empty())
                .then(|| std::env::var(self.api_key_env.trim()).ok())
                .flatten()
        })
    }
}

fn default_deepgram_api_key_env() -> String {
    "DEEPGRAM_API_KEY".to_string()
}

fn default_deepgram_base_url() -> String {
    "https://api.deepgram.com".to_string()
}

fn default_deepgram_streaming_base_url() -> String {
    "wss://api.deepgram.com".to_string()
}

fn default_deepgram_stt_model() -> String {
    "nova-2".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssemblyAiVoiceConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_assemblyai_api_key_env")]
    pub api_key_env: String,
    #[serde(default = "default_assemblyai_base_url")]
    pub base_url: String,
    #[serde(default = "default_assemblyai_streaming_base_url")]
    pub streaming_base_url: String,
    #[serde(default = "default_assemblyai_stt_model")]
    pub stt_model: String,
}

impl Default for AssemblyAiVoiceConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_key_env: default_assemblyai_api_key_env(),
            base_url: default_assemblyai_base_url(),
            streaming_base_url: default_assemblyai_streaming_base_url(),
            stt_model: default_assemblyai_stt_model(),
        }
    }
}

impl AssemblyAiVoiceConfig {
    #[must_use]
    pub fn resolve_api_key(&self) -> Option<String> {
        self.api_key.clone().or_else(|| {
            (!self.api_key_env.trim().is_empty())
                .then(|| std::env::var(self.api_key_env.trim()).ok())
                .flatten()
        })
    }
}

fn default_assemblyai_api_key_env() -> String {
    "ASSEMBLYAI_API_KEY".to_string()
}

fn default_assemblyai_base_url() -> String {
    "https://api.assemblyai.com".to_string()
}

fn default_assemblyai_streaming_base_url() -> String {
    "wss://streaming.assemblyai.com".to_string()
}

fn default_assemblyai_stt_model() -> String {
    "universal".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageConfig {
    #[serde(default)]
    pub root_dir: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelsConfig {
    #[serde(default)]
    pub dingtalk: Vec<DingtalkConfig>,
    #[serde(default)]
    pub telegram: Vec<TelegramConfig>,
    #[serde(default)]
    pub websocket: Vec<WebsocketConfig>,
    #[serde(default)]
    pub disable_session_commands_for: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DingtalkConfig {
    pub id: String,
    #[serde(default = "default_channel_enabled")]
    pub enabled: bool,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default = "default_dingtalk_bot_title")]
    pub bot_title: String,
    #[serde(default)]
    pub show_reasoning: bool,
    #[serde(default)]
    pub stream_output: bool,
    #[serde(default)]
    pub stream_template_id: String,
    #[serde(default = "default_dingtalk_stream_content_key")]
    pub stream_content_key: String,
    #[serde(default)]
    pub allowlist: Vec<String>,
    #[serde(default)]
    pub proxy: DingtalkProxyConfig,
}

impl Default for DingtalkConfig {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            enabled: default_channel_enabled(),
            client_id: String::new(),
            client_secret: String::new(),
            bot_title: default_dingtalk_bot_title(),
            show_reasoning: false,
            stream_output: false,
            stream_template_id: String::new(),
            stream_content_key: default_dingtalk_stream_content_key(),
            allowlist: Vec::new(),
            proxy: DingtalkProxyConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DingtalkProxyConfig {
    pub enabled: bool,
    pub url: String,
}

fn default_channel_enabled() -> bool {
    true
}

fn default_websocket_stream_output() -> bool {
    true
}

fn default_dingtalk_bot_title() -> String {
    "Klaw".to_string()
}

fn default_dingtalk_stream_content_key() -> String {
    "content".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub id: String,
    #[serde(default = "default_channel_enabled")]
    pub enabled: bool,
    pub bot_token: String,
    #[serde(default)]
    pub show_reasoning: bool,
    #[serde(default)]
    pub stream_output: bool,
    #[serde(default)]
    pub allowlist: Vec<String>,
    #[serde(default)]
    pub proxy: TelegramProxyConfig,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            enabled: default_channel_enabled(),
            bot_token: String::new(),
            show_reasoning: false,
            stream_output: false,
            allowlist: Vec::new(),
            proxy: TelegramProxyConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebsocketConfig {
    pub id: String,
    #[serde(default = "default_channel_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub show_reasoning: bool,
    #[serde(default = "default_websocket_stream_output")]
    pub stream_output: bool,
}

impl Default for WebsocketConfig {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            enabled: default_channel_enabled(),
            show_reasoning: false,
            stream_output: default_websocket_stream_output(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalAttachmentConfig {
    pub allowlist: Vec<String>,
    pub max_bytes: u64,
}

impl Default for LocalAttachmentConfig {
    fn default() -> Self {
        Self {
            allowlist: Vec::new(),
            max_bytes: 10 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramProxyConfig {
    pub enabled: bool,
    pub url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    #[serde(default)]
    pub defaults: HeartbeatDefaultsConfig,
    #[serde(default)]
    pub sessions: Vec<HeartbeatSessionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatDefaultsConfig {
    #[serde(default = "default_heartbeat_enabled")]
    pub enabled: bool,
    #[serde(default = "default_heartbeat_every")]
    pub every: String,
    #[serde(default = "default_heartbeat_prompt")]
    pub prompt: String,
    #[serde(default = "default_heartbeat_silent_ack_token")]
    pub silent_ack_token: String,
    #[serde(default = "default_heartbeat_timezone")]
    pub timezone: String,
}

impl Default for HeartbeatDefaultsConfig {
    fn default() -> Self {
        Self {
            enabled: default_heartbeat_enabled(),
            every: default_heartbeat_every(),
            prompt: default_heartbeat_prompt(),
            silent_ack_token: default_heartbeat_silent_ack_token(),
            timezone: default_heartbeat_timezone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatSessionConfig {
    pub session_key: String,
    pub chat_id: String,
    pub channel: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub every: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub silent_ack_token: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
}

fn default_heartbeat_enabled() -> bool {
    true
}

fn default_heartbeat_every() -> String {
    "30m".to_string()
}

fn default_heartbeat_prompt() -> String {
    "Review the session state. If no user-visible action is needed, reply with exactly HEARTBEAT_OK and nothing else."
        .to_string()
}

fn default_heartbeat_silent_ack_token() -> String {
    "HEARTBEAT_OK".to_string()
}

fn default_heartbeat_timezone() -> String {
    system_timezone_name()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GatewayAuthConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub env_key: Option<String>,
}

impl GatewayAuthConfig {
    pub fn resolve_token(&self) -> Option<String> {
        if let Some(token) = &self.token {
            return Some(token.clone());
        }
        if let Some(env_key) = &self.env_key {
            if let Ok(val) = std::env::var(env_key) {
                return Some(val);
            }
        }
        None
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled && self.resolve_token().is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TailscaleMode {
    #[default]
    Off,
    Serve,
    Funnel,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GatewayTailscaleConfig {
    #[serde(default)]
    pub mode: TailscaleMode,
    #[serde(default = "default_tailscale_reset_on_exit")]
    pub reset_on_exit: bool,
}

fn default_tailscale_reset_on_exit() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_gateway_listen_ip")]
    pub listen_ip: String,
    #[serde(default = "default_gateway_listen_port")]
    pub listen_port: u16,
    #[serde(default)]
    pub auth: GatewayAuthConfig,
    #[serde(default)]
    pub tailscale: GatewayTailscaleConfig,
    #[serde(default)]
    pub tls: GatewayTlsConfig,
    #[serde(default)]
    pub webhook: GatewayWebhookConfig,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen_ip: default_gateway_listen_ip(),
            listen_port: default_gateway_listen_port(),
            auth: GatewayAuthConfig::default(),
            tailscale: GatewayTailscaleConfig::default(),
            tls: GatewayTlsConfig::default(),
            webhook: GatewayWebhookConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GatewayWebhookConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_gateway_webhook_events_config")]
    pub events: GatewayWebhookEndpointConfig,
    #[serde(default = "default_gateway_webhook_agents_config")]
    pub agents: GatewayWebhookEndpointConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayWebhookEndpointConfig {
    #[serde(default)]
    pub enabled: bool,
    pub max_body_bytes: usize,
}

impl Default for GatewayWebhookConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            events: default_gateway_webhook_events_config(),
            agents: default_gateway_webhook_agents_config(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct GatewayWebhookConfigCompat {
    #[serde(default)]
    enabled: bool,
    #[serde(default, rename = "path")]
    _path: Option<String>,
    #[serde(default)]
    max_body_bytes: Option<usize>,
    #[serde(default)]
    events: Option<GatewayWebhookEndpointConfigCompat>,
    #[serde(default)]
    agents: Option<GatewayWebhookEndpointConfigCompat>,
}

#[derive(Debug, Default, Deserialize)]
struct GatewayWebhookEndpointConfigCompat {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default, rename = "path")]
    _path: Option<String>,
    #[serde(default)]
    max_body_bytes: Option<usize>,
}

impl<'de> Deserialize<'de> for GatewayWebhookConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let compat = GatewayWebhookConfigCompat::deserialize(deserializer)?;
        let mut config = GatewayWebhookConfig::default();
        config.enabled = compat.enabled;
        if let Some(events) = compat.events {
            if let Some(enabled) = events.enabled {
                config.events.enabled = enabled;
            }
            if let Some(max_body_bytes) = events.max_body_bytes {
                config.events.max_body_bytes = max_body_bytes;
            }
        }
        if let Some(agents) = compat.agents {
            if let Some(enabled) = agents.enabled {
                config.agents.enabled = enabled;
            }
            if let Some(max_body_bytes) = agents.max_body_bytes {
                config.agents.max_body_bytes = max_body_bytes;
            }
        }
        if let Some(max_body_bytes) = compat.max_body_bytes {
            config.events.max_body_bytes = max_body_bytes;
        }
        Ok(config)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GatewayTlsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub cert_path: Option<String>,
    #[serde(default)]
    pub key_path: Option<String>,
}

fn default_gateway_listen_ip() -> String {
    "127.0.0.1".to_string()
}

fn default_gateway_listen_port() -> u16 {
    0
}

fn default_gateway_webhook_max_body_bytes() -> usize {
    262_144
}

fn default_gateway_webhook_events_config() -> GatewayWebhookEndpointConfig {
    GatewayWebhookEndpointConfig {
        enabled: true,
        max_body_bytes: default_gateway_webhook_max_body_bytes(),
    }
}

fn default_gateway_webhook_agents_config() -> GatewayWebhookEndpointConfig {
    GatewayWebhookEndpointConfig {
        enabled: false,
        max_body_bytes: default_gateway_webhook_max_body_bytes(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default = "default_mcp_startup_timeout_seconds")]
    pub startup_timeout_seconds: u64,
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            startup_timeout_seconds: default_mcp_startup_timeout_seconds(),
            servers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpServerMode {
    Stdio,
    Sse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServerConfig {
    pub id: String,
    #[serde(default = "default_mcp_server_enabled")]
    pub enabled: bool,
    pub mode: McpServerMode,
    #[serde(default = "default_mcp_server_tool_timeout_seconds")]
    pub tool_timeout_seconds: u64,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            enabled: default_mcp_server_enabled(),
            mode: McpServerMode::Stdio,
            tool_timeout_seconds: default_mcp_server_tool_timeout_seconds(),
            command: None,
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            url: None,
            headers: BTreeMap::new(),
        }
    }
}

fn default_mcp_startup_timeout_seconds() -> u64 {
    60
}

fn default_mcp_server_enabled() -> bool {
    true
}

fn default_mcp_server_tool_timeout_seconds() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpConfig {
    #[serde(default = "default_acp_startup_timeout_seconds")]
    pub startup_timeout_seconds: u64,
    #[serde(default = "default_acp_agents")]
    pub agents: Vec<AcpAgentConfig>,
}

impl Default for AcpConfig {
    fn default() -> Self {
        Self {
            startup_timeout_seconds: default_acp_startup_timeout_seconds(),
            agents: default_acp_agents(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpAgentConfig {
    pub id: String,
    #[serde(default = "default_acp_agent_enabled")]
    pub enabled: bool,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub description: String,
}

impl Default for AcpAgentConfig {
    fn default() -> Self {
        default_claude_acp_agent()
    }
}

fn default_acp_startup_timeout_seconds() -> u64 {
    30
}

fn default_acp_agents() -> Vec<AcpAgentConfig> {
    vec![default_claude_acp_agent(), default_codex_acp_agent()]
}

fn default_acp_agent_enabled() -> bool {
    true
}

fn default_claude_acp_agent() -> AcpAgentConfig {
    AcpAgentConfig {
        id: "claude_code".to_string(),
        enabled: default_acp_agent_enabled(),
        command: "npx".to_string(),
        args: vec![
            "-y".to_string(),
            "@zed-industries/claude-agent-acp".to_string(),
        ],
        env: BTreeMap::new(),
        description: "Claude Code ACP adapter template".to_string(),
    }
}

fn default_codex_acp_agent() -> AcpAgentConfig {
    AcpAgentConfig {
        id: "codex".to_string(),
        enabled: default_acp_agent_enabled(),
        command: "npx".to_string(),
        args: vec!["-y".to_string(), "@zed-industries/codex-acp".to_string()],
        env: BTreeMap::new(),
        description: "Codex ACP adapter template".to_string(),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryConfig {
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub archive: MemoryArchiveConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryArchiveConfig {
    #[serde(default = "default_memory_archive_enabled")]
    pub enabled: bool,
    #[serde(default = "default_memory_archive_schedule")]
    pub schedule: String,
    #[serde(default = "default_memory_archive_max_age_days")]
    pub max_age_days: i64,
    #[serde(default = "default_memory_archive_summary_max_sources")]
    pub summary_max_sources: usize,
    #[serde(default = "default_memory_archive_summary_timeout_secs")]
    pub summary_timeout_secs: u64,
    #[serde(default = "default_memory_archive_command_timeout_secs")]
    pub command_timeout_secs: u64,
}

impl Default for MemoryArchiveConfig {
    fn default() -> Self {
        Self {
            enabled: default_memory_archive_enabled(),
            schedule: default_memory_archive_schedule(),
            max_age_days: default_memory_archive_max_age_days(),
            summary_max_sources: default_memory_archive_summary_max_sources(),
            summary_timeout_secs: default_memory_archive_summary_timeout_secs(),
            command_timeout_secs: default_memory_archive_command_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_memory_embedding_provider")]
    pub provider: String,
    #[serde(default = "default_memory_embedding_model")]
    pub model: String,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: default_memory_embedding_provider(),
            model: default_memory_embedding_model(),
        }
    }
}

fn default_memory_embedding_provider() -> String {
    "openai".to_string()
}

fn default_memory_embedding_model() -> String {
    "text-embedding-3-small".to_string()
}

fn default_memory_archive_enabled() -> bool {
    true
}

fn default_memory_archive_schedule() -> String {
    "0 0 2 * * *".to_string()
}

fn default_memory_archive_max_age_days() -> i64 {
    30
}

fn default_memory_archive_summary_max_sources() -> usize {
    8
}

fn default_memory_archive_summary_timeout_secs() -> u64 {
    60
}

fn default_memory_archive_command_timeout_secs() -> u64 {
    120
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProviderConfig {
    #[serde(default)]
    pub name: Option<String>,
    pub base_url: String,
    pub wire_api: String,
    pub default_model: String,
    #[serde(default)]
    pub tokenizer_path: Option<String>,
    #[serde(default)]
    pub proxy: bool,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub env_key: Option<String>,
}

impl Default for ModelProviderConfig {
    fn default() -> Self {
        Self {
            name: Some("OpenAI".to_string()),
            base_url: "https://api.openai.com/v1".to_string(),
            wire_api: "chat_completions".to_string(),
            default_model: "gpt-4o-mini".to_string(),
            tokenizer_path: None,
            proxy: false,
            stream: false,
            api_key: None,
            env_key: Some("OPENAI_API_KEY".to_string()),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default)]
    pub archive: ArchiveToolConfig,
    #[serde(default)]
    pub channel_attachment: ChannelAttachmentToolConfig,
    #[serde(default)]
    pub voice: VoiceToolConfig,
    #[serde(default)]
    pub apply_patch: ApplyPatchConfig,
    #[serde(default)]
    pub file_read: FileReadConfig,
    #[serde(default)]
    pub shell: ShellConfig,
    #[serde(default)]
    pub approval: ApprovalToolConfig,
    #[serde(default)]
    pub ask_question: AskQuestionToolConfig,
    #[serde(default)]
    pub geo: GeoToolConfig,
    #[serde(default)]
    pub local_search: LocalSearchConfig,
    #[serde(default)]
    pub terminal_multiplexers: TerminalMultiplexersConfig,
    #[serde(default)]
    pub cron_manager: CronManagerConfig,
    #[serde(default)]
    pub heartbeat_manager: HeartbeatManagerConfig,
    #[serde(default)]
    pub skills_registry: SkillsRegistryToolConfig,
    #[serde(default)]
    pub skills_manager: SkillsManagerToolConfig,
    #[serde(default)]
    pub memory: MemoryToolConfig,
    #[serde(default)]
    pub web_fetch: WebFetchConfig,
    #[serde(default)]
    pub web_search: WebSearchConfig,
    #[serde(default)]
    pub sub_agent: SubAgentConfig,
}

pub trait ToolEnabled {
    fn enabled(&self) -> bool;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveToolConfig {
    #[serde(default = "default_archive_tool_enabled")]
    pub enabled: bool,
}

impl Default for ArchiveToolConfig {
    fn default() -> Self {
        Self {
            enabled: default_archive_tool_enabled(),
        }
    }
}

impl ToolEnabled for ArchiveToolConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

fn default_archive_tool_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelAttachmentToolConfig {
    #[serde(default = "default_channel_attachment_tool_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub local_attachments: LocalAttachmentConfig,
}

impl Default for ChannelAttachmentToolConfig {
    fn default() -> Self {
        Self {
            enabled: default_channel_attachment_tool_enabled(),
            local_attachments: LocalAttachmentConfig::default(),
        }
    }
}

impl ToolEnabled for ChannelAttachmentToolConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

fn default_channel_attachment_tool_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceToolConfig {
    #[serde(default = "default_voice_tool_enabled")]
    pub enabled: bool,
}

impl Default for VoiceToolConfig {
    fn default() -> Self {
        Self {
            enabled: default_voice_tool_enabled(),
        }
    }
}

impl ToolEnabled for VoiceToolConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

fn default_voice_tool_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyPatchConfig {
    #[serde(default = "default_apply_patch_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub workspace: Option<String>,
    #[serde(default = "default_apply_patch_allow_absolute_paths")]
    pub allow_absolute_paths: bool,
    #[serde(default)]
    pub allowed_roots: Vec<String>,
}

impl Default for ApplyPatchConfig {
    fn default() -> Self {
        Self {
            enabled: default_apply_patch_enabled(),
            workspace: None,
            allow_absolute_paths: default_apply_patch_allow_absolute_paths(),
            allowed_roots: Vec::new(),
        }
    }
}

impl ToolEnabled for ApplyPatchConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

fn default_apply_patch_enabled() -> bool {
    true
}

fn default_apply_patch_allow_absolute_paths() -> bool {
    false
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalToolConfig {
    #[serde(default = "default_approval_tool_enabled")]
    pub enabled: bool,
}

impl Default for ApprovalToolConfig {
    fn default() -> Self {
        Self {
            enabled: default_approval_tool_enabled(),
        }
    }
}

impl ToolEnabled for ApprovalToolConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

fn default_approval_tool_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskQuestionToolConfig {
    #[serde(default = "default_ask_question_tool_enabled")]
    pub enabled: bool,
}

impl Default for AskQuestionToolConfig {
    fn default() -> Self {
        Self {
            enabled: default_ask_question_tool_enabled(),
        }
    }
}

impl ToolEnabled for AskQuestionToolConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

fn default_ask_question_tool_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoToolConfig {
    #[serde(default = "default_geo_tool_enabled")]
    pub enabled: bool,
}

impl Default for GeoToolConfig {
    fn default() -> Self {
        Self {
            enabled: default_geo_tool_enabled(),
        }
    }
}

impl ToolEnabled for GeoToolConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

fn default_geo_tool_enabled() -> bool {
    false
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalSearchConfig {
    #[serde(default = "default_local_search_enabled")]
    pub enabled: bool,
}

impl Default for LocalSearchConfig {
    fn default() -> Self {
        Self {
            enabled: default_local_search_enabled(),
        }
    }
}

impl ToolEnabled for LocalSearchConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

fn default_local_search_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReadConfig {
    #[serde(default = "default_file_read_enabled")]
    pub enabled: bool,
    #[serde(default = "default_file_read_max_lines")]
    pub max_lines: usize,
    #[serde(default = "default_file_read_max_bytes")]
    pub max_bytes: usize,
    #[serde(default = "default_file_read_auto_resize_images")]
    pub auto_resize_images: bool,
}

impl Default for FileReadConfig {
    fn default() -> Self {
        Self {
            enabled: default_file_read_enabled(),
            max_lines: default_file_read_max_lines(),
            max_bytes: default_file_read_max_bytes(),
            auto_resize_images: default_file_read_auto_resize_images(),
        }
    }
}

impl ToolEnabled for FileReadConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

fn default_file_read_enabled() -> bool {
    true
}

fn default_file_read_max_lines() -> usize {
    2000
}

fn default_file_read_max_bytes() -> usize {
    50 * 1024
}

fn default_file_read_auto_resize_images() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalMultiplexersConfig {
    #[serde(default = "default_terminal_multiplexers_enabled")]
    pub enabled: bool,
}

impl Default for TerminalMultiplexersConfig {
    fn default() -> Self {
        Self {
            enabled: default_terminal_multiplexers_enabled(),
        }
    }
}

impl ToolEnabled for TerminalMultiplexersConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

fn default_terminal_multiplexers_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronManagerConfig {
    #[serde(default = "default_cron_manager_enabled")]
    pub enabled: bool,
}

impl Default for CronManagerConfig {
    fn default() -> Self {
        Self {
            enabled: default_cron_manager_enabled(),
        }
    }
}

impl ToolEnabled for CronManagerConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

fn default_cron_manager_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatManagerConfig {
    #[serde(default = "default_heartbeat_manager_enabled")]
    pub enabled: bool,
}

impl Default for HeartbeatManagerConfig {
    fn default() -> Self {
        Self {
            enabled: default_heartbeat_manager_enabled(),
        }
    }
}

impl ToolEnabled for HeartbeatManagerConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

fn default_heartbeat_manager_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsRegistryToolConfig {
    #[serde(default = "default_skills_registry_tool_enabled")]
    pub enabled: bool,
}

impl Default for SkillsRegistryToolConfig {
    fn default() -> Self {
        Self {
            enabled: default_skills_registry_tool_enabled(),
        }
    }
}

impl ToolEnabled for SkillsRegistryToolConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

fn default_skills_registry_tool_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsManagerToolConfig {
    #[serde(default = "default_skills_manager_tool_enabled")]
    pub enabled: bool,
}

impl Default for SkillsManagerToolConfig {
    fn default() -> Self {
        Self {
            enabled: default_skills_manager_tool_enabled(),
        }
    }
}

impl ToolEnabled for SkillsManagerToolConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

fn default_skills_manager_tool_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsConfig {
    #[serde(default = "default_skills_sync_timeout")]
    pub sync_timeout: u64,
    #[serde(flatten)]
    pub registries: BTreeMap<String, SkillsRegistryConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsRegistryConfig {
    pub address: String,
    #[serde(default)]
    pub installed: Vec<String>,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        let mut registries = BTreeMap::new();
        registries.insert(
            "anthropic".to_string(),
            SkillsRegistryConfig {
                address: "https://github.com/anthropics/skills".to_string(),
                installed: Vec::new(),
            },
        );
        Self {
            sync_timeout: default_skills_sync_timeout(),
            registries,
        }
    }
}

fn default_skills_sync_timeout() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryToolConfig {
    #[serde(default = "default_memory_tool_enabled")]
    pub enabled: bool,
    #[serde(default = "default_memory_tool_search_limit")]
    pub search_limit: usize,
    #[serde(default = "default_memory_tool_fts_limit")]
    pub fts_limit: usize,
    #[serde(default = "default_memory_tool_vector_limit")]
    pub vector_limit: usize,
    #[serde(default = "default_memory_tool_use_vector")]
    pub use_vector: bool,
}

impl Default for MemoryToolConfig {
    fn default() -> Self {
        Self {
            enabled: default_memory_tool_enabled(),
            search_limit: default_memory_tool_search_limit(),
            fts_limit: default_memory_tool_fts_limit(),
            vector_limit: default_memory_tool_vector_limit(),
            use_vector: default_memory_tool_use_vector(),
        }
    }
}

impl ToolEnabled for MemoryToolConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

fn default_memory_tool_enabled() -> bool {
    true
}

fn default_memory_tool_search_limit() -> usize {
    8
}

fn default_memory_tool_fts_limit() -> usize {
    20
}

fn default_memory_tool_vector_limit() -> usize {
    20
}

fn default_memory_tool_use_vector() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellConfig {
    #[serde(default = "default_shell_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub workspace: Option<String>,
    #[serde(default = "default_shell_blocked_patterns")]
    pub blocked_patterns: Vec<String>,
    #[serde(default = "default_shell_unsafe_patterns")]
    pub unsafe_patterns: Vec<String>,
    #[serde(default = "default_shell_allow_login_shell")]
    pub allow_login_shell: bool,
    #[serde(default = "default_shell_max_timeout_ms")]
    pub max_timeout_ms: u64,
    #[serde(default = "default_shell_max_output_bytes")]
    pub max_output_bytes: usize,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            enabled: default_shell_enabled(),
            workspace: None,
            blocked_patterns: default_shell_blocked_patterns(),
            unsafe_patterns: default_shell_unsafe_patterns(),
            allow_login_shell: default_shell_allow_login_shell(),
            max_timeout_ms: default_shell_max_timeout_ms(),
            max_output_bytes: default_shell_max_output_bytes(),
        }
    }
}

impl ToolEnabled for ShellConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

fn default_shell_blocked_patterns() -> Vec<String> {
    vec![":(){ :|:& };:".to_string()]
}

fn default_shell_unsafe_patterns() -> Vec<String> {
    vec![
        "rm -rf /".to_string(),
        "rm -rf ~".to_string(),
        "mkfs".to_string(),
        "shutdown".to_string(),
        "reboot".to_string(),
    ]
}

fn default_shell_enabled() -> bool {
    true
}

fn default_shell_allow_login_shell() -> bool {
    true
}

fn default_shell_max_timeout_ms() -> u64 {
    120_000
}

fn default_shell_max_output_bytes() -> usize {
    128 * 1024
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchConfig {
    #[serde(default = "default_web_search_enabled")]
    pub enabled: bool,
    #[serde(default = "default_web_search_provider")]
    pub provider: String,
    #[serde(default)]
    pub tavily: TavilyWebSearchConfig,
    #[serde(default)]
    pub brave: BraveWebSearchConfig,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            enabled: default_web_search_enabled(),
            provider: default_web_search_provider(),
            tavily: TavilyWebSearchConfig::default(),
            brave: BraveWebSearchConfig::default(),
        }
    }
}

impl ToolEnabled for WebSearchConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchConfig {
    #[serde(default = "default_web_fetch_enabled")]
    pub enabled: bool,
    #[serde(default = "default_web_fetch_max_chars")]
    pub max_chars: usize,
    #[serde(default = "default_web_fetch_timeout_seconds")]
    pub timeout_seconds: u64,
    #[serde(default = "default_web_fetch_cache_ttl_minutes")]
    pub cache_ttl_minutes: u64,
    #[serde(default = "default_web_fetch_max_redirects")]
    pub max_redirects: u8,
    #[serde(default = "default_web_fetch_readability")]
    pub readability: bool,
    #[serde(default)]
    pub ssrf_allowlist: Vec<String>,
}

impl Default for WebFetchConfig {
    fn default() -> Self {
        Self {
            enabled: default_web_fetch_enabled(),
            max_chars: default_web_fetch_max_chars(),
            timeout_seconds: default_web_fetch_timeout_seconds(),
            cache_ttl_minutes: default_web_fetch_cache_ttl_minutes(),
            max_redirects: default_web_fetch_max_redirects(),
            readability: default_web_fetch_readability(),
            ssrf_allowlist: Vec::new(),
        }
    }
}

impl ToolEnabled for WebFetchConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

fn default_web_fetch_max_chars() -> usize {
    50_000
}

fn default_web_fetch_enabled() -> bool {
    true
}

fn default_web_fetch_timeout_seconds() -> u64 {
    15
}

fn default_web_fetch_cache_ttl_minutes() -> u64 {
    10
}

fn default_web_fetch_max_redirects() -> u8 {
    3
}

fn default_web_fetch_readability() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentConfig {
    #[serde(default = "default_sub_agent_enabled")]
    pub enabled: bool,
    #[serde(default = "default_sub_agent_max_iterations")]
    pub max_iterations: u32,
    #[serde(default = "default_sub_agent_max_tool_calls")]
    pub max_tool_calls: u32,
    #[serde(default = "default_sub_agent_inherit_parent_tools")]
    pub inherit_parent_tools: bool,
    #[serde(default = "default_sub_agent_exclude_tools")]
    pub exclude_tools: Vec<String>,
}

impl Default for SubAgentConfig {
    fn default() -> Self {
        Self {
            enabled: default_sub_agent_enabled(),
            max_iterations: default_sub_agent_max_iterations(),
            max_tool_calls: default_sub_agent_max_tool_calls(),
            inherit_parent_tools: default_sub_agent_inherit_parent_tools(),
            exclude_tools: default_sub_agent_exclude_tools(),
        }
    }
}

impl ToolEnabled for SubAgentConfig {
    fn enabled(&self) -> bool {
        self.enabled
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronConfig {
    #[serde(default = "default_cron_tick_ms")]
    pub tick_ms: u64,
    #[serde(default = "default_cron_runtime_tick_ms")]
    pub runtime_tick_ms: u64,
    #[serde(default = "default_cron_runtime_drain_batch")]
    pub runtime_drain_batch: usize,
    #[serde(default = "default_cron_batch_limit")]
    pub batch_limit: i64,
    #[serde(default)]
    pub missed_run_policy: CronMissedRunPolicy,
}

impl Default for CronConfig {
    fn default() -> Self {
        Self {
            tick_ms: default_cron_tick_ms(),
            runtime_tick_ms: default_cron_runtime_tick_ms(),
            runtime_drain_batch: default_cron_runtime_drain_batch(),
            batch_limit: default_cron_batch_limit(),
            missed_run_policy: CronMissedRunPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CronMissedRunPolicy {
    #[default]
    Skip,
    CatchUp,
}

fn default_cron_tick_ms() -> u64 {
    1_000
}

fn default_cron_runtime_tick_ms() -> u64 {
    200
}

fn default_cron_runtime_drain_batch() -> usize {
    8
}

fn default_cron_batch_limit() -> i64 {
    64
}

fn default_sub_agent_max_iterations() -> u32 {
    6
}

fn default_sub_agent_enabled() -> bool {
    true
}

fn default_sub_agent_max_tool_calls() -> u32 {
    12
}

fn default_sub_agent_inherit_parent_tools() -> bool {
    true
}

fn default_sub_agent_exclude_tools() -> Vec<String> {
    vec!["sub_agent".to_string()]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TavilyWebSearchConfig {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub env_key: Option<String>,
    #[serde(default = "default_tavily_search_depth")]
    pub search_depth: String,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub include_answer: Option<bool>,
    #[serde(default)]
    pub include_raw_content: Option<bool>,
    #[serde(default)]
    pub include_images: Option<bool>,
    #[serde(default)]
    pub project_id: Option<String>,
}

fn default_web_search_provider() -> String {
    "tavily".to_string()
}

fn default_web_search_enabled() -> bool {
    true
}

impl Default for TavilyWebSearchConfig {
    fn default() -> Self {
        Self {
            base_url: Some("https://api.tavily.com".to_string()),
            api_key: None,
            env_key: Some("TAVILY_API_KEY".to_string()),
            search_depth: default_tavily_search_depth(),
            topic: None,
            include_answer: None,
            include_raw_content: None,
            include_images: None,
            project_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BraveWebSearchConfig {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub env_key: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub search_lang: Option<String>,
    #[serde(default)]
    pub ui_lang: Option<String>,
    #[serde(default)]
    pub safesearch: Option<String>,
    #[serde(default)]
    pub freshness: Option<String>,
}

impl Default for BraveWebSearchConfig {
    fn default() -> Self {
        Self {
            base_url: Some("https://api.search.brave.com".to_string()),
            api_key: None,
            env_key: Some("BRAVE_SEARCH_API_KEY".to_string()),
            country: None,
            search_lang: None,
            ui_lang: None,
            safesearch: None,
            freshness: None,
        }
    }
}

fn default_tavily_search_depth() -> String {
    "basic".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceEntry {
    pub input_rate: f64,
    pub output_rate: f64,
}

pub type PriceTable = BTreeMap<String, BTreeMap<String, PriceEntry>>;

fn default_price_table() -> PriceTable {
    let mut openai = BTreeMap::new();
    openai.insert(
        "gpt-4.1".to_string(),
        PriceEntry {
            input_rate: 2.0,
            output_rate: 8.0,
        },
    );
    openai.insert(
        "gpt-4.1-mini".to_string(),
        PriceEntry {
            input_rate: 0.4,
            output_rate: 1.6,
        },
    );
    openai.insert(
        "gpt-4o".to_string(),
        PriceEntry {
            input_rate: 2.5,
            output_rate: 10.0,
        },
    );
    openai.insert(
        "gpt-4o-mini".to_string(),
        PriceEntry {
            input_rate: 0.15,
            output_rate: 0.6,
        },
    );
    let mut anthropic = BTreeMap::new();
    anthropic.insert(
        "claude-3-7-sonnet".to_string(),
        PriceEntry {
            input_rate: 3.0,
            output_rate: 15.0,
        },
    );
    anthropic.insert(
        "claude-sonnet-4".to_string(),
        PriceEntry {
            input_rate: 3.0,
            output_rate: 15.0,
        },
    );
    anthropic.insert(
        "claude-opus-4".to_string(),
        PriceEntry {
            input_rate: 15.0,
            output_rate: 75.0,
        },
    );
    let mut table = BTreeMap::new();
    table.insert("openai".to_string(), openai);
    table.insert("anthropic".to_string(), anthropic);
    table
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    #[serde(default = "default_observability_enabled")]
    pub enabled: bool,
    #[serde(default = "default_observability_service_name")]
    pub service_name: String,
    #[serde(default = "default_observability_service_version")]
    pub service_version: String,
    #[serde(default)]
    pub metrics: ObservabilityMetricsConfig,
    #[serde(default)]
    pub traces: ObservabilityTracesConfig,
    #[serde(default)]
    pub otlp: ObservabilityOtlpConfig,
    #[serde(default)]
    pub prometheus: ObservabilityPrometheusConfig,
    #[serde(default)]
    pub audit: ObservabilityAuditConfig,
    #[serde(default)]
    pub local_store: ObservabilityLocalStoreConfig,
    #[serde(default = "default_price_table")]
    pub price: PriceTable,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            enabled: default_observability_enabled(),
            service_name: default_observability_service_name(),
            service_version: default_observability_service_version(),
            metrics: ObservabilityMetricsConfig::default(),
            traces: ObservabilityTracesConfig::default(),
            otlp: ObservabilityOtlpConfig::default(),
            prometheus: ObservabilityPrometheusConfig::default(),
            audit: ObservabilityAuditConfig::default(),
            local_store: ObservabilityLocalStoreConfig::default(),
            price: default_price_table(),
        }
    }
}

fn default_observability_enabled() -> bool {
    false
}

fn default_observability_service_name() -> String {
    "klaw".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilerConfig {
    #[serde(default)]
    pub enabled: bool,
}

impl Default for ProfilerConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

fn default_observability_service_version() -> String {
    "0.1.0".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityMetricsConfig {
    #[serde(default = "default_observability_metrics_enabled")]
    pub enabled: bool,
    #[serde(default = "default_observability_export_interval_seconds")]
    pub export_interval_seconds: u64,
}

impl Default for ObservabilityMetricsConfig {
    fn default() -> Self {
        Self {
            enabled: default_observability_metrics_enabled(),
            export_interval_seconds: default_observability_export_interval_seconds(),
        }
    }
}

fn default_observability_metrics_enabled() -> bool {
    true
}

fn default_observability_export_interval_seconds() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityTracesConfig {
    #[serde(default = "default_observability_traces_enabled")]
    pub enabled: bool,
    #[serde(default = "default_observability_sample_rate")]
    pub sample_rate: f64,
}

impl Default for ObservabilityTracesConfig {
    fn default() -> Self {
        Self {
            enabled: default_observability_traces_enabled(),
            sample_rate: default_observability_sample_rate(),
        }
    }
}

fn default_observability_traces_enabled() -> bool {
    true
}

fn default_observability_sample_rate() -> f64 {
    0.1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityOtlpConfig {
    #[serde(default = "default_observability_otlp_enabled")]
    pub enabled: bool,
    #[serde(default = "default_observability_otlp_endpoint")]
    pub endpoint: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

impl Default for ObservabilityOtlpConfig {
    fn default() -> Self {
        Self {
            enabled: default_observability_otlp_enabled(),
            endpoint: default_observability_otlp_endpoint(),
            headers: BTreeMap::new(),
        }
    }
}

fn default_observability_otlp_enabled() -> bool {
    false
}

fn default_observability_otlp_endpoint() -> String {
    "http://localhost:4317".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityPrometheusConfig {
    #[serde(default = "default_observability_prometheus_enabled")]
    pub enabled: bool,
    #[serde(default = "default_observability_prometheus_listen_port")]
    pub listen_port: u16,
    #[serde(default = "default_observability_prometheus_path")]
    pub path: String,
}

impl Default for ObservabilityPrometheusConfig {
    fn default() -> Self {
        Self {
            enabled: default_observability_prometheus_enabled(),
            listen_port: default_observability_prometheus_listen_port(),
            path: default_observability_prometheus_path(),
        }
    }
}

fn default_observability_prometheus_enabled() -> bool {
    false
}

fn default_observability_prometheus_listen_port() -> u16 {
    9090
}

fn default_observability_prometheus_path() -> String {
    "/metrics".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityAuditConfig {
    #[serde(default = "default_observability_audit_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub output_path: Option<String>,
}

impl Default for ObservabilityAuditConfig {
    fn default() -> Self {
        Self {
            enabled: default_observability_audit_enabled(),
            output_path: None,
        }
    }
}

fn default_observability_audit_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityLocalStoreConfig {
    #[serde(default = "default_observability_local_store_enabled")]
    pub enabled: bool,
    #[serde(default = "default_observability_local_store_retention_days")]
    pub retention_days: u16,
    #[serde(default = "default_observability_local_store_flush_interval_seconds")]
    pub flush_interval_seconds: u64,
}

impl Default for ObservabilityLocalStoreConfig {
    fn default() -> Self {
        Self {
            enabled: default_observability_local_store_enabled(),
            retention_days: default_observability_local_store_retention_days(),
            flush_interval_seconds: default_observability_local_store_flush_interval_seconds(),
        }
    }
}

fn default_observability_local_store_enabled() -> bool {
    true
}

fn default_observability_local_store_retention_days() -> u16 {
    7
}

fn default_observability_local_store_flush_interval_seconds() -> u64 {
    5
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot resolve home directory for default config path")]
    HomeDirUnavailable,
    #[error("config file not found: {0}")]
    ConfigNotFound(PathBuf),
    #[error("failed to create config directory: {0}")]
    CreateDir(#[source] std::io::Error),
    #[error("failed to write config file: {0}")]
    WriteConfig(#[source] std::io::Error),
    #[error("failed to read config file {path}: {source}")]
    ReadConfig {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config file {path}: {source}")]
    ParseConfig {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("failed to serialize config: {0}")]
    SerializeConfig(String),
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}

mod io;
mod validate;

pub use io::{
    ConfigSnapshot, ConfigStore, LoadedConfig, MigratedConfig, default_config_path,
    default_config_template, load_or_init, migrate_with_defaults, reset_to_defaults,
    validate_config_file,
};
#[cfg(test)]
pub(crate) use io::{load_from_path, migrate_path_with_defaults, reset_path_to_defaults};
pub(crate) use validate::validate;

#[cfg(test)]
mod tests;
