use super::*;
use klaw_util::system_timezone_name;
use std::{
    env, fs,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static TEMP_TEST_ROOT_COUNTER: AtomicU64 = AtomicU64::new(1);

fn temp_test_root(prefix: &str) -> std::path::PathBuf {
    let counter = TEMP_TEST_ROOT_COUNTER.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join(format!("{prefix}-{}-{counter}", std::process::id()))
}

#[test]
fn temp_test_root_is_unique_per_call() {
    let first = temp_test_root("klaw-config-store-test");
    let second = temp_test_root("klaw-config-store-test");

    assert_ne!(first, second);
}

#[test]
fn parse_default_template_succeeds() {
    let template = default_config_template();
    let parsed: AppConfig = toml::from_str(&template).expect("default template should parse");
    assert_eq!(parsed.model_provider, "openai");
    assert!(parsed.model.is_none());
    assert_eq!(parsed.conversation_history_limit, 40);
    assert!(parsed.model_providers.contains_key("openai"));
    assert!(!parsed.model_providers["openai"].proxy);
    assert!(!parsed.memory.embedding.enabled);
    assert_eq!(parsed.memory.embedding.provider, "openai");
    assert_eq!(parsed.memory.embedding.model, "text-embedding-3-small");
    assert_eq!(
        parsed.tools.shell.blocked_patterns,
        default_shell_blocked_patterns()
    );
    assert_eq!(
        parsed.tools.shell.unsafe_patterns,
        default_shell_unsafe_patterns()
    );
    assert!(parsed.tools.shell.allow_login_shell);
    assert_eq!(parsed.tools.shell.max_timeout_ms, 120_000);
    assert_eq!(parsed.tools.shell.max_output_bytes, 128 * 1024);
    assert!(parsed.tools.archive.enabled);
    assert!(parsed.tools.channel_attachment.enabled);
    assert!(parsed.tools.voice.enabled);
    assert!(parsed.tools.shell.enabled);
    assert!(parsed.tools.approval.enabled);
    assert!(parsed.tools.ask_question.enabled);
    assert!(!parsed.tools.geo.enabled);
    assert!(parsed.tools.local_search.enabled);
    assert!(parsed.tools.terminal_multiplexers.enabled);
    assert!(parsed.tools.cron_manager.enabled);
    assert!(parsed.tools.heartbeat_manager.enabled);
    assert!(parsed.tools.skills_registry.enabled);
    assert!(parsed.tools.skills_manager.enabled);
    assert_eq!(parsed.heartbeat.defaults.timezone, system_timezone_name());
    assert!(parsed.tools.shell.workspace.is_none());
    assert!(parsed.tools.apply_patch.enabled);
    assert!(parsed.tools.apply_patch.workspace.is_none());
    assert!(!parsed.tools.apply_patch.allow_absolute_paths);
    assert!(parsed.tools.apply_patch.allowed_roots.is_empty());
    assert!(parsed
        .tools
        .channel_attachment
        .local_attachments
        .allowlist
        .is_empty());
    assert_eq!(
        parsed.tools.channel_attachment.local_attachments.max_bytes,
        10 * 1024 * 1024
    );
    assert!(parsed.tools.memory.enabled);
    assert_eq!(parsed.tools.memory.search_limit, 8);
    assert_eq!(parsed.tools.memory.fts_limit, 20);
    assert_eq!(parsed.tools.memory.vector_limit, 20);
    assert!(parsed.tools.memory.use_vector);
    assert!(parsed.tools.web_fetch.enabled);
    assert_eq!(parsed.tools.web_fetch.max_chars, 50_000);
    assert_eq!(parsed.tools.web_fetch.timeout_seconds, 15);
    assert_eq!(parsed.tools.web_fetch.cache_ttl_minutes, 10);
    assert_eq!(parsed.tools.web_fetch.max_redirects, 3);
    assert!(parsed.tools.web_fetch.readability);
    assert!(parsed.tools.web_search.enabled);
    assert_eq!(parsed.tools.web_search.provider, "tavily");
    assert_eq!(
        parsed.tools.web_search.tavily.env_key.as_deref(),
        Some("TAVILY_API_KEY")
    );
    assert_eq!(
        parsed.tools.web_search.brave.env_key.as_deref(),
        Some("BRAVE_SEARCH_API_KEY")
    );
    assert!(parsed.tools.sub_agent.enabled);
    assert_eq!(parsed.tools.sub_agent.max_iterations, 6);
    assert_eq!(parsed.tools.sub_agent.max_tool_calls, 12);
    assert_eq!(parsed.skills.sync_timeout, 60);
    assert!(!parsed.skills.registries.is_empty());
    assert_eq!(
        parsed
            .skills
            .registries
            .get("anthropic")
            .map(|registry| registry.address.as_str()),
        Some("https://github.com/anthropics/skills")
    );
    assert!(parsed
        .skills
        .registries
        .get("anthropic")
        .is_some_and(|registry| registry.installed.is_empty()));
    assert_eq!(parsed.cron.tick_ms, 1_000);
    assert_eq!(parsed.cron.runtime_tick_ms, 200);
    assert_eq!(parsed.cron.runtime_drain_batch, 8);
    assert_eq!(parsed.cron.batch_limit, 64);
    assert_eq!(
        parsed.cron.missed_run_policy,
        crate::CronMissedRunPolicy::Skip
    );
    assert!(parsed.heartbeat.defaults.enabled);
    assert_eq!(parsed.heartbeat.defaults.every, "30m");
    assert_eq!(parsed.heartbeat.defaults.silent_ack_token, "HEARTBEAT_OK");
    assert!(parsed.heartbeat.sessions.is_empty());
    assert!(parsed.channels.dingtalk.is_empty());
    assert!(parsed.channels.telegram.is_empty());
    assert!(!parsed.voice.enabled);
    assert_eq!(parsed.voice.stt_provider.as_str(), "deepgram");
    assert_eq!(parsed.voice.tts_provider.as_str(), "elevenlabs");
    assert_eq!(parsed.voice.default_language, "zh-CN");
    assert_eq!(
        parsed.voice.providers.deepgram.api_key_env,
        "DEEPGRAM_API_KEY".to_string()
    );
    assert_eq!(
        parsed.voice.providers.assemblyai.api_key_env,
        "ASSEMBLYAI_API_KEY".to_string()
    );
    assert_eq!(
        parsed.voice.providers.elevenlabs.api_key_env,
        "ELEVENLABS_API_KEY".to_string()
    );
    assert_eq!(
        parsed.tools.sub_agent.exclude_tools,
        vec!["sub_agent".to_string()]
    );
    assert_eq!(parsed.mcp.startup_timeout_seconds, 60);
    assert!(parsed.mcp.servers.is_empty());
    assert!(!parsed.gateway.enabled);
    assert_eq!(parsed.gateway.listen_ip, "127.0.0.1");
    assert_eq!(parsed.gateway.listen_port, 0);
    assert!(!parsed.gateway.tls.enabled);
    assert!(parsed.gateway.tls.cert_path.is_none());
    assert!(parsed.gateway.tls.key_path.is_none());
    assert!(!parsed.gateway.webhook.enabled);
    assert!(parsed.gateway.webhook.events.enabled);
    assert_eq!(parsed.gateway.webhook.events.max_body_bytes, 262_144);
    assert!(!parsed.gateway.webhook.agents.enabled);
    assert_eq!(parsed.gateway.webhook.agents.max_body_bytes, 262_144);
    validate(&parsed).expect("default template should be valid");
}

#[test]
fn parse_cron_missed_run_policy_override() {
    let parsed: CronConfig =
        toml::from_str("missed_run_policy = \"catch_up\"").expect("cron config should parse");
    assert_eq!(parsed.missed_run_policy, CronMissedRunPolicy::CatchUp);
    assert_eq!(parsed.tick_ms, 1_000);
}

#[test]
fn parse_conversation_history_limit_succeeds() {
    let raw = r#"
model_provider = "openai"
conversation_history_limit = 12

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
    assert_eq!(parsed.conversation_history_limit, 12);
}

#[test]
fn parse_model_provider_proxy_succeeds() {
    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
proxy = true
env_key = "OPENAI_API_KEY"
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
    assert!(parsed.model_providers["openai"].proxy);
}

#[test]
fn parse_voice_config_succeeds() {
    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[voice]
enabled = true
stt_provider = "assemblyai"
tts_provider = "elevenlabs"
default_language = "en-US"
default_voice_id = "voice-42"

[voice.providers.deepgram]
api_key = "dg-key"
base_url = "https://api.deepgram.com"
streaming_base_url = "wss://api.deepgram.com"
stt_model = "nova-2"

[voice.providers.assemblyai]
api_key = "aa-key"
base_url = "https://api.assemblyai.com"
streaming_base_url = "wss://streaming.assemblyai.com"
stt_model = "universal"

[voice.providers.elevenlabs]
api_key = "el-key"
base_url = "https://api.elevenlabs.io"
streaming_base_url = "wss://api.elevenlabs.io"
default_model = "eleven_multilingual_v2"
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("voice config should parse");
    assert!(parsed.voice.enabled);
    assert_eq!(parsed.voice.stt_provider.as_str(), "assemblyai");
    assert_eq!(parsed.voice.tts_provider.as_str(), "elevenlabs");
    assert_eq!(parsed.voice.default_language, "en-US");
    assert_eq!(parsed.voice.default_voice_id.as_deref(), Some("voice-42"));
    assert_eq!(
        parsed.voice.providers.deepgram.api_key.as_deref(),
        Some("dg-key")
    );
}

#[test]
fn validate_voice_enabled_requires_selected_provider_keys() {
    let cfg = AppConfig {
        voice: VoiceConfig {
            enabled: true,
            ..VoiceConfig::default()
        },
        ..Default::default()
    };
    let err = validate(&cfg).expect_err("voice config should require provider keys");
    assert!(format!("{err}").contains("voice.providers.deepgram requires api_key or api_key_env"));
}

#[test]
fn parse_legacy_root_model_field_succeeds() {
    let raw = r#"
model_provider = "openai"
model = "gpt-4.1-mini"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
    assert_eq!(parsed.model.as_deref(), Some("gpt-4.1-mini"));
}

#[test]
fn parse_storage_root_dir_succeeds() {
    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[storage]
root_dir = "/tmp/klaw-data"
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
    assert_eq!(parsed.storage.root_dir.as_deref(), Some("/tmp/klaw-data"));
}

#[test]
fn validate_fails_when_active_provider_missing() {
    let cfg = AppConfig {
        model_provider: "missing".to_string(),
        model_providers: BTreeMap::new(),
        ..Default::default()
    };
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("not found in model_providers"));
}

#[test]
fn validate_fails_when_root_model_is_blank() {
    let cfg = AppConfig {
        model: Some("   ".to_string()),
        ..Default::default()
    };
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("model cannot be empty when configured"));
}

#[test]
fn parse_tools_shell_unsafe_patterns_succeeds() {
    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[tools.shell]
enabled = true
workspace = "/Users/example/shell"
blocked_patterns = [":(){ :|:& };:"]
unsafe_patterns = ["sudo rm -rf /tmp/example"]
allow_login_shell = false
max_timeout_ms = 30000
max_output_bytes = 64000
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
    assert_eq!(
        parsed.tools.shell.blocked_patterns,
        vec![":(){ :|:& };:".to_string()]
    );
    assert_eq!(
        parsed.tools.shell.unsafe_patterns,
        vec!["sudo rm -rf /tmp/example".to_string()]
    );
    assert_eq!(
        parsed.tools.shell.workspace.as_deref(),
        Some("/Users/example/shell")
    );
    assert!(parsed.tools.shell.enabled);
    assert!(!parsed.tools.shell.allow_login_shell);
    assert_eq!(parsed.tools.shell.max_timeout_ms, 30_000);
    assert_eq!(parsed.tools.shell.max_output_bytes, 64_000);
}

#[test]
fn parse_tools_workspaces_succeeds() {
    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[tools.shell]
workspace = "/Users/example/shell"

[tools.apply_patch]
workspace = "/Users/example/patch"
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
    assert_eq!(
        parsed.tools.shell.workspace.as_deref(),
        Some("/Users/example/shell")
    );
    assert_eq!(
        parsed.tools.apply_patch.workspace.as_deref(),
        Some("/Users/example/patch")
    );
}

#[test]
fn parse_tools_apply_patch_succeeds() {
    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[tools.apply_patch]
enabled = true
workspace = "/Users/example/patch"
allow_absolute_paths = true
allowed_roots = ["/tmp", "sandbox/allowed"]
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
    assert_eq!(
        parsed.tools.apply_patch.workspace.as_deref(),
        Some("/Users/example/patch")
    );
    assert!(parsed.tools.apply_patch.enabled);
    assert!(parsed.tools.apply_patch.allow_absolute_paths);
    assert_eq!(
        parsed.tools.apply_patch.allowed_roots,
        vec!["/tmp".to_string(), "sandbox/allowed".to_string()]
    );
}

#[test]
fn parse_tools_web_search_succeeds() {
    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[tools.web_search]
enabled = false
provider = "tavily"

[tools.web_search.tavily]
base_url = "https://api.tavily.com"
env_key = "TAVILY_API_KEY"
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
    assert_eq!(parsed.tools.web_search.provider, "tavily");
    assert!(!parsed.tools.web_search.enabled);
    assert_eq!(
        parsed.tools.web_search.tavily.base_url.as_deref(),
        Some("https://api.tavily.com")
    );
}

#[test]
fn parse_tools_web_fetch_succeeds() {
    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[tools.web_fetch]
enabled = true
max_chars = 12000
timeout_seconds = 20
cache_ttl_minutes = 30
max_redirects = 2
readability = true
ssrf_allowlist = ["172.22.0.0/16"]
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
    assert!(parsed.tools.web_fetch.enabled);
    assert_eq!(parsed.tools.web_fetch.max_chars, 12000);
    assert_eq!(parsed.tools.web_fetch.timeout_seconds, 20);
    assert_eq!(parsed.tools.web_fetch.cache_ttl_minutes, 30);
    assert_eq!(parsed.tools.web_fetch.max_redirects, 2);
    assert!(parsed.tools.web_fetch.readability);
    assert_eq!(
        parsed.tools.web_fetch.ssrf_allowlist,
        vec!["172.22.0.0/16".to_string()]
    );
}

#[test]
fn parse_mcp_servers_succeeds() {
    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[mcp]
enabled = true
startup_timeout_seconds = 30

[[mcp.servers]]
id = "filesystem"
enabled = true
mode = "stdio"
tool_timeout_seconds = 45
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
cwd = "/tmp"

[mcp.servers.env]
NODE_ENV = "production"

[[mcp.servers]]
id = "remote"
enabled = true
mode = "sse"
url = "https://mcp.example.com/sse"

[mcp.servers.headers]
Authorization = "Bearer test"
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
    assert_eq!(parsed.mcp.startup_timeout_seconds, 30);
    assert_eq!(parsed.mcp.servers.len(), 2);
    assert_eq!(parsed.mcp.servers[0].mode, McpServerMode::Stdio);
    assert_eq!(parsed.mcp.servers[0].tool_timeout_seconds, 45);
    assert_eq!(parsed.mcp.servers[0].command.as_deref(), Some("npx"),);
    assert_eq!(parsed.mcp.servers[1].tool_timeout_seconds, 60);
    assert_eq!(
        parsed.mcp.servers[1].url.as_deref(),
        Some("https://mcp.example.com/sse")
    );
    assert_eq!(
        parsed.mcp.servers[1].headers.get("Authorization"),
        Some(&"Bearer test".to_string())
    );
}

#[test]
fn parse_gateway_config_succeeds() {
    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[gateway]
enabled = true
listen_ip = "0.0.0.0"
listen_port = 18080

[gateway.webhook]
enabled = true
path = "/hooks/events"
max_body_bytes = 4096

[gateway.tls]
enabled = false
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
    assert!(parsed.gateway.enabled);
    assert_eq!(parsed.gateway.listen_ip, "0.0.0.0");
    assert_eq!(parsed.gateway.listen_port, 18_080);
    assert!(parsed.gateway.webhook.enabled);
    assert!(parsed.gateway.webhook.events.enabled);
    assert_eq!(parsed.gateway.webhook.events.max_body_bytes, 4096);
    assert!(!parsed.gateway.webhook.agents.enabled);
    assert!(!parsed.gateway.tls.enabled);
}

#[test]
fn parse_gateway_dual_webhook_config_succeeds() {
    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[gateway.webhook]
enabled = true

[gateway.webhook.events]
enabled = true
path = "/hooks/events"
max_body_bytes = 4096

[gateway.webhook.agents]
enabled = true
path = "/hooks/agents"
max_body_bytes = 8192
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("dual webhook config should parse");
    assert!(parsed.gateway.webhook.enabled);
    assert!(parsed.gateway.webhook.events.enabled);
    assert_eq!(parsed.gateway.webhook.events.max_body_bytes, 4096);
    assert!(parsed.gateway.webhook.agents.enabled);
    assert_eq!(parsed.gateway.webhook.agents.max_body_bytes, 8192);
}

#[test]
fn parse_heartbeat_config_succeeds() {
    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[heartbeat.defaults]
enabled = true
every = "45m"
prompt = "Review session."
silent_ack_token = "HEARTBEAT_OK"
timezone = "Asia/Shanghai"

[[heartbeat.sessions]]
session_key = "terminal:main"
chat_id = "main"
channel = "terminal"
enabled = true
every = "10m"
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("heartbeat config should parse");
    assert_eq!(parsed.heartbeat.defaults.every, "45m");
    assert_eq!(parsed.heartbeat.defaults.timezone, "Asia/Shanghai");
    assert_eq!(parsed.heartbeat.sessions.len(), 1);
    assert_eq!(parsed.heartbeat.sessions[0].session_key, "terminal:main");
    assert_eq!(parsed.heartbeat.sessions[0].every.as_deref(), Some("10m"));
}

#[test]
fn parse_dingtalk_channel_config_succeeds() {
    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[[channels.dingtalk]]
id = "ops"
enabled = true
client_id = "ding-client"
client_secret = "ding-secret"
bot_title = "Ops Bot"
show_reasoning = true
allowlist = ["u123", "u456"]
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("dingtalk config should parse");
    assert_eq!(parsed.channels.dingtalk.len(), 1);
    let account = &parsed.channels.dingtalk[0];
    assert_eq!(account.id, "ops");
    assert!(account.enabled);
    assert_eq!(account.client_id, "ding-client");
    assert_eq!(account.bot_title, "Ops Bot");
    assert!(account.show_reasoning);
    assert_eq!(
        account.allowlist,
        vec!["u123".to_string(), "u456".to_string()]
    );
    assert!(!account.proxy.enabled);
    assert!(account.proxy.url.is_empty());
}

#[test]
fn parse_telegram_channel_config_succeeds() {
    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[[channels.telegram]]
id = "ops"
enabled = true
bot_token = "123456:secret"
show_reasoning = true
allowlist = ["u123", "*"]
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("telegram config should parse");
    assert_eq!(parsed.channels.telegram.len(), 1);
    let account = &parsed.channels.telegram[0];
    assert_eq!(account.id, "ops");
    assert!(account.enabled);
    assert_eq!(account.bot_token, "123456:secret");
    assert!(account.show_reasoning);
    assert_eq!(account.allowlist, vec!["u123".to_string(), "*".to_string()]);
    assert!(!account.proxy.enabled);
    assert!(account.proxy.url.is_empty());
}

#[test]
fn validate_fails_when_web_fetch_limits_are_invalid() {
    let mut cfg = AppConfig::default();
    cfg.tools.web_fetch.enabled = true;
    cfg.tools.web_fetch.max_chars = 0;
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("tools.web_fetch.max_chars"));

    let mut cfg2 = AppConfig::default();
    cfg2.tools.web_fetch.enabled = true;
    cfg2.tools.web_fetch.timeout_seconds = 0;
    let err2 = validate(&cfg2).expect_err("should fail");
    assert!(format!("{err2}").contains("tools.web_fetch.timeout_seconds"));
}

#[test]
fn validate_fails_when_mcp_timeout_is_zero() {
    let mut cfg = AppConfig::default();
    cfg.mcp.startup_timeout_seconds = 0;
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("mcp.startup_timeout_seconds"));
}

#[test]
fn validate_fails_when_mcp_server_tool_timeout_is_zero() {
    let mut cfg = AppConfig::default();
    cfg.mcp.servers = vec![McpServerConfig {
        id: "filesystem".to_string(),
        enabled: true,
        mode: McpServerMode::Stdio,
        command: Some("npx".to_string()),
        args: Vec::new(),
        env: BTreeMap::new(),
        cwd: None,
        url: None,
        headers: BTreeMap::new(),
        tool_timeout_seconds: 0,
    }];

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("tool_timeout_seconds"));
}

#[test]
fn validate_fails_when_gateway_ip_is_invalid() {
    let mut cfg = AppConfig::default();
    cfg.gateway.listen_ip = "invalid-ip".to_string();
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("gateway.listen_ip"));
}

#[test]
fn validate_accepts_gateway_random_port() {
    let mut cfg = AppConfig::default();
    cfg.gateway.listen_port = 0;
    validate(&cfg).expect("random port should be valid");
}

#[test]
fn validate_fails_when_gateway_webhook_max_body_bytes_zero() {
    let mut cfg = AppConfig::default();
    cfg.gateway.webhook.agents.max_body_bytes = 0;
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("gateway.webhook.agents.max_body_bytes"));
}

#[test]
fn parse_gateway_webhook_path_config_is_ignored() {
    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[gateway.webhook]
enabled = true

[gateway.webhook.events]
enabled = true
path = "/ignored/events"
max_body_bytes = 4096

[gateway.webhook.agents]
enabled = true
path = "/ignored/agents"
max_body_bytes = 8192
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("webhook path config should parse");
    assert_eq!(parsed.gateway.webhook.events.max_body_bytes, 4096);
    assert_eq!(parsed.gateway.webhook.agents.max_body_bytes, 8192);
    validate(&parsed).expect("ignored webhook paths should not affect validation");
}

#[test]
fn validate_fails_when_gateway_tls_paths_missing() {
    let mut cfg = AppConfig::default();
    cfg.gateway.tls.enabled = true;
    cfg.gateway.tls.cert_path = Some("".to_string());
    cfg.gateway.tls.key_path = None;
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("gateway.tls.cert_path"));
}

#[test]
fn validate_fails_when_dingtalk_channel_ids_duplicate() {
    let mut cfg = AppConfig::default();
    cfg.channels.dingtalk = vec![
        DingtalkConfig {
            id: "ops".to_string(),
            enabled: true,
            client_id: "client-a".to_string(),
            client_secret: "secret-a".to_string(),
            bot_title: "Ops".to_string(),
            show_reasoning: false,
            stream_output: false,
            allowlist: vec![],
            proxy: DingtalkProxyConfig::default(),
        },
        DingtalkConfig {
            id: "ops".to_string(),
            enabled: true,
            client_id: "client-b".to_string(),
            client_secret: "secret-b".to_string(),
            bot_title: "Ops2".to_string(),
            show_reasoning: false,
            stream_output: false,
            allowlist: vec![],
            proxy: DingtalkProxyConfig::default(),
        },
    ];

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("duplicated id"));
}

#[test]
fn validate_fails_when_enabled_dingtalk_channel_missing_secret() {
    let mut cfg = AppConfig::default();
    cfg.channels.dingtalk = vec![DingtalkConfig {
        id: "ops".to_string(),
        enabled: true,
        client_id: "client-a".to_string(),
        client_secret: String::new(),
        bot_title: "Ops".to_string(),
        show_reasoning: false,
        stream_output: false,
        allowlist: vec![],
        proxy: DingtalkProxyConfig::default(),
    }];

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("channels.dingtalk.client_secret"));
}

#[test]
fn validate_fails_when_enabled_dingtalk_proxy_missing_url() {
    let mut cfg = AppConfig::default();
    cfg.channels.dingtalk = vec![DingtalkConfig {
        id: "ops".to_string(),
        enabled: true,
        client_id: "client-a".to_string(),
        client_secret: "secret-a".to_string(),
        bot_title: "Ops".to_string(),
        show_reasoning: false,
        stream_output: false,
        allowlist: vec![],
        proxy: DingtalkProxyConfig {
            enabled: true,
            url: String::new(),
        },
    }];

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("channels.dingtalk.proxy.url"));
}

#[test]
fn validate_fails_when_enabled_dingtalk_proxy_has_invalid_scheme() {
    let mut cfg = AppConfig::default();
    cfg.channels.dingtalk = vec![DingtalkConfig {
        id: "ops".to_string(),
        enabled: true,
        client_id: "client-a".to_string(),
        client_secret: "secret-a".to_string(),
        bot_title: "Ops".to_string(),
        show_reasoning: false,
        stream_output: false,
        allowlist: vec![],
        proxy: DingtalkProxyConfig {
            enabled: true,
            url: "socks5://127.0.0.1:1080".to_string(),
        },
    }];

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("proxy url scheme must be http or https"));
}

#[test]
fn validate_fails_when_dingtalk_local_attachment_allowlist_is_relative() {
    let mut cfg = AppConfig::default();
    cfg.channels.dingtalk = vec![DingtalkConfig {
        id: "ops".to_string(),
        enabled: true,
        client_id: "client-a".to_string(),
        client_secret: "secret-a".to_string(),
        bot_title: "Ops".to_string(),
        show_reasoning: false,
        stream_output: false,
        allowlist: vec![],
        proxy: DingtalkProxyConfig::default(),
    }];
    cfg.tools.channel_attachment.local_attachments = crate::LocalAttachmentConfig {
        allowlist: vec!["relative/path".to_string()],
        max_bytes: 1024,
    };

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("must be an absolute path"));
}

#[test]
fn validate_fails_when_telegram_channel_ids_duplicate() {
    let mut cfg = AppConfig::default();
    cfg.channels.telegram = vec![
        TelegramConfig {
            id: "ops".to_string(),
            enabled: true,
            bot_token: "token-a".to_string(),
            show_reasoning: false,
            stream_output: false,
            allowlist: vec![],
            proxy: TelegramProxyConfig::default(),
        },
        TelegramConfig {
            id: "ops".to_string(),
            enabled: true,
            bot_token: "token-b".to_string(),
            show_reasoning: false,
            stream_output: false,
            allowlist: vec![],
            proxy: TelegramProxyConfig::default(),
        },
    ];

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("channels.telegram contains duplicated id"));
}

#[test]
fn validate_fails_when_websocket_channel_ids_duplicate() {
    let mut cfg = AppConfig::default();
    cfg.channels.websocket = vec![
        WebsocketConfig {
            id: "browser".to_string(),
            enabled: true,
            show_reasoning: false,
            stream_output: true,
        },
        WebsocketConfig {
            id: "browser".to_string(),
            enabled: true,
            show_reasoning: true,
            stream_output: false,
        },
    ];

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("channels.websocket contains duplicated id"));
}

#[test]
fn validate_fails_when_websocket_channel_id_is_blank() {
    let mut cfg = AppConfig::default();
    cfg.channels.websocket = vec![WebsocketConfig {
        id: "   ".to_string(),
        enabled: true,
        show_reasoning: false,
        stream_output: true,
    }];

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("channels.websocket.id"));
}

#[test]
fn validate_fails_when_enabled_telegram_channel_missing_token() {
    let mut cfg = AppConfig::default();
    cfg.channels.telegram = vec![TelegramConfig {
        id: "ops".to_string(),
        enabled: true,
        bot_token: String::new(),
        show_reasoning: false,
        stream_output: false,
        allowlist: vec![],
        proxy: TelegramProxyConfig::default(),
    }];

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("channels.telegram.bot_token"));
}

#[test]
fn validate_fails_when_enabled_telegram_proxy_missing_url() {
    let mut cfg = AppConfig::default();
    cfg.channels.telegram = vec![TelegramConfig {
        id: "ops".to_string(),
        enabled: true,
        bot_token: "token-a".to_string(),
        show_reasoning: false,
        stream_output: false,
        allowlist: vec![],
        proxy: TelegramProxyConfig {
            enabled: true,
            url: String::new(),
        },
    }];

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("channels.telegram.proxy.url"));
}

#[test]
fn validate_fails_when_enabled_telegram_proxy_has_invalid_scheme() {
    let mut cfg = AppConfig::default();
    cfg.channels.telegram = vec![TelegramConfig {
        id: "ops".to_string(),
        enabled: true,
        bot_token: "token-a".to_string(),
        show_reasoning: false,
        stream_output: false,
        allowlist: vec![],
        proxy: TelegramProxyConfig {
            enabled: true,
            url: "socks5://127.0.0.1:1080".to_string(),
        },
    }];

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("proxy url scheme must be http or https"));
}

#[test]
fn validate_fails_when_telegram_local_attachment_max_bytes_is_zero() {
    let mut cfg = AppConfig::default();
    cfg.channels.telegram = vec![TelegramConfig {
        id: "ops".to_string(),
        enabled: true,
        bot_token: "token-a".to_string(),
        show_reasoning: false,
        stream_output: false,
        allowlist: vec![],
        proxy: TelegramProxyConfig::default(),
    }];
    cfg.tools.channel_attachment.local_attachments = crate::LocalAttachmentConfig {
        allowlist: vec![],
        max_bytes: 0,
    };

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("max_bytes must be greater than 0"));
}

#[test]
fn validate_fails_when_mcp_server_ids_duplicate() {
    let mut cfg = AppConfig::default();
    cfg.mcp.servers = vec![
        McpServerConfig {
            id: "dup".to_string(),
            enabled: true,
            mode: McpServerMode::Stdio,
            tool_timeout_seconds: 60,
            command: Some("echo".to_string()),
            args: vec![],
            env: BTreeMap::new(),
            cwd: None,
            url: None,
            headers: BTreeMap::new(),
        },
        McpServerConfig {
            id: "dup".to_string(),
            enabled: true,
            mode: McpServerMode::Sse,
            tool_timeout_seconds: 60,
            command: None,
            args: vec![],
            env: BTreeMap::new(),
            cwd: None,
            url: Some("https://example.com/sse".to_string()),
            headers: BTreeMap::new(),
        },
    ];
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("duplicated id"));
}

#[test]
fn validate_fails_when_stdio_server_command_missing() {
    let mut cfg = AppConfig::default();
    cfg.mcp.servers = vec![McpServerConfig {
        id: "stdio".to_string(),
        enabled: true,
        mode: McpServerMode::Stdio,
        tool_timeout_seconds: 60,
        command: Some("".to_string()),
        args: vec![],
        env: BTreeMap::new(),
        cwd: None,
        url: None,
        headers: BTreeMap::new(),
    }];
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("mode=stdio"));
}

#[test]
fn validate_fails_when_sse_server_url_invalid() {
    let mut cfg = AppConfig::default();
    cfg.mcp.servers = vec![McpServerConfig {
        id: "sse".to_string(),
        enabled: true,
        mode: McpServerMode::Sse,
        tool_timeout_seconds: 60,
        command: None,
        args: vec![],
        env: BTreeMap::new(),
        cwd: None,
        url: Some("ftp://example.com/sse".to_string()),
        headers: BTreeMap::new(),
    }];
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("http or https"));
}

#[test]
fn validate_fails_when_web_search_provider_missing() {
    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[tools.web_search]
enabled = true
provider = "missing"
"#;
    let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
    let err = validate(&parsed).expect_err("should fail");
    assert!(format!("{err}").contains("is not supported"));
}

#[test]
fn validate_fails_when_sub_agent_limits_are_zero() {
    let mut cfg = AppConfig::default();
    cfg.tools.sub_agent.max_iterations = 0;
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("max_iterations"));

    let mut cfg2 = AppConfig::default();
    cfg2.tools.sub_agent.max_tool_calls = 0;
    let err2 = validate(&cfg2).expect_err("should fail");
    assert!(format!("{err2}").contains("max_tool_calls"));
}

#[test]
fn validate_allows_invalid_sub_agent_limits_when_disabled() {
    let mut cfg = AppConfig::default();
    cfg.tools.sub_agent.enabled = false;
    cfg.tools.sub_agent.max_iterations = 0;
    cfg.tools.sub_agent.max_tool_calls = 0;
    validate(&cfg).expect("should be valid when sub_agent is disabled");
}

#[test]
fn validate_fails_when_apply_patch_allowed_root_is_empty() {
    let mut cfg = AppConfig::default();
    cfg.tools.apply_patch.allowed_roots = vec![" ".to_string()];

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("tools.apply_patch.allowed_roots"));
}

#[test]
fn validate_allows_invalid_apply_patch_config_when_disabled() {
    let mut cfg = AppConfig::default();
    cfg.tools.apply_patch.enabled = false;
    cfg.tools.apply_patch.workspace = Some(" ".to_string());
    cfg.tools.apply_patch.allowed_roots = vec![" ".to_string()];
    validate(&cfg).expect("should be valid when apply_patch is disabled");
}

#[test]
fn validate_fails_when_apply_patch_workspace_is_empty() {
    let mut cfg = AppConfig::default();
    cfg.tools.apply_patch.workspace = Some(" ".to_string());

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("tools.apply_patch.workspace"));
}

#[test]
fn validate_fails_when_shell_workspace_is_empty() {
    let mut cfg = AppConfig::default();
    cfg.tools.shell.workspace = Some(" ".to_string());

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("tools.shell.workspace"));
}

#[test]
fn validate_allows_invalid_shell_config_when_disabled() {
    let mut cfg = AppConfig::default();
    cfg.tools.shell.enabled = false;
    cfg.tools.shell.workspace = Some(" ".to_string());
    cfg.tools.shell.max_timeout_ms = 0;
    cfg.tools.shell.max_output_bytes = 0;
    validate(&cfg).expect("should be valid when shell is disabled");
}

#[test]
fn validate_fails_when_storage_root_dir_is_empty() {
    let mut cfg = AppConfig::default();
    cfg.storage.root_dir = Some("   ".to_string());

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("storage.root_dir"));
}

#[test]
fn validate_fails_when_observability_sample_rate_out_of_range() {
    let mut cfg = AppConfig::default();
    cfg.observability.traces.enabled = true;
    cfg.observability.traces.sample_rate = 1.5;
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("observability.traces.sample_rate"));
}

#[test]
fn validate_fails_when_observability_otlp_endpoint_invalid() {
    let mut cfg = AppConfig::default();
    cfg.observability.otlp.enabled = true;
    cfg.observability.otlp.endpoint = "localhost:4317".to_string();
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("observability.otlp.endpoint"));
}

#[test]
fn validate_fails_when_observability_prometheus_path_invalid() {
    let mut cfg = AppConfig::default();
    cfg.observability.prometheus.enabled = true;
    cfg.observability.prometheus.path = "metrics".to_string();
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("observability.prometheus.path"));
}

#[test]
fn validate_fails_when_observability_local_store_retention_is_zero() {
    let mut cfg = AppConfig::default();
    cfg.observability.local_store.enabled = true;
    cfg.observability.local_store.retention_days = 0;
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("observability.local_store.retention_days"));
}

#[test]
fn validate_fails_when_observability_local_store_flush_interval_is_zero() {
    let mut cfg = AppConfig::default();
    cfg.observability.local_store.enabled = true;
    cfg.observability.local_store.flush_interval_seconds = 0;
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("observability.local_store.flush_interval_seconds"));
}

#[test]
fn validate_fails_when_skills_config_invalid() {
    let mut cfg = AppConfig::default();
    cfg.skills.sync_timeout = 0;
    let err0 = validate(&cfg).expect_err("should fail");
    assert!(format!("{err0}").contains("skills.sync_timeout"));

    let mut cfg = AppConfig::default();
    cfg.skills.registries.insert(
        String::new(),
        SkillsRegistryConfig {
            address: "https://github.com/anthropics/skills".to_string(),
            installed: vec![],
        },
    );
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("skills.<registry>.address"));

    let mut cfg2 = AppConfig::default();
    cfg2.skills.registries.insert(
        "empty-address".to_string(),
        SkillsRegistryConfig {
            address: String::new(),
            installed: vec![],
        },
    );
    let err2 = validate(&cfg2).expect_err("should fail");
    assert!(format!("{err2}").contains("skills.<registry>.address"));

    let mut cfg3 = AppConfig::default();
    cfg3.skills.registries.insert(
        "anthropic".to_string(),
        SkillsRegistryConfig {
            address: "https://github.com/anthropics/skills".to_string(),
            installed: vec!["".to_string()],
        },
    );
    let err3 = validate(&cfg3).expect_err("should fail");
    assert!(format!("{err3}").contains("empty skill name"));

    let mut cfg4 = AppConfig::default();
    cfg4.skills.registries.insert(
        "anthropic".to_string(),
        SkillsRegistryConfig {
            address: "https://github.com/anthropics/skills".to_string(),
            installed: vec!["code-review".to_string(), "code-review".to_string()],
        },
    );
    let err4 = validate(&cfg4).expect_err("should fail");
    assert!(format!("{err4}").contains("duplicated skill"));
}

#[test]
fn validate_fails_when_cron_limits_are_zero() {
    let mut cfg = AppConfig::default();
    cfg.cron.tick_ms = 0;
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("cron.tick_ms"));

    let mut cfg2 = AppConfig::default();
    cfg2.cron.runtime_tick_ms = 0;
    let err2 = validate(&cfg2).expect_err("should fail");
    assert!(format!("{err2}").contains("cron.runtime_tick_ms"));

    let mut cfg3 = AppConfig::default();
    cfg3.cron.runtime_drain_batch = 0;
    let err3 = validate(&cfg3).expect_err("should fail");
    assert!(format!("{err3}").contains("cron.runtime_drain_batch"));

    let mut cfg4 = AppConfig::default();
    cfg4.cron.batch_limit = 0;
    let err4 = validate(&cfg4).expect_err("should fail");
    assert!(format!("{err4}").contains("cron.batch_limit"));
}

#[test]
fn validate_fails_when_memory_tool_limits_are_zero() {
    let mut cfg = AppConfig::default();
    cfg.tools.memory.search_limit = 0;
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("tools.memory.search_limit"));

    let mut cfg2 = AppConfig::default();
    cfg2.tools.memory.fts_limit = 0;
    let err2 = validate(&cfg2).expect_err("should fail");
    assert!(format!("{err2}").contains("tools.memory.fts_limit"));

    let mut cfg3 = AppConfig::default();
    cfg3.tools.memory.vector_limit = 0;
    let err3 = validate(&cfg3).expect_err("should fail");
    assert!(format!("{err3}").contains("tools.memory.vector_limit"));
}

#[test]
fn validate_allows_invalid_memory_tool_limits_when_disabled() {
    let mut cfg = AppConfig::default();
    cfg.tools.memory.enabled = false;
    cfg.tools.memory.search_limit = 0;
    cfg.tools.memory.fts_limit = 0;
    cfg.tools.memory.vector_limit = 0;
    validate(&cfg).expect("should be valid when memory tool is disabled");
}

#[test]
fn validate_fails_when_shell_limits_are_invalid() {
    let mut cfg = AppConfig::default();
    cfg.tools.shell.max_timeout_ms = 0;
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("tools.shell.max_timeout_ms"));

    let mut cfg2 = AppConfig::default();
    cfg2.tools.shell.max_output_bytes = 0;
    let err2 = validate(&cfg2).expect_err("should fail");
    assert!(format!("{err2}").contains("tools.shell.max_output_bytes"));

    let mut cfg3 = AppConfig::default();
    cfg3.tools.shell.workspace = Some(" ".to_string());
    let err3 = validate(&cfg3).expect_err("should fail");
    assert!(format!("{err3}").contains("tools.shell.workspace"));
}

#[test]
fn validate_fails_when_memory_embedding_provider_missing() {
    let mut cfg = AppConfig::default();
    cfg.memory.embedding.enabled = true;
    cfg.memory.embedding.provider = String::new();
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("memory.embedding.provider cannot be empty when"));
}

#[test]
fn validate_fails_when_memory_embedding_model_missing() {
    let mut cfg = AppConfig::default();
    cfg.memory.embedding.enabled = true;
    cfg.memory.embedding.model = String::new();
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("memory.embedding.model cannot be empty when"));
}

#[test]
fn validate_fails_when_memory_embedding_provider_not_found() {
    let mut cfg = AppConfig::default();
    cfg.memory.embedding.enabled = true;
    cfg.memory.embedding.provider = "missing".to_string();
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("not found in model_providers"));
}

#[test]
fn validate_allows_missing_embedding_provider_when_disabled() {
    let mut cfg = AppConfig::default();
    cfg.memory.embedding.enabled = false;
    cfg.memory.embedding.provider = String::new();
    cfg.memory.embedding.model = String::new();
    validate(&cfg).expect("should be valid when embedding disabled");
}

#[test]
fn load_from_path_creates_default_and_reloads() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let root = env::temp_dir().join(format!("klaw-config-test-{suffix}"));
    let path = root.join("config.toml");

    let loaded = load_from_path(&path, true).expect("should create and load");
    assert!(loaded.created_default);
    assert!(path.exists());

    let loaded2 = load_from_path(&path, false).expect("should reload");
    assert!(!loaded2.created_default);

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn migrate_path_with_defaults_creates_file_when_missing() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let root = env::temp_dir().join(format!("klaw-config-migrate-test-{suffix}"));
    let path = root.join("config.toml");

    let migrated = migrate_path_with_defaults(&path).expect("should create and migrate");
    assert!(migrated.created_file);
    assert!(path.exists());

    let loaded = load_from_path(&path, false).expect("migrated file should load");
    assert_eq!(loaded.config.model_provider, "openai");

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn migrate_path_with_defaults_merges_existing_and_preserves_unknown_keys() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let root = env::temp_dir().join(format!("klaw-config-migrate-test-{suffix}"));
    let path = root.join("config.toml");

    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4.1-mini"
env_key = "OPENAI_API_KEY"

[tools.web_fetch]
max_chars = 12000

[custom]
flag = true
"#;
    fs::create_dir_all(&root).expect("should create temp root");
    fs::write(&path, raw).expect("should write source config");

    let migrated = migrate_path_with_defaults(&path).expect("should merge defaults with existing");
    assert!(!migrated.created_file);

    let loaded = load_from_path(&path, false).expect("migrated file should load");
    assert_eq!(
        loaded.config.model_providers["openai"].default_model,
        "gpt-4.1-mini"
    );
    assert_eq!(loaded.config.tools.web_fetch.max_chars, 12000);
    assert!(loaded.config.tools.memory.enabled);

    let merged_raw = fs::read_to_string(&path).expect("should read migrated config");
    let merged_value: toml::Value =
        toml::from_str(&merged_raw).expect("migrated toml should parse");
    assert_eq!(
        merged_value["custom"]["flag"].as_bool(),
        Some(true),
        "unknown keys should be preserved"
    );

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn reset_path_to_defaults_overwrites_existing_config() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let root = env::temp_dir().join(format!("klaw-config-reset-test-{suffix}"));
    let path = root.join("config.toml");

    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4.1-mini"
env_key = "OPENAI_API_KEY"
"#;
    fs::create_dir_all(&root).expect("should create temp root");
    fs::write(&path, raw).expect("should write source config");

    let migrated = reset_path_to_defaults(&path).expect("should reset to defaults");
    assert!(!migrated.created_file);

    let loaded = load_from_path(&path, false).expect("reset file should load");
    assert_eq!(
        loaded.config.model_providers["openai"].default_model,
        "gpt-4o-mini"
    );

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn validate_config_file_does_not_create_when_missing() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let root = env::temp_dir().join(format!("klaw-config-validate-test-{suffix}"));
    let path = root.join("config.toml");

    let err = validate_config_file(Some(&path)).expect_err("missing file should fail");
    assert!(matches!(err, ConfigError::ConfigNotFound(_)));
    assert!(!path.exists());

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn config_store_save_updates_shared_snapshot_revision() {
    let root = temp_test_root("klaw-config-store-test");
    let path = root.join("config.toml");

    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"
"#;
    fs::create_dir_all(&root).expect("should create temp root");
    fs::write(&path, raw).expect("should write source config");

    let store = ConfigStore::open(Some(&path)).expect("store should open");
    let store_clone = store.clone();
    let before = store.snapshot();
    assert_eq!(before.revision, 1);

    let next_raw = raw.replace("gpt-4o-mini", "gpt-4.1-mini");
    let saved = store
        .save_raw_toml(&next_raw)
        .expect("save should parse, validate and persist");
    assert_eq!(saved.revision, 2);
    assert_eq!(
        saved.config.model_providers["openai"].default_model,
        "gpt-4.1-mini"
    );

    let clone_snapshot = store_clone.snapshot();
    assert_eq!(clone_snapshot.revision, 2);
    assert_eq!(
        clone_snapshot.config.model_providers["openai"].default_model,
        "gpt-4.1-mini"
    );

    let disk_raw = fs::read_to_string(&path).expect("saved config should be written");
    assert!(disk_raw.contains("gpt-4.1-mini"));

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn config_store_update_config_merges_changes_from_stale_store_snapshots() {
    let root = temp_test_root("klaw-config-store-test");
    let path = root.join("config.toml");

    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[memory.embedding]
enabled = true
provider = "openai"
model = "text-embedding-3-small"
"#;
    fs::create_dir_all(&root).expect("should create temp root");
    fs::write(&path, raw).expect("should write source config");

    let store_a = ConfigStore::open(Some(&path)).expect("first store should open");
    let store_b = ConfigStore::open(Some(&path)).expect("second store should open");

    store_a
        .update_config(|config| {
            config.model_providers.insert(
                "anthropic".to_string(),
                ModelProviderConfig {
                    name: Some("Anthropic".to_string()),
                    base_url: "https://api.anthropic.com/v1".to_string(),
                    wire_api: "responses".to_string(),
                    default_model: "claude-sonnet-4".to_string(),
                    ..ModelProviderConfig::default()
                },
            );
            Ok(())
        })
        .expect("provider update should succeed");

    let saved = store_b
        .update_config(|config| {
            config.memory.embedding.provider = "anthropic".to_string();
            config.memory.embedding.model = "text-embedding-v4".to_string();
            Ok(())
        })
        .expect("stale store update should merge on latest disk config")
        .0;

    assert!(saved.config.model_providers.contains_key("anthropic"));
    assert_eq!(saved.config.memory.embedding.provider, "anthropic");
    assert_eq!(saved.config.memory.embedding.model, "text-embedding-v4");

    let disk_raw = fs::read_to_string(&path).expect("saved config should be readable");
    assert!(disk_raw.contains("[model_providers.anthropic]"));
    assert!(disk_raw.contains("provider = \"anthropic\""));

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn config_store_reset_and_migrate_refresh_snapshot_from_disk() {
    let root = temp_test_root("klaw-config-store-test");
    let path = root.join("config.toml");

    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4.1-mini"
env_key = "OPENAI_API_KEY"
"#;
    fs::create_dir_all(&root).expect("should create temp root");
    fs::write(&path, raw).expect("should write source config");

    let store = ConfigStore::open(Some(&path)).expect("store should open");
    let reset_snapshot = store.reset_to_defaults().expect("reset should succeed");
    assert!(reset_snapshot.revision >= 2);
    assert_eq!(
        reset_snapshot.config.model_providers["openai"].default_model,
        "gpt-4o-mini"
    );

    let merge_raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4.1-mini"
env_key = "OPENAI_API_KEY"

[custom]
flag = true
"#;
    fs::create_dir_all(&root).expect("should recreate temp root");
    fs::write(&path, merge_raw).expect("should rewrite source config");
    let migrated_snapshot = store
        .migrate_with_defaults()
        .expect("migrate should succeed");
    assert!(migrated_snapshot.revision >= 3);
    assert_eq!(
        migrated_snapshot.config.model_providers["openai"].default_model,
        "gpt-4.1-mini"
    );
    assert!(migrated_snapshot.raw_toml.contains("[custom]"));

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn config_store_validate_raw_toml_works_without_persisting() {
    let root = temp_test_root("klaw-config-store-test");
    let path = root.join("config.toml");
    fs::create_dir_all(&root).expect("should create temp root");
    fs::write(
        &path,
        r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"
"#,
    )
    .expect("should write source config");

    let store = ConfigStore::open(Some(&path)).expect("store should open");
    let valid_raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4.1-mini"
env_key = "OPENAI_API_KEY"
"#;
    store
        .validate_raw_toml(valid_raw)
        .expect("valid config should pass validation");
    let invalid_raw = "model_provider = \"openai\"\n[broken";
    let err = store
        .validate_raw_toml(invalid_raw)
        .expect_err("invalid config should fail");
    assert!(matches!(err, ConfigError::ParseConfig { .. }));

    let disk_raw = fs::read_to_string(&path).expect("source config should still exist");
    assert!(
        disk_raw.contains("gpt-4o-mini"),
        "validate should not write to disk"
    );

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn config_store_save_observability_config_persists_changes() {
    let root = temp_test_root("klaw-config-store-test");
    let path = root.join("config.toml");
    fs::create_dir_all(&root).expect("should create temp root");
    fs::write(
        &path,
        r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"
"#,
    )
    .expect("should write source config");

    let store = ConfigStore::open(Some(&path)).expect("store should open");
    store
        .update_config(|config| {
            config.model_providers.insert(
                "anthropic".to_string(),
                ModelProviderConfig {
                    name: Some("Anthropic".to_string()),
                    base_url: "https://api.anthropic.com/v1".to_string(),
                    wire_api: "responses".to_string(),
                    default_model: "claude-sonnet-4".to_string(),
                    ..ModelProviderConfig::default()
                },
            );
            Ok(())
        })
        .expect("provider update should succeed");
    let mut observability = store.snapshot().config.observability;
    observability.enabled = true;
    observability.service_name = "klaw-gui".to_string();
    observability.traces.sample_rate = 0.25;
    observability.prometheus.enabled = true;
    observability.prometheus.listen_port = 9100;
    observability.prometheus.path = "/metrics".to_string();
    observability.local_store.enabled = true;
    observability.local_store.retention_days = 14;
    observability.local_store.flush_interval_seconds = 9;

    let saved = store
        .save_observability_config(&observability)
        .expect("save should persist observability config");
    assert_eq!(saved.config.observability.service_name, "klaw-gui");
    assert!(saved.config.observability.enabled);
    assert_eq!(saved.config.observability.traces.sample_rate, 0.25);
    assert!(saved.config.observability.prometheus.enabled);
    assert_eq!(saved.config.observability.prometheus.listen_port, 9100);
    assert_eq!(saved.config.observability.local_store.retention_days, 14);
    assert!(saved.config.model_providers.contains_key("anthropic"));
    assert_eq!(
        saved
            .config
            .observability
            .local_store
            .flush_interval_seconds,
        9
    );
    assert!(saved.revision >= 2);

    let disk_raw = fs::read_to_string(&path).expect("saved config should be written");
    assert!(disk_raw.contains("[observability]"));
    assert!(disk_raw.contains("service_name = \"klaw-gui\""));
    assert!(disk_raw.contains("sample_rate = 0.25"));
    assert!(disk_raw.contains("[observability.local_store]"));
    assert!(disk_raw.contains("retention_days = 14"));
    assert!(disk_raw.contains("[model_providers.anthropic]"));

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn parse_stream_flags_from_config() {
    let parsed: AppConfig = toml::from_str(
        r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "responses"
default_model = "gpt-4.1-mini"
env_key = "OPENAI_API_KEY"
stream = true

[[channels.telegram]]
id = "bot"
enabled = true
bot_token = "token"
stream_output = true

[[channels.dingtalk]]
id = "robot"
enabled = true
client_id = "cid"
client_secret = "secret"
stream_output = true
"#,
    )
    .expect("config should parse");

    assert!(
        parsed
            .model_providers
            .get("openai")
            .expect("provider should exist")
            .stream
    );
    assert!(parsed.channels.telegram[0].stream_output);
    assert!(parsed.channels.dingtalk[0].stream_output);
}
