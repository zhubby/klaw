use super::*;
use std::{
    env, fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn parse_default_template_succeeds() {
    let template = default_config_template();
    let parsed: AppConfig = toml::from_str(&template).expect("default template should parse");
    assert_eq!(parsed.model_provider, "openai");
    assert!(parsed.model.is_none());
    assert!(parsed.model_providers.contains_key("openai"));
    assert!(!parsed.memory.embedding.enabled);
    assert_eq!(parsed.memory.embedding.provider, "openai");
    assert_eq!(parsed.memory.embedding.model, "text-embedding-3-small");
    assert_eq!(
        parsed.tools.shell.blocked_patterns,
        default_shell_blocked_patterns()
    );
    assert_eq!(
        parsed.tools.shell.safe_commands,
        default_shell_safe_commands()
    );
    assert_eq!(
        parsed.tools.shell.approval_policy,
        ShellApprovalPolicy::OnRequest
    );
    assert!(parsed.tools.shell.allow_login_shell);
    assert_eq!(parsed.tools.shell.max_timeout_ms, 120_000);
    assert_eq!(parsed.tools.shell.max_output_bytes, 128 * 1024);
    assert!(parsed.tools.shell.workspace.is_none());
    assert!(parsed.tools.apply_patch.workspace.is_none());
    assert!(!parsed.tools.apply_patch.allow_absolute_paths);
    assert!(parsed.tools.apply_patch.allowed_roots.is_empty());
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
    assert!(parsed.heartbeat.defaults.enabled);
    assert_eq!(parsed.heartbeat.defaults.every, "30m");
    assert_eq!(parsed.heartbeat.defaults.silent_ack_token, "HEARTBEAT_OK");
    assert!(parsed.heartbeat.sessions.is_empty());
    assert!(parsed.channels.dingtalk.is_empty());
    assert_eq!(
        parsed.tools.sub_agent.exclude_tools,
        vec!["sub_agent".to_string()]
    );
    assert!(parsed.mcp.enabled);
    assert_eq!(parsed.mcp.startup_timeout_seconds, 60);
    assert!(parsed.mcp.servers.is_empty());
    assert_eq!(parsed.gateway.listen_ip, "127.0.0.1");
    assert_eq!(parsed.gateway.listen_port, 8080);
    assert!(!parsed.gateway.tls.enabled);
    assert!(parsed.gateway.tls.cert_path.is_none());
    assert!(parsed.gateway.tls.key_path.is_none());
    validate(&parsed).expect("default template should be valid");
}

#[test]
fn parse_root_model_override_succeeds() {
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
    let mut cfg = AppConfig::default();
    cfg.model = Some("   ".to_string());
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("model cannot be empty when configured"));
}

#[test]
fn parse_tools_shell_blocked_patterns_succeeds() {
    let raw = r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[tools.shell]
workspace = "/Users/example/shell"
blocked_patterns = ["sudo rm -rf /tmp/example"]
safe_commands = ["ls", "cat"]
approval_policy = "never"
allow_login_shell = false
max_timeout_ms = 30000
max_output_bytes = 64000
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
    assert_eq!(
        parsed.tools.shell.blocked_patterns,
        vec!["sudo rm -rf /tmp/example".to_string()]
    );
    assert_eq!(
        parsed.tools.shell.safe_commands,
        vec!["ls".to_string(), "cat".to_string()]
    );
    assert_eq!(
        parsed.tools.shell.approval_policy,
        ShellApprovalPolicy::Never
    );
    assert_eq!(
        parsed.tools.shell.workspace.as_deref(),
        Some("/Users/example/shell")
    );
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
workspace = "/Users/example/patch"
allow_absolute_paths = true
allowed_roots = ["/tmp", "sandbox/allowed"]
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
    assert_eq!(
        parsed.tools.apply_patch.workspace.as_deref(),
        Some("/Users/example/patch")
    );
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
    assert_eq!(parsed.tools.web_search.enabled, false);
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
    assert!(parsed.mcp.enabled);
    assert_eq!(parsed.mcp.startup_timeout_seconds, 30);
    assert_eq!(parsed.mcp.servers.len(), 2);
    assert_eq!(parsed.mcp.servers[0].mode, McpServerMode::Stdio);
    assert_eq!(parsed.mcp.servers[0].command.as_deref(), Some("npx"),);
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
listen_ip = "0.0.0.0"
listen_port = 18080

[gateway.tls]
enabled = false
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("custom config should parse");
    assert_eq!(parsed.gateway.listen_ip, "0.0.0.0");
    assert_eq!(parsed.gateway.listen_port, 18_080);
    assert!(!parsed.gateway.tls.enabled);
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
session_key = "stdio:main"
chat_id = "main"
channel = "stdio"
enabled = true
every = "10m"
"#;

    let parsed: AppConfig = toml::from_str(raw).expect("heartbeat config should parse");
    assert_eq!(parsed.heartbeat.defaults.every, "45m");
    assert_eq!(parsed.heartbeat.defaults.timezone, "Asia/Shanghai");
    assert_eq!(parsed.heartbeat.sessions.len(), 1);
    assert_eq!(parsed.heartbeat.sessions[0].session_key, "stdio:main");
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
fn validate_fails_when_heartbeat_session_keys_duplicate() {
    let mut cfg = AppConfig::default();
    cfg.heartbeat.sessions = vec![
        HeartbeatSessionConfig {
            session_key: "stdio:dup".to_string(),
            chat_id: "a".to_string(),
            channel: "stdio".to_string(),
            enabled: None,
            every: None,
            prompt: None,
            silent_ack_token: None,
            timezone: None,
        },
        HeartbeatSessionConfig {
            session_key: "stdio:dup".to_string(),
            chat_id: "b".to_string(),
            channel: "stdio".to_string(),
            enabled: None,
            every: None,
            prompt: None,
            silent_ack_token: None,
            timezone: None,
        },
    ];

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("duplicated session_key"));
}

#[test]
fn validate_fails_when_heartbeat_every_is_invalid() {
    let mut cfg = AppConfig::default();
    cfg.heartbeat.defaults.every = "0s".to_string();
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("heartbeat.defaults.every"));
}

#[test]
fn validate_fails_when_gateway_ip_is_invalid() {
    let mut cfg = AppConfig::default();
    cfg.gateway.listen_ip = "invalid-ip".to_string();
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("gateway.listen_ip"));
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
fn validate_fails_when_mcp_server_ids_duplicate() {
    let mut cfg = AppConfig::default();
    cfg.mcp.servers = vec![
        McpServerConfig {
            id: "dup".to_string(),
            enabled: true,
            mode: McpServerMode::Stdio,
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
fn validate_fails_when_apply_patch_allowed_root_is_empty() {
    let mut cfg = AppConfig::default();
    cfg.tools.apply_patch.allowed_roots = vec![" ".to_string()];

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("tools.apply_patch.allowed_roots"));
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
fn validate_fails_when_storage_root_dir_is_empty() {
    let mut cfg = AppConfig::default();
    cfg.storage.root_dir = Some("   ".to_string());

    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("storage.root_dir"));
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
        SkillRegistryConfig {
            address: "https://github.com/anthropics/skills".to_string(),
            installed: vec![],
        },
    );
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("skills.<registry>.address"));

    let mut cfg2 = AppConfig::default();
    cfg2.skills.registries.insert(
        "empty-address".to_string(),
        SkillRegistryConfig {
            address: String::new(),
            installed: vec![],
        },
    );
    let err2 = validate(&cfg2).expect_err("should fail");
    assert!(format!("{err2}").contains("skills.<registry>.address"));

    let mut cfg3 = AppConfig::default();
    cfg3.skills.registries.insert(
        "anthropic".to_string(),
        SkillRegistryConfig {
            address: "https://github.com/anthropics/skills".to_string(),
            installed: vec!["".to_string()],
        },
    );
    let err3 = validate(&cfg3).expect_err("should fail");
    assert!(format!("{err3}").contains("empty skill name"));

    let mut cfg4 = AppConfig::default();
    cfg4.skills.registries.insert(
        "anthropic".to_string(),
        SkillRegistryConfig {
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
fn validate_fails_when_shell_limits_are_invalid() {
    let mut cfg = AppConfig::default();
    cfg.tools.shell.safe_commands.clear();
    let err = validate(&cfg).expect_err("should fail");
    assert!(format!("{err}").contains("tools.shell.safe_commands"));

    let mut cfg2 = AppConfig::default();
    cfg2.tools.shell.max_timeout_ms = 0;
    let err2 = validate(&cfg2).expect_err("should fail");
    assert!(format!("{err2}").contains("tools.shell.max_timeout_ms"));

    let mut cfg3 = AppConfig::default();
    cfg3.tools.shell.max_output_bytes = 0;
    let err3 = validate(&cfg3).expect_err("should fail");
    assert!(format!("{err3}").contains("tools.shell.max_output_bytes"));
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
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let root = env::temp_dir().join(format!("klaw-config-store-test-{suffix}"));
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
fn config_store_reset_and_migrate_refresh_snapshot_from_disk() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let root = env::temp_dir().join(format!("klaw-config-store-test-{suffix}"));
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
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let root = env::temp_dir().join(format!("klaw-config-store-test-{suffix}"));
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
