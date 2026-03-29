use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_millis;
use crate::widgets::show_json_tree_with_id;
use crate::{request_sync_tools, request_tool_definitions};
use chrono::{Datelike, Local, NaiveDate};
use egui::{Color32, FontId, TextFormat, text::LayoutJob};
use egui_extras::{Column, DatePickerButton, Size, StripBuilder, TableBuilder};
use egui_phosphor::regular;
use klaw_config::{
    AppConfig, ApplyPatchConfig, ChannelAttachmentToolConfig, ConfigError, ConfigSnapshot,
    ConfigStore, LocalAttachmentConfig, MemoryToolConfig, ShellConfig, SubAgentConfig,
    WebFetchConfig, WebSearchConfig,
};
use klaw_llm::ToolDefinition;
use klaw_session::{
    SessionManager, SqliteSessionManager, ToolAuditFilterOptionsQuery, ToolAuditQuery,
    ToolAuditRecord, ToolAuditSortOrder,
};
use std::future::Future;
use std::thread;
use time::{Month, OffsetDateTime, PrimitiveDateTime, Time};
use tokio::runtime::Builder;

pub struct ToolPanel {
    store: Option<ConfigStore>,
    config: AppConfig,
    form: Option<ToolForm>,
    runtime_definitions: Vec<ToolDefinition>,
    inspect_key: Option<&'static str>,
    logs_key: Option<&'static str>,
    log_rows: Vec<ToolAuditRecord>,
    log_selected_id: Option<String>,
    log_summary_id: Option<String>,
    log_summary_tab: ToolLogSummaryTab,
    log_session_options: Vec<String>,
    log_session_filter: Option<String>,
    log_start_date: Option<NaiveDate>,
    log_end_date: Option<NaiveDate>,
    log_status_filter: LogStatusFilter,
    log_sort_order: ToolLogSortOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LogStatusFilter {
    #[default]
    All,
    FailedOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ToolLogSortOrder {
    StartedAtAsc,
    #[default]
    StartedAtDesc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ToolLogSummaryTab {
    #[default]
    Arguments,
    Result,
    Metadata,
}

#[derive(Debug, Clone)]
enum ToolForm {
    ApplyPatch(ApplyPatchForm),
    Shell(ShellForm),
    Archive(ToggleForm),
    ChannelAttachment(ChannelAttachmentForm),
    Voice(ToggleForm),
    Approval(ToggleForm),
    Geo(ToggleForm),
    LocalSearch(ToggleForm),
    TerminalMultiplexers(ToggleForm),
    CronManager(ToggleForm),
    HeartbeatManager(ToggleForm),
    SkillsRegistry(ToggleForm),
    SkillsManager(ToggleForm),
    Memory(MemoryForm),
    WebFetch(WebFetchForm),
    WebSearch(WebSearchForm),
    SubAgent(SubAgentForm),
}

#[derive(Debug, Clone)]
struct ToggleForm {
    title: &'static str,
    enabled: bool,
}

#[derive(Debug, Clone)]
struct ApplyPatchForm {
    enabled: bool,
    workspace: String,
    allow_absolute_paths: bool,
    allowed_roots_text: String,
}

#[derive(Debug, Clone)]
struct ShellForm {
    enabled: bool,
    workspace: String,
    blocked_patterns_text: String,
    unsafe_patterns_text: String,
    allow_login_shell: bool,
    max_timeout_ms: String,
    max_output_bytes: String,
}

#[derive(Debug, Clone)]
struct ChannelAttachmentForm {
    enabled: bool,
    allowlist_text: String,
    max_bytes: String,
}

#[derive(Debug, Clone)]
struct MemoryForm {
    enabled: bool,
    search_limit: String,
    fts_limit: String,
    vector_limit: String,
    use_vector: bool,
}

#[derive(Debug, Clone)]
struct WebFetchForm {
    enabled: bool,
    max_chars: String,
    timeout_seconds: String,
    cache_ttl_minutes: String,
    max_redirects: String,
    readability: bool,
    ssrf_allowlist_text: String,
}

#[derive(Debug, Clone)]
struct WebSearchForm {
    enabled: bool,
    provider: String,
    tavily_base_url: String,
    tavily_api_key: String,
    tavily_env_key: String,
    tavily_search_depth: String,
    tavily_topic: String,
    tavily_include_answer: bool,
    tavily_include_raw_content: bool,
    tavily_include_images: bool,
    tavily_project_id: String,
    brave_base_url: String,
    brave_api_key: String,
    brave_env_key: String,
    brave_country: String,
    brave_search_lang: String,
    brave_ui_lang: String,
    brave_safesearch: String,
    brave_freshness: String,
}

#[derive(Debug, Clone)]
struct SubAgentForm {
    enabled: bool,
    max_iterations: String,
    max_tool_calls: String,
    inherit_parent_tools: bool,
    exclude_tools_text: String,
}

#[derive(Clone, Copy)]
struct ToolDescriptor {
    key: &'static str,
    name: &'static str,
    description: &'static str,
    enabled: bool,
}

const INSPECT_WINDOW_WIDTH: f32 = 760.0;
const INSPECT_WINDOW_MAX_HEIGHT: f32 = 760.0;
const INSPECT_WINDOW_CHROME_HEIGHT: f32 = 56.0;
const INSPECT_SECTION_CHROME_HEIGHT: f32 = 28.0;
const INSPECT_DESCRIPTION_HEIGHT: f32 = 120.0;
const INSPECT_SCHEMA_HEIGHT: f32 = 260.0;
const LOGS_WINDOW_VIEWPORT_RATIO: f32 = 2.0 / 3.0;
const LOGS_SUMMARY_WINDOW_WIDTH: f32 = 860.0;
const LOGS_SUMMARY_WINDOW_HEIGHT: f32 = 720.0;
const LOGS_SUMMARY_WINDOW_CHROME_HEIGHT: f32 = 56.0;
const LOGS_SUMMARY_STATIC_HEIGHT: f32 = 220.0;
const LOG_WINDOW_VIEWPORT_MARGIN: f32 = 48.0;
const LOG_DETAIL_SECTION_HEIGHT: f32 = 220.0;
const LOG_DETAIL_SECTION_BLOCK_HEIGHT: f32 = LOG_DETAIL_SECTION_HEIGHT + 28.0;
const TOOL_LOG_PAGE_SIZE: i64 = 100;

impl Default for ToolPanel {
    fn default() -> Self {
        let today = Local::now().date_naive();
        let one_month_ago = today - chrono::Duration::days(30);
        Self {
            store: None,
            config: AppConfig::default(),
            form: None,
            runtime_definitions: Vec::new(),
            inspect_key: None,
            logs_key: None,
            log_rows: Vec::new(),
            log_selected_id: None,
            log_summary_id: None,
            log_summary_tab: ToolLogSummaryTab::Arguments,
            log_session_options: Vec::new(),
            log_session_filter: None,
            log_start_date: Some(one_month_ago),
            log_end_date: Some(today),
            log_status_filter: LogStatusFilter::All,
            log_sort_order: ToolLogSortOrder::StartedAtDesc,
        }
    }
}

impl ApplyPatchForm {
    fn from_config(config: &ApplyPatchConfig) -> Self {
        Self {
            enabled: config.enabled,
            workspace: config.workspace.clone().unwrap_or_default(),
            allow_absolute_paths: config.allow_absolute_paths,
            allowed_roots_text: config.allowed_roots.join("\n"),
        }
    }

    fn to_config(&self) -> ApplyPatchConfig {
        let workspace = self.workspace.trim();
        ApplyPatchConfig {
            enabled: self.enabled,
            workspace: (!workspace.is_empty()).then(|| workspace.to_string()),
            allow_absolute_paths: self.allow_absolute_paths,
            allowed_roots: parse_lines(&self.allowed_roots_text),
        }
    }
}

impl ShellForm {
    fn from_config(config: &ShellConfig) -> Self {
        Self {
            enabled: config.enabled,
            workspace: config.workspace.clone().unwrap_or_default(),
            blocked_patterns_text: config.blocked_patterns.join("\n"),
            unsafe_patterns_text: config.unsafe_patterns.join("\n"),
            allow_login_shell: config.allow_login_shell,
            max_timeout_ms: config.max_timeout_ms.to_string(),
            max_output_bytes: config.max_output_bytes.to_string(),
        }
    }

    fn to_config(&self) -> Result<ShellConfig, String> {
        let max_timeout_ms = self
            .max_timeout_ms
            .trim()
            .parse::<u64>()
            .map_err(|_| "max_timeout_ms must be a positive integer".to_string())?;
        let max_output_bytes = self
            .max_output_bytes
            .trim()
            .parse::<usize>()
            .map_err(|_| "max_output_bytes must be a positive integer".to_string())?;

        let workspace = self.workspace.trim();
        Ok(ShellConfig {
            enabled: self.enabled,
            workspace: (!workspace.is_empty()).then(|| workspace.to_string()),
            blocked_patterns: parse_lines(&self.blocked_patterns_text),
            unsafe_patterns: parse_lines(&self.unsafe_patterns_text),
            allow_login_shell: self.allow_login_shell,
            max_timeout_ms,
            max_output_bytes,
        })
    }
}

impl ChannelAttachmentForm {
    fn from_config(config: &ChannelAttachmentToolConfig) -> Self {
        Self {
            enabled: config.enabled,
            allowlist_text: config.local_attachments.allowlist.join("\n"),
            max_bytes: config.local_attachments.max_bytes.to_string(),
        }
    }

    fn to_config(&self) -> Result<ChannelAttachmentToolConfig, String> {
        let max_bytes = self
            .max_bytes
            .trim()
            .parse::<u64>()
            .map_err(|_| "max_bytes must be a positive integer".to_string())?;

        Ok(ChannelAttachmentToolConfig {
            enabled: self.enabled,
            local_attachments: LocalAttachmentConfig {
                allowlist: parse_lines(&self.allowlist_text),
                max_bytes,
            },
        })
    }
}

impl MemoryForm {
    fn from_config(config: &MemoryToolConfig) -> Self {
        Self {
            enabled: config.enabled,
            search_limit: config.search_limit.to_string(),
            fts_limit: config.fts_limit.to_string(),
            vector_limit: config.vector_limit.to_string(),
            use_vector: config.use_vector,
        }
    }

    fn to_config(&self) -> Result<MemoryToolConfig, String> {
        let search_limit = self
            .search_limit
            .trim()
            .parse::<usize>()
            .map_err(|_| "search_limit must be a positive integer".to_string())?;
        let fts_limit = self
            .fts_limit
            .trim()
            .parse::<usize>()
            .map_err(|_| "fts_limit must be a positive integer".to_string())?;
        let vector_limit = self
            .vector_limit
            .trim()
            .parse::<usize>()
            .map_err(|_| "vector_limit must be a positive integer".to_string())?;

        Ok(MemoryToolConfig {
            enabled: self.enabled,
            search_limit,
            fts_limit,
            vector_limit,
            use_vector: self.use_vector,
        })
    }
}

impl WebFetchForm {
    fn from_config(config: &WebFetchConfig) -> Self {
        Self {
            enabled: config.enabled,
            max_chars: config.max_chars.to_string(),
            timeout_seconds: config.timeout_seconds.to_string(),
            cache_ttl_minutes: config.cache_ttl_minutes.to_string(),
            max_redirects: config.max_redirects.to_string(),
            readability: config.readability,
            ssrf_allowlist_text: config.ssrf_allowlist.join("\n"),
        }
    }

    fn to_config(&self) -> Result<WebFetchConfig, String> {
        let max_chars = self
            .max_chars
            .trim()
            .parse::<usize>()
            .map_err(|_| "max_chars must be a positive integer".to_string())?;
        let timeout_seconds = self
            .timeout_seconds
            .trim()
            .parse::<u64>()
            .map_err(|_| "timeout_seconds must be a positive integer".to_string())?;
        let cache_ttl_minutes = self
            .cache_ttl_minutes
            .trim()
            .parse::<u64>()
            .map_err(|_| "cache_ttl_minutes must be a positive integer".to_string())?;
        let max_redirects = self
            .max_redirects
            .trim()
            .parse::<u8>()
            .map_err(|_| "max_redirects must be an integer between 0 and 255".to_string())?;

        Ok(WebFetchConfig {
            enabled: self.enabled,
            max_chars,
            timeout_seconds,
            cache_ttl_minutes,
            max_redirects,
            readability: self.readability,
            ssrf_allowlist: parse_lines(&self.ssrf_allowlist_text),
        })
    }
}

impl WebSearchForm {
    fn from_config(config: &WebSearchConfig) -> Self {
        Self {
            enabled: config.enabled,
            provider: config.provider.clone(),
            tavily_base_url: config.tavily.base_url.clone().unwrap_or_default(),
            tavily_api_key: config.tavily.api_key.clone().unwrap_or_default(),
            tavily_env_key: config.tavily.env_key.clone().unwrap_or_default(),
            tavily_search_depth: config.tavily.search_depth.clone(),
            tavily_topic: config.tavily.topic.clone().unwrap_or_default(),
            tavily_include_answer: config.tavily.include_answer.unwrap_or(false),
            tavily_include_raw_content: config.tavily.include_raw_content.unwrap_or(false),
            tavily_include_images: config.tavily.include_images.unwrap_or(false),
            tavily_project_id: config.tavily.project_id.clone().unwrap_or_default(),
            brave_base_url: config.brave.base_url.clone().unwrap_or_default(),
            brave_api_key: config.brave.api_key.clone().unwrap_or_default(),
            brave_env_key: config.brave.env_key.clone().unwrap_or_default(),
            brave_country: config.brave.country.clone().unwrap_or_default(),
            brave_search_lang: config.brave.search_lang.clone().unwrap_or_default(),
            brave_ui_lang: config.brave.ui_lang.clone().unwrap_or_default(),
            brave_safesearch: config.brave.safesearch.clone().unwrap_or_default(),
            brave_freshness: config.brave.freshness.clone().unwrap_or_default(),
        }
    }

    fn apply_to(&self, config: &mut WebSearchConfig) {
        let provider = self.provider.trim();
        config.enabled = self.enabled;
        config.provider = provider.to_string();

        config.tavily.base_url = optional_string(&self.tavily_base_url);
        config.tavily.api_key = optional_string(&self.tavily_api_key);
        config.tavily.env_key = optional_string(&self.tavily_env_key);
        config.tavily.search_depth = self.tavily_search_depth.trim().to_string();
        config.tavily.topic = optional_string(&self.tavily_topic);
        config.tavily.include_answer = Some(self.tavily_include_answer);
        config.tavily.include_raw_content = Some(self.tavily_include_raw_content);
        config.tavily.include_images = Some(self.tavily_include_images);
        config.tavily.project_id = optional_string(&self.tavily_project_id);

        config.brave.base_url = optional_string(&self.brave_base_url);
        config.brave.api_key = optional_string(&self.brave_api_key);
        config.brave.env_key = optional_string(&self.brave_env_key);
        config.brave.country = optional_string(&self.brave_country);
        config.brave.search_lang = optional_string(&self.brave_search_lang);
        config.brave.ui_lang = optional_string(&self.brave_ui_lang);
        config.brave.safesearch = optional_string(&self.brave_safesearch);
        config.brave.freshness = optional_string(&self.brave_freshness);
    }
}

impl SubAgentForm {
    fn from_config(config: &SubAgentConfig) -> Self {
        Self {
            enabled: config.enabled,
            max_iterations: config.max_iterations.to_string(),
            max_tool_calls: config.max_tool_calls.to_string(),
            inherit_parent_tools: config.inherit_parent_tools,
            exclude_tools_text: config.exclude_tools.join("\n"),
        }
    }

    fn to_config(&self) -> Result<SubAgentConfig, String> {
        let max_iterations = self
            .max_iterations
            .trim()
            .parse::<u32>()
            .map_err(|_| "max_iterations must be a positive integer".to_string())?;
        let max_tool_calls = self
            .max_tool_calls
            .trim()
            .parse::<u32>()
            .map_err(|_| "max_tool_calls must be a positive integer".to_string())?;

        Ok(SubAgentConfig {
            enabled: self.enabled,
            max_iterations,
            max_tool_calls,
            inherit_parent_tools: self.inherit_parent_tools,
            exclude_tools: parse_lines(&self.exclude_tools_text),
        })
    }
}

impl ToolForm {
    fn title(&self) -> &'static str {
        match self {
            ToolForm::ApplyPatch(_) => "Edit Tool: apply_patch",
            ToolForm::Shell(_) => "Edit Tool: shell",
            ToolForm::Archive(form)
            | ToolForm::Voice(form)
            | ToolForm::Approval(form)
            | ToolForm::Geo(form)
            | ToolForm::LocalSearch(form)
            | ToolForm::TerminalMultiplexers(form)
            | ToolForm::CronManager(form)
            | ToolForm::HeartbeatManager(form)
            | ToolForm::SkillsRegistry(form)
            | ToolForm::SkillsManager(form) => form.title,
            ToolForm::ChannelAttachment(_) => "Edit Tool: channel_attachment",
            ToolForm::Memory(_) => "Edit Tool: memory",
            ToolForm::WebFetch(_) => "Edit Tool: web_fetch",
            ToolForm::WebSearch(_) => "Edit Tool: web_search",
            ToolForm::SubAgent(_) => "Edit Tool: sub_agent",
        }
    }
}

impl ToolPanel {
    fn ensure_store_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.store.is_some() {
            return;
        }
        match ConfigStore::open(None) {
            Ok(store) => {
                let snapshot = store.snapshot();
                self.store = Some(store);
                self.apply_snapshot(snapshot);
                self.refresh_runtime_tool_definitions(notifications, false);
                notifications.success("Tool config loaded from disk");
            }
            Err(err) => notifications.error(format!("Failed to load config: {err}")),
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config = snapshot.config;
    }

    fn save_config<F>(
        &mut self,
        notifications: &mut NotificationCenter,
        success_message: &str,
        mutate: F,
    ) -> bool
    where
        F: FnOnce(&mut AppConfig) -> Result<(), String>,
    {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return false;
        };
        match store.update_config(|config| mutate(config).map_err(ConfigError::InvalidConfig)) {
            Ok((snapshot, ())) => {
                self.apply_snapshot(snapshot);
                notifications.success(success_message);
                true
            }
            Err(err) => {
                notifications.error(format!("Save failed: {err}"));
                false
            }
        }
    }

    fn reload(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };
        match store.reload() {
            Ok(snapshot) => {
                self.apply_snapshot(snapshot);
                self.refresh_runtime_tool_definitions(notifications, false);
                notifications.success("Configuration reloaded from disk");
            }
            Err(err) => notifications.error(format!("Reload failed: {err}")),
        }
    }

    fn refresh_runtime_tool_definitions(
        &mut self,
        notifications: &mut NotificationCenter,
        notify_on_error: bool,
    ) {
        match request_tool_definitions() {
            Ok(definitions) => {
                self.runtime_definitions = definitions;
            }
            Err(err) if notify_on_error => {
                notifications.error(format!("Failed to load runtime tool metadata: {err}"));
            }
            Err(_) => {}
        }
    }

    fn tools(&self) -> Vec<ToolDescriptor> {
        let mut tools = vec![
            ToolDescriptor {
                key: "apply_patch",
                name: "apply_patch",
                description: "Patch workspace files with constrained path policy.",
                enabled: self.config.tools.apply_patch.enabled,
            },
            ToolDescriptor {
                key: "shell",
                name: "shell",
                description: "Execute local shell commands with approval policy.",
                enabled: self.config.tools.shell.enabled,
            },
            ToolDescriptor {
                key: "archive",
                name: "archive",
                description: "Manage archived attachments from conversations.",
                enabled: self.config.tools.archive.enabled,
            },
            ToolDescriptor {
                key: "channel_attachment",
                name: "channel_attachment",
                description: "Send archived or approved local files back into the current chat.",
                enabled: self.config.tools.channel_attachment.enabled,
            },
            ToolDescriptor {
                key: "voice",
                name: "voice",
                description: "Transcribe audio and synthesize text into speech.",
                enabled: self.config.tools.voice.enabled,
            },
            ToolDescriptor {
                key: "approval",
                name: "approval",
                description: "Manage approval lifecycle for high-risk actions.",
                enabled: self.config.tools.approval.enabled,
            },
            ToolDescriptor {
                key: "geo",
                name: "geo",
                description: "Get current coordinates from available system location services.",
                enabled: self.config.tools.geo.enabled,
            },
            ToolDescriptor {
                key: "local_search",
                name: "local_search",
                description: "Search local workspace files and snippets.",
                enabled: self.config.tools.local_search.enabled,
            },
            ToolDescriptor {
                key: "terminal_multiplexers",
                name: "terminal_multiplexers",
                description: "Operate tmux/zellij sessions for long-running tasks.",
                enabled: self.config.tools.terminal_multiplexers.enabled,
            },
            ToolDescriptor {
                key: "cron_manager",
                name: "cron_manager",
                description: "Create and control scheduled cron jobs.",
                enabled: self.config.tools.cron_manager.enabled,
            },
            ToolDescriptor {
                key: "heartbeat_manager",
                name: "heartbeat_manager",
                description: "Manage session-bound heartbeat jobs.",
                enabled: self.config.tools.heartbeat_manager.enabled,
            },
            ToolDescriptor {
                key: "skills_registry",
                name: "skills_registry",
                description: "Browse and inspect read-only registry catalogs.",
                enabled: self.config.tools.skills_registry.enabled,
            },
            ToolDescriptor {
                key: "skills_manager",
                name: "skills_manager",
                description: "Install, uninstall, and load installed skills.",
                enabled: self.config.tools.skills_manager.enabled,
            },
            ToolDescriptor {
                key: "memory",
                name: "memory",
                description: "Persist and retrieve long-term memory records.",
                enabled: self.config.tools.memory.enabled,
            },
            ToolDescriptor {
                key: "web_fetch",
                name: "web_fetch",
                description: "Fetch and extract web page content safely.",
                enabled: self.config.tools.web_fetch.enabled,
            },
            ToolDescriptor {
                key: "web_search",
                name: "web_search",
                description: "Search web results via configured provider.",
                enabled: self.config.tools.web_search.enabled,
            },
            ToolDescriptor {
                key: "sub_agent",
                name: "sub_agent",
                description: "Delegate focused tasks to a bounded child agent.",
                enabled: self.config.tools.sub_agent.enabled,
            },
        ];
        tools.sort_unstable_by(|left, right| left.name.cmp(right.name));
        tools
    }

    fn runtime_definition(&self, key: &str) -> Option<&ToolDefinition> {
        self.runtime_definitions
            .iter()
            .find(|item| item.name == key)
    }

    fn open_editor(&mut self, key: &str) {
        match key {
            "apply_patch" => self.open_apply_patch(),
            "shell" => self.open_shell(),
            "archive" => self.open_toggle(
                "archive",
                "Edit Tool: archive",
                self.config.tools.archive.enabled,
            ),
            "channel_attachment" => self.open_channel_attachment(),
            "voice" => {
                self.open_toggle("voice", "Edit Tool: voice", self.config.tools.voice.enabled)
            }
            "approval" => self.open_toggle(
                "approval",
                "Edit Tool: approval",
                self.config.tools.approval.enabled,
            ),
            "geo" => self.open_toggle("geo", "Edit Tool: geo", self.config.tools.geo.enabled),
            "local_search" => self.open_toggle(
                "local_search",
                "Edit Tool: local_search",
                self.config.tools.local_search.enabled,
            ),
            "terminal_multiplexers" => self.open_toggle(
                "terminal_multiplexers",
                "Edit Tool: terminal_multiplexers",
                self.config.tools.terminal_multiplexers.enabled,
            ),
            "cron_manager" => self.open_toggle(
                "cron_manager",
                "Edit Tool: cron_manager",
                self.config.tools.cron_manager.enabled,
            ),
            "heartbeat_manager" => self.open_toggle(
                "heartbeat_manager",
                "Edit Tool: heartbeat_manager",
                self.config.tools.heartbeat_manager.enabled,
            ),
            "skills_registry" => self.open_toggle(
                "skills_registry",
                "Edit Tool: skills_registry",
                self.config.tools.skills_registry.enabled,
            ),
            "skills_manager" => self.open_toggle(
                "skills_manager",
                "Edit Tool: skills_manager",
                self.config.tools.skills_manager.enabled,
            ),
            "memory" => self.open_memory(),
            "web_fetch" => self.open_web_fetch(),
            "web_search" => self.open_web_search(),
            "sub_agent" => self.open_sub_agent(),
            _ => {}
        }
    }

    fn open_apply_patch(&mut self) {
        self.form = Some(ToolForm::ApplyPatch(ApplyPatchForm::from_config(
            &self.config.tools.apply_patch,
        )));
    }

    fn open_shell(&mut self) {
        self.form = Some(ToolForm::Shell(ShellForm::from_config(
            &self.config.tools.shell,
        )));
    }

    fn open_channel_attachment(&mut self) {
        self.form = Some(ToolForm::ChannelAttachment(
            ChannelAttachmentForm::from_config(&self.config.tools.channel_attachment),
        ));
    }

    fn open_toggle(&mut self, key: &str, title: &'static str, enabled: bool) {
        let form = ToggleForm { title, enabled };
        self.form = Some(match key {
            "archive" => ToolForm::Archive(form),
            "voice" => ToolForm::Voice(form),
            "approval" => ToolForm::Approval(form),
            "geo" => ToolForm::Geo(form),
            "local_search" => ToolForm::LocalSearch(form),
            "terminal_multiplexers" => ToolForm::TerminalMultiplexers(form),
            "cron_manager" => ToolForm::CronManager(form),
            "heartbeat_manager" => ToolForm::HeartbeatManager(form),
            "skills_registry" => ToolForm::SkillsRegistry(form),
            _ => ToolForm::SkillsManager(form),
        });
    }

    fn open_memory(&mut self) {
        self.form = Some(ToolForm::Memory(MemoryForm::from_config(
            &self.config.tools.memory,
        )));
    }

    fn open_web_fetch(&mut self) {
        self.form = Some(ToolForm::WebFetch(WebFetchForm::from_config(
            &self.config.tools.web_fetch,
        )));
    }

    fn open_web_search(&mut self) {
        self.form = Some(ToolForm::WebSearch(WebSearchForm::from_config(
            &self.config.tools.web_search,
        )));
    }

    fn open_sub_agent(&mut self) {
        self.form = Some(ToolForm::SubAgent(SubAgentForm::from_config(
            &self.config.tools.sub_agent,
        )));
    }

    fn save_form(&mut self, notifications: &mut NotificationCenter) {
        let Some(form) = self.form.clone() else {
            return;
        };

        if self.save_config(
            notifications,
            "Tool config saved",
            move |config| match form {
                ToolForm::ApplyPatch(form) => {
                    config.tools.apply_patch = form.to_config();
                    Ok(())
                }
                ToolForm::Shell(form) => {
                    config.tools.shell = form.to_config()?;
                    Ok(())
                }
                ToolForm::Archive(form) => {
                    config.tools.archive.enabled = form.enabled;
                    Ok(())
                }
                ToolForm::ChannelAttachment(form) => {
                    config.tools.channel_attachment = form.to_config()?;
                    Ok(())
                }
                ToolForm::Voice(form) => {
                    config.tools.voice.enabled = form.enabled;
                    Ok(())
                }
                ToolForm::Approval(form) => {
                    config.tools.approval.enabled = form.enabled;
                    Ok(())
                }
                ToolForm::Geo(form) => {
                    config.tools.geo.enabled = form.enabled;
                    Ok(())
                }
                ToolForm::LocalSearch(form) => {
                    config.tools.local_search.enabled = form.enabled;
                    Ok(())
                }
                ToolForm::TerminalMultiplexers(form) => {
                    config.tools.terminal_multiplexers.enabled = form.enabled;
                    Ok(())
                }
                ToolForm::CronManager(form) => {
                    config.tools.cron_manager.enabled = form.enabled;
                    Ok(())
                }
                ToolForm::HeartbeatManager(form) => {
                    config.tools.heartbeat_manager.enabled = form.enabled;
                    Ok(())
                }
                ToolForm::SkillsRegistry(form) => {
                    config.tools.skills_registry.enabled = form.enabled;
                    Ok(())
                }
                ToolForm::SkillsManager(form) => {
                    config.tools.skills_manager.enabled = form.enabled;
                    Ok(())
                }
                ToolForm::Memory(form) => {
                    config.tools.memory = form.to_config()?;
                    Ok(())
                }
                ToolForm::WebFetch(form) => {
                    config.tools.web_fetch = form.to_config()?;
                    Ok(())
                }
                ToolForm::WebSearch(form) => {
                    form.apply_to(&mut config.tools.web_search);
                    Ok(())
                }
                ToolForm::SubAgent(form) => {
                    config.tools.sub_agent = form.to_config()?;
                    Ok(())
                }
            },
        ) {
            match request_sync_tools() {
                Ok(tool_names) => notifications.success(format!(
                    "Tool config saved and runtime synced ({} tools active)",
                    tool_names.len()
                )),
                Err(err) => notifications.error(format!(
                    "Tool config saved, but failed to sync running runtime: {err}"
                )),
            }
            self.refresh_runtime_tool_definitions(notifications, true);
            self.form = None;
        }
    }

    fn render_form_window(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        let mut save_clicked = false;
        let mut cancel_clicked = false;

        let Some(form) = self.form.as_mut() else {
            return;
        };

        egui::Window::new(form.title())
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .show(ui.ctx(), |ui| {
                ui.set_min_width(560.0);

                match form {
                    ToolForm::ApplyPatch(form) => {
                        egui::Grid::new("tool-apply-patch-grid")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                ui.label("enabled");
                                ui.checkbox(&mut form.enabled, "");
                                ui.end_row();

                                ui.label("workspace");
                                ui.text_edit_singleline(&mut form.workspace);
                                ui.end_row();

                                ui.label("allow_absolute_paths");
                                ui.checkbox(&mut form.allow_absolute_paths, "");
                                ui.end_row();
                            });
                        ui.separator();
                        ui.label("allowed_roots (one path per line)");
                        ui.add(
                            egui::TextEdit::multiline(&mut form.allowed_roots_text)
                                .desired_rows(5)
                                .desired_width(f32::INFINITY),
                        );
                    }
                    ToolForm::Shell(form) => {
                        egui::Grid::new("tool-shell-grid")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                ui.label("enabled");
                                ui.checkbox(&mut form.enabled, "");
                                ui.end_row();

                                ui.label("workspace");
                                ui.text_edit_singleline(&mut form.workspace);
                                ui.end_row();

                                ui.label("allow_login_shell");
                                ui.checkbox(&mut form.allow_login_shell, "");
                                ui.end_row();

                                ui.label("max_timeout_ms");
                                ui.text_edit_singleline(&mut form.max_timeout_ms);
                                ui.end_row();

                                ui.label("max_output_bytes");
                                ui.text_edit_singleline(&mut form.max_output_bytes);
                                ui.end_row();
                            });

                        ui.separator();
                        ui.label("blocked_patterns (one per line)");
                        ui.add(
                            egui::TextEdit::multiline(&mut form.blocked_patterns_text)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY),
                        );

                        ui.label("unsafe_patterns (one per line)");
                        ui.add(
                            egui::TextEdit::multiline(&mut form.unsafe_patterns_text)
                                .desired_rows(5)
                                .desired_width(f32::INFINITY),
                        );
                    }
                    ToolForm::Archive(form)
                    | ToolForm::Voice(form)
                    | ToolForm::Approval(form)
                    | ToolForm::Geo(form)
                    | ToolForm::LocalSearch(form)
                    | ToolForm::TerminalMultiplexers(form)
                    | ToolForm::CronManager(form)
                    | ToolForm::HeartbeatManager(form)
                    | ToolForm::SkillsRegistry(form)
                    | ToolForm::SkillsManager(form) => {
                        ui.horizontal(|ui| {
                            ui.label("enabled");
                            ui.checkbox(&mut form.enabled, "");
                        });
                    }
                    ToolForm::ChannelAttachment(form) => {
                        egui::Grid::new("tool-channel-attachment-grid")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                ui.label("enabled");
                                ui.checkbox(&mut form.enabled, "");
                                ui.end_row();

                                ui.label("max_bytes");
                                ui.text_edit_singleline(&mut form.max_bytes);
                                ui.end_row();
                            });
                        ui.separator();
                        ui.label("allowlist (absolute paths, one per line)");
                        ui.add(
                            egui::TextEdit::multiline(&mut form.allowlist_text)
                                .desired_rows(5)
                                .desired_width(f32::INFINITY),
                        );
                    }
                    ToolForm::Memory(form) => {
                        egui::Grid::new("tool-memory-grid")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                ui.label("enabled");
                                ui.checkbox(&mut form.enabled, "");
                                ui.end_row();

                                ui.label("search_limit");
                                ui.text_edit_singleline(&mut form.search_limit);
                                ui.end_row();

                                ui.label("fts_limit");
                                ui.text_edit_singleline(&mut form.fts_limit);
                                ui.end_row();

                                ui.label("vector_limit");
                                ui.text_edit_singleline(&mut form.vector_limit);
                                ui.end_row();

                                ui.label("use_vector");
                                ui.checkbox(&mut form.use_vector, "");
                                ui.end_row();
                            });
                    }
                    ToolForm::WebFetch(form) => {
                        egui::Grid::new("tool-web-fetch-grid")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                ui.label("enabled");
                                ui.checkbox(&mut form.enabled, "");
                                ui.end_row();

                                ui.label("max_chars");
                                ui.text_edit_singleline(&mut form.max_chars);
                                ui.end_row();

                                ui.label("timeout_seconds");
                                ui.text_edit_singleline(&mut form.timeout_seconds);
                                ui.end_row();

                                ui.label("cache_ttl_minutes");
                                ui.text_edit_singleline(&mut form.cache_ttl_minutes);
                                ui.end_row();

                                ui.label("max_redirects");
                                ui.text_edit_singleline(&mut form.max_redirects);
                                ui.end_row();

                                ui.label("readability");
                                ui.checkbox(&mut form.readability, "");
                                ui.end_row();
                            });
                        ui.separator();
                        ui.label("ssrf_allowlist (CIDR/IP, one per line)");
                        ui.add(
                            egui::TextEdit::multiline(&mut form.ssrf_allowlist_text)
                                .desired_rows(5)
                                .desired_width(f32::INFINITY),
                        );
                    }
                    ToolForm::WebSearch(form) => {
                        egui::Grid::new("tool-web-search-grid")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                ui.label("enabled");
                                ui.checkbox(&mut form.enabled, "");
                                ui.end_row();

                                ui.label("provider");
                                ui.text_edit_singleline(&mut form.provider);
                                ui.end_row();
                            });

                        ui.separator();
                        ui.strong("Tavily");
                        egui::Grid::new("tool-web-search-tavily-grid")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                ui.label("base_url");
                                ui.text_edit_singleline(&mut form.tavily_base_url);
                                ui.end_row();

                                ui.label("api_key");
                                ui.text_edit_singleline(&mut form.tavily_api_key);
                                ui.end_row();

                                ui.label("env_key");
                                ui.text_edit_singleline(&mut form.tavily_env_key);
                                ui.end_row();

                                ui.label("search_depth");
                                ui.text_edit_singleline(&mut form.tavily_search_depth);
                                ui.end_row();

                                ui.label("topic");
                                ui.text_edit_singleline(&mut form.tavily_topic);
                                ui.end_row();

                                ui.label("include_answer");
                                ui.checkbox(&mut form.tavily_include_answer, "");
                                ui.end_row();

                                ui.label("include_raw_content");
                                ui.checkbox(&mut form.tavily_include_raw_content, "");
                                ui.end_row();

                                ui.label("include_images");
                                ui.checkbox(&mut form.tavily_include_images, "");
                                ui.end_row();

                                ui.label("project_id");
                                ui.text_edit_singleline(&mut form.tavily_project_id);
                                ui.end_row();
                            });

                        ui.separator();
                        ui.strong("Brave");
                        egui::Grid::new("tool-web-search-brave-grid")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                ui.label("base_url");
                                ui.text_edit_singleline(&mut form.brave_base_url);
                                ui.end_row();

                                ui.label("api_key");
                                ui.text_edit_singleline(&mut form.brave_api_key);
                                ui.end_row();

                                ui.label("env_key");
                                ui.text_edit_singleline(&mut form.brave_env_key);
                                ui.end_row();

                                ui.label("country");
                                ui.text_edit_singleline(&mut form.brave_country);
                                ui.end_row();

                                ui.label("search_lang");
                                ui.text_edit_singleline(&mut form.brave_search_lang);
                                ui.end_row();

                                ui.label("ui_lang");
                                ui.text_edit_singleline(&mut form.brave_ui_lang);
                                ui.end_row();

                                ui.label("safesearch");
                                ui.text_edit_singleline(&mut form.brave_safesearch);
                                ui.end_row();

                                ui.label("freshness");
                                ui.text_edit_singleline(&mut form.brave_freshness);
                                ui.end_row();
                            });
                    }
                    ToolForm::SubAgent(form) => {
                        egui::Grid::new("tool-sub-agent-grid")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                ui.label("enabled");
                                ui.checkbox(&mut form.enabled, "");
                                ui.end_row();

                                ui.label("max_iterations");
                                ui.text_edit_singleline(&mut form.max_iterations);
                                ui.end_row();

                                ui.label("max_tool_calls");
                                ui.text_edit_singleline(&mut form.max_tool_calls);
                                ui.end_row();

                                ui.label("inherit_parent_tools");
                                ui.checkbox(&mut form.inherit_parent_tools, "");
                                ui.end_row();
                            });
                        ui.separator();
                        ui.label("exclude_tools (one per line)");
                        ui.add(
                            egui::TextEdit::multiline(&mut form.exclude_tools_text)
                                .desired_rows(5)
                                .desired_width(f32::INFINITY),
                        );
                    }
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        save_clicked = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel_clicked = true;
                    }
                });
            });

        if save_clicked {
            self.save_form(notifications);
        }
        if cancel_clicked {
            self.form = None;
        }
    }

    fn render_inspect_window(&mut self, ui: &mut egui::Ui) {
        let Some(tool) = self
            .inspect_key
            .and_then(|key| self.tools().into_iter().find(|item| item.key == key))
        else {
            self.inspect_key = None;
            return;
        };
        let definition = self.runtime_definition(tool.key);
        let mut schema_json = definition
            .map(|item| serde_json::to_string_pretty(&item.parameters).unwrap_or_default())
            .unwrap_or_default();
        let mut json_layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
            let mut job = json_syntax_highlight_job(text.as_str());
            job.wrap.max_width = wrap_width;
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        };
        let preferred_window_height = preferred_inspect_window_height();
        let window_size = constrained_window_size(
            ui.ctx(),
            INSPECT_WINDOW_WIDTH,
            preferred_window_height.min(INSPECT_WINDOW_MAX_HEIGHT),
        );
        let inspect_body_height =
            (window_size.y - INSPECT_WINDOW_CHROME_HEIGHT).max(INSPECT_SCHEMA_HEIGHT);

        let mut open = true;
        egui::Window::new(format!("Inspect Tool: {}", tool.name))
            .id(egui::Id::new(("tool-inspect", tool.key)))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .default_width(INSPECT_WINDOW_WIDTH)
            .min_width(INSPECT_WINDOW_WIDTH)
            .max_width(INSPECT_WINDOW_WIDTH)
            .default_height(window_size.y)
            .min_height(window_size.y)
            .max_height(window_size.y)
            .show(ui.ctx(), |ui| {
                let description = definition
                    .map(|item| item.description.as_str())
                    .unwrap_or(tool.description);
                egui::ScrollArea::vertical()
                    .id_salt(("tool-inspect-body", tool.key))
                    .max_height(inspect_body_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        StripBuilder::new(ui)
                            .size(Size::exact(
                                INSPECT_DESCRIPTION_HEIGHT + INSPECT_SECTION_CHROME_HEIGHT,
                            ))
                            .size(Size::exact(
                                INSPECT_SCHEMA_HEIGHT + INSPECT_SECTION_CHROME_HEIGHT,
                            ))
                            .vertical(|mut strip| {
                                strip.cell(|ui| {
                                    ui.strong("Description");
                                    ui.add_space(6.0);
                                    egui::Frame::group(ui.style()).show(ui, |ui| {
                                        ui.set_min_height(INSPECT_DESCRIPTION_HEIGHT);
                                        ui.set_max_height(INSPECT_DESCRIPTION_HEIGHT);
                                        egui::ScrollArea::vertical()
                                            .id_salt(("tool-inspect-description", tool.key))
                                            .max_height(INSPECT_DESCRIPTION_HEIGHT)
                                            .auto_shrink([false, false])
                                            .show(ui, |ui| {
                                                ui.label(description);
                                            });
                                    });
                                });

                                strip.cell(|ui| {
                                    ui.separator();
                                    ui.strong("Schema");
                                    ui.add_space(6.0);
                                    egui::Frame::group(ui.style()).show(ui, |ui| {
                                        ui.set_min_height(INSPECT_SCHEMA_HEIGHT);
                                        ui.set_max_height(INSPECT_SCHEMA_HEIGHT);
                                        if definition.is_none() {
                                            egui::ScrollArea::vertical()
                                                .id_salt(("tool-inspect-schema-empty", tool.key))
                                                .max_height(INSPECT_SCHEMA_HEIGHT)
                                                .auto_shrink([false, false])
                                                .show(ui, |ui| {
                                                    ui.label(
                                                        "Runtime metadata unavailable for this tool.",
                                                    );
                                                });
                                            return;
                                        }

                                        egui::ScrollArea::both()
                                            .id_salt(("tool-inspect-schema", tool.key))
                                            .max_height(INSPECT_SCHEMA_HEIGHT)
                                            .auto_shrink([false, false])
                                            .show(ui, |ui| {
                                                let editor_width = ui.available_width().max(1.0);
                                                ui.add_sized(
                                                    [editor_width, INSPECT_SCHEMA_HEIGHT],
                                                    egui::TextEdit::multiline(&mut schema_json)
                                                        .desired_width(f32::INFINITY)
                                                        .font(egui::TextStyle::Monospace)
                                                        .code_editor()
                                                        .layouter(&mut json_layouter)
                                                        .interactive(false),
                                                );
                                            });
                                    });
                                });
                            });
                    });
            });

        if !open {
            self.inspect_key = None;
        }
    }

    fn refresh_tool_logs(&mut self, key: &'static str, notifications: &mut NotificationCenter) {
        let filter_query = ToolAuditFilterOptionsQuery {
            started_from_ms: self.log_start_date.and_then(date_start_ms),
            started_to_ms: self.log_end_date.and_then(date_end_ms),
        };
        let query = ToolAuditQuery {
            session_key: self.log_session_filter.clone(),
            tool_name: Some(key.to_string()),
            started_from_ms: filter_query.started_from_ms,
            started_to_ms: filter_query.started_to_ms,
            limit: TOOL_LOG_PAGE_SIZE,
            offset: 0,
            sort_order: match self.log_sort_order {
                ToolLogSortOrder::StartedAtAsc => ToolAuditSortOrder::StartedAtAsc,
                ToolLogSortOrder::StartedAtDesc => ToolAuditSortOrder::StartedAtDesc,
            },
        };
        match run_session_task(move |manager| async move {
            let filter_options = manager
                .list_tool_audit_filter_options(&filter_query)
                .await?;
            let rows = manager.list_tool_audit(&query).await?;
            Ok((filter_options, rows))
        }) {
            Ok((filter_options, rows)) => {
                self.logs_key = Some(key);
                self.log_session_options = filter_options.session_keys;
                self.log_selected_id = rows.first().map(|row| row.id.clone());
                self.log_rows = rows;
            }
            Err(err) => notifications.error(format!("Failed to load tool audit rows: {err}")),
        }
    }

    fn log_record_by_id(&self, id: &str) -> Option<&ToolAuditRecord> {
        self.log_rows.iter().find(|row| row.id == id)
    }

    fn filtered_log_rows(&self) -> Vec<ToolAuditRecord> {
        self.log_rows
            .iter()
            .filter(|row| match self.log_status_filter {
                LogStatusFilter::All => true,
                LogStatusFilter::FailedOnly => {
                    matches!(row.status, klaw_session::ToolAuditStatus::Failed)
                }
            })
            .cloned()
            .collect()
    }

    fn toggle_log_sort_order(&mut self) {
        self.log_sort_order = match self.log_sort_order {
            ToolLogSortOrder::StartedAtAsc => ToolLogSortOrder::StartedAtDesc,
            ToolLogSortOrder::StartedAtDesc => ToolLogSortOrder::StartedAtAsc,
        };
    }

    fn log_sort_label(&self) -> &'static str {
        match self.log_sort_order {
            ToolLogSortOrder::StartedAtAsc => "Time ↑",
            ToolLogSortOrder::StartedAtDesc => "Time ↓",
        }
    }

    fn render_logs_window(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        let Some(tool_key) = self.logs_key else {
            return;
        };
        let Some(tool) = self.tools().into_iter().find(|item| item.key == tool_key) else {
            self.logs_key = None;
            self.log_rows.clear();
            self.log_selected_id = None;
            self.log_summary_id = None;
            return;
        };

        let window_size = viewport_ratio_window_size(ui.ctx(), LOGS_WINDOW_VIEWPORT_RATIO);
        let mut open = true;
        egui::Window::new(format!("Tool Logs: {}", tool.name))
            .id(egui::Id::new(("tool-logs", tool.key)))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .default_width(window_size.x)
            .min_width(window_size.x)
            .max_width(window_size.x)
            .default_height(window_size.y)
            .min_height(window_size.y)
            .max_height(window_size.y)
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Refresh").clicked() {
                        self.refresh_tool_logs(tool.key, notifications);
                    }
                    ui.label(format!("Rows: {}", self.log_rows.len()));
                    ui.separator();
                    ui.label("Double-click a row or right-click for Summary.");
                });
                let mut need_refresh = false;
                ui.horizontal(|ui| {
                    ui.label("session");
                    let combo_resp =
                        egui::ComboBox::from_id_salt(("tool-log-session-filter", tool.key))
                            .selected_text(self.log_session_filter.as_deref().unwrap_or("All"))
                            .width(220.0)
                            .show_ui(ui, |ui| {
                                let mut changed = false;
                                if ui
                                    .selectable_value(&mut self.log_session_filter, None, "All")
                                    .changed()
                                {
                                    changed = true;
                                }
                                for session_key in &self.log_session_options {
                                    if ui
                                        .selectable_value(
                                            &mut self.log_session_filter,
                                            Some(session_key.clone()),
                                            session_key,
                                        )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                }
                                changed
                            });
                    if combo_resp.inner.unwrap_or(false) {
                        need_refresh = true;
                    }

                    ui.label("start");
                    if render_date_picker(ui, &mut self.log_start_date, "tool-log-start-date") {
                        need_refresh = true;
                    }
                    ui.label("end");
                    if render_date_picker(ui, &mut self.log_end_date, "tool-log-end-date") {
                        need_refresh = true;
                    }

                    ui.label("status");
                    egui::ComboBox::from_id_salt(("tool-log-status-filter", tool.key))
                        .selected_text(match self.log_status_filter {
                            LogStatusFilter::All => "All",
                            LogStatusFilter::FailedOnly => "Failed only",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.log_status_filter,
                                LogStatusFilter::All,
                                "All",
                            );
                            ui.selectable_value(
                                &mut self.log_status_filter,
                                LogStatusFilter::FailedOnly,
                                "Failed only",
                            );
                        });
                });
                ui.separator();
                if need_refresh {
                    self.refresh_tool_logs(tool.key, notifications);
                }
                let filtered_rows = self.filtered_log_rows();
                if filtered_rows.is_empty() {
                    ui.label("No tool audit rows found.");
                    return;
                }
                egui::ScrollArea::both()
                    .id_salt(("tool-logs-list", tool.key))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let row_height = ui.spacing().interact_size.y;
                        TableBuilder::new(ui)
                            .striped(true)
                            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                            .column(Column::auto().at_least(150.0))
                            .column(Column::auto().at_least(90.0))
                            .column(Column::auto().at_least(80.0))
                            .column(Column::auto().at_least(80.0))
                            .column(Column::remainder().at_least(120.0))
                            .sense(egui::Sense::click())
                            .header(24.0, |mut header| {
                                header.col(|ui| {
                                    if ui.button(self.log_sort_label()).clicked() {
                                        self.toggle_log_sort_order();
                                        self.refresh_tool_logs(tool.key, notifications);
                                    }
                                });
                                header.col(|ui| {
                                    ui.strong("Tool Call ID");
                                });
                                header.col(|ui| {
                                    ui.strong("Status");
                                });
                                header.col(|ui| {
                                    ui.strong("Seq");
                                });
                                header.col(|ui| {
                                    ui.strong("Session");
                                });
                            })
                            .body(|body| {
                                body.rows(row_height, filtered_rows.len(), |mut row| {
                                    let audit = &filtered_rows[row.index()];
                                    let selected =
                                        self.log_selected_id.as_deref() == Some(audit.id.as_str());
                                    row.set_selected(selected);

                                    row.col(|ui| {
                                        ui.label(format_timestamp_millis(audit.started_at_ms));
                                    });
                                    row.col(|ui| {
                                        ui.monospace(tool_call_id_label(audit));
                                    });
                                    row.col(|ui| {
                                        let failed = matches!(
                                            audit.status,
                                            klaw_session::ToolAuditStatus::Failed
                                        );
                                        render_boolean_status(ui, !failed, "Success", "Failed");
                                    });
                                    row.col(|ui| {
                                        ui.monospace(format!(
                                            "{}/{}",
                                            audit.request_seq, audit.tool_call_seq
                                        ));
                                    });
                                    row.col(|ui| {
                                        ui.label(&audit.session_key);
                                    });

                                    let row_response = row.response();
                                    if row_response.double_clicked() {
                                        self.log_selected_id = Some(audit.id.clone());
                                        self.log_summary_id = Some(audit.id.clone());
                                        self.log_summary_tab = ToolLogSummaryTab::Arguments;
                                    } else if row_response.clicked()
                                        || row_response.secondary_clicked()
                                    {
                                        self.log_selected_id = Some(audit.id.clone());
                                    }
                                    row_response.context_menu(|ui| {
                                        if ui.button(format!("{} Summary", regular::EYE)).clicked()
                                        {
                                            self.log_selected_id = Some(audit.id.clone());
                                            self.log_summary_id = Some(audit.id.clone());
                                            self.log_summary_tab = ToolLogSummaryTab::Arguments;
                                            ui.close();
                                        }
                                    });
                                });
                            });
                    });
            });

        if !open {
            self.logs_key = None;
            self.log_rows.clear();
            self.log_selected_id = None;
            self.log_summary_id = None;
            self.log_summary_tab = ToolLogSummaryTab::Arguments;
            self.log_session_options.clear();
            self.log_session_filter = None;
            self.log_status_filter = LogStatusFilter::All;
            self.log_sort_order = ToolLogSortOrder::StartedAtDesc;
        }
    }

    fn render_log_summary_window(&mut self, ui: &mut egui::Ui) {
        let Some(summary_id) = self.log_summary_id.clone() else {
            return;
        };
        let Some(audit) = self.log_record_by_id(&summary_id).cloned() else {
            self.log_summary_id = None;
            return;
        };

        let desired_height = preferred_tool_log_summary_window_height(&audit);
        let window_size = constrained_window_size(
            ui.ctx(),
            LOGS_SUMMARY_WINDOW_WIDTH,
            desired_height.min(LOGS_SUMMARY_WINDOW_HEIGHT),
        );
        let summary_body_height = (window_size.y - LOGS_SUMMARY_WINDOW_CHROME_HEIGHT)
            .max(LOG_DETAIL_SECTION_BLOCK_HEIGHT);
        let mut open = true;
        egui::Window::new(format!("Tool Log Summary: {}", audit.tool_name))
            .id(egui::Id::new((
                "tool-log-summary-window",
                audit.id.as_str(),
            )))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .default_width(window_size.x)
            .min_width(window_size.x)
            .max_width(window_size.x)
            .default_height(window_size.y)
            .min_height(window_size.y)
            .max_height(window_size.y)
            .show(ui.ctx(), |ui| {
                ui.label("Arguments / Result / Metadata supports tab switching.");
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt(("tool-log-summary-body", audit.id.as_str()))
                    .max_height(summary_body_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        render_tool_log_summary(ui, &audit, &mut self.log_summary_tab);
                    });
            });

        if !open {
            self.log_summary_id = None;
            self.log_summary_tab = ToolLogSummaryTab::Arguments;
        }
    }
}

fn render_boolean_status(
    ui: &mut egui::Ui,
    enabled: bool,
    enabled_label: &str,
    disabled_label: &str,
) {
    let (icon, color, label) = if enabled {
        (
            regular::CHECK_CIRCLE,
            Color32::from_rgb(0x22, 0xC5, 0x5E),
            enabled_label,
        )
    } else {
        (
            regular::X_CIRCLE,
            ui.visuals().error_fg_color,
            disabled_label,
        )
    };
    ui.horizontal(|ui| {
        ui.colored_label(color, icon);
        ui.colored_label(color, label);
    });
}

impl PanelRenderer for ToolPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_store_loaded(notifications);

        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            ui.label("Manage tool enablement and per-tool settings.");
            if ui.button("Reload").clicked() {
                self.reload(notifications);
            }
        });
        ui.separator();

        let tools = self.tools();
        let mut edit_key: Option<&'static str> = None;
        let mut inspect_key: Option<&'static str> = None;
        let mut logs_key: Option<&'static str> = None;
        let table_width = ui.available_width();
        let table_height = ui.available_height();
        let row_height = ui.spacing().interact_size.y;

        egui::ScrollArea::both()
            .id_salt("tool-table-scroll")
            .auto_shrink([false, false])
            .max_width(table_width)
            .max_height(table_height)
            .show(ui, |ui| {
                ui.set_min_width(table_width);
                TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::auto().at_least(150.0))
                    .column(Column::auto().at_least(90.0))
                    .column(Column::remainder().at_least(320.0))
                    .min_scrolled_height(table_height)
                    .max_scroll_height(table_height)
                    .sense(egui::Sense::click())
                    .header(24.0, |mut header| {
                        header.col(|ui| {
                            ui.strong("Tool");
                        });
                        header.col(|ui| {
                            ui.strong("Status");
                        });
                        header.col(|ui| {
                            ui.strong("Description");
                        });
                    })
                    .body(|body| {
                        body.rows(row_height, tools.len(), |mut row| {
                            let tool = tools[row.index()];
                            row.col(|ui| {
                                ui.monospace(tool.name);
                            });
                            row.col(|ui| {
                                render_boolean_status(ui, tool.enabled, "Enabled", "Disabled");
                            });
                            row.col(|ui| {
                                ui.label(
                                    self.runtime_definition(tool.key)
                                        .map(|item| item.description.as_str())
                                        .unwrap_or(tool.description),
                                );
                            });

                            let response = row.response();
                            response.context_menu(|ui| {
                                if ui
                                    .button(format!("{} Edit", regular::PENCIL_SIMPLE))
                                    .clicked()
                                {
                                    edit_key = Some(tool.key);
                                    ui.close();
                                }
                                if ui.button(format!("{} Inspect", regular::EYE)).clicked() {
                                    inspect_key = Some(tool.key);
                                    ui.close();
                                }
                                if ui
                                    .button(format!("{} Logs", regular::LIST_BULLETS))
                                    .clicked()
                                {
                                    logs_key = Some(tool.key);
                                    ui.close();
                                }
                            });
                        });
                    });
            });

        if let Some(key) = edit_key {
            self.open_editor(key);
        }
        if let Some(key) = inspect_key {
            self.inspect_key = Some(key);
        }
        if let Some(key) = logs_key {
            self.refresh_tool_logs(key, notifications);
        }

        self.render_form_window(ui, notifications);
        self.render_inspect_window(ui);
        self.render_logs_window(ui, notifications);
        self.render_log_summary_window(ui);
    }
}

fn json_syntax_highlight_job(code: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    let bytes_len = code.len();
    let mut idx = 0;

    while idx < bytes_len {
        let ch = code[idx..].chars().next().unwrap_or_default();

        if ch.is_whitespace() {
            let start = idx;
            idx += ch.len_utf8();
            while idx < bytes_len {
                let next = code[idx..].chars().next().unwrap_or_default();
                if !next.is_whitespace() {
                    break;
                }
                idx += next.len_utf8();
            }
            job.append(&code[start..idx], 0.0, json_fmt_default());
            continue;
        }

        if ch == '"' {
            let start = idx;
            idx += ch.len_utf8();
            let mut escaped = false;
            while idx < bytes_len {
                let next = code[idx..].chars().next().unwrap_or_default();
                idx += next.len_utf8();
                if next == '\\' && !escaped {
                    escaped = true;
                    continue;
                }
                if next == '"' && !escaped {
                    break;
                }
                escaped = false;
            }

            let token = &code[start..idx];
            let mut lookahead = idx;
            while lookahead < bytes_len {
                let next = code[lookahead..].chars().next().unwrap_or_default();
                if next.is_whitespace() {
                    lookahead += next.len_utf8();
                    continue;
                }
                break;
            }
            let fmt = if lookahead < bytes_len && code[lookahead..].starts_with(':') {
                json_fmt_key()
            } else {
                json_fmt_string()
            };
            job.append(token, 0.0, fmt);
            continue;
        }

        if ch.is_ascii_digit() || ch == '-' {
            let start = idx;
            idx += ch.len_utf8();
            while idx < bytes_len {
                let next = code[idx..].chars().next().unwrap_or_default();
                if !(next.is_ascii_digit() || matches!(next, '.' | 'e' | 'E' | '+' | '-')) {
                    break;
                }
                idx += next.len_utf8();
            }
            job.append(&code[start..idx], 0.0, json_fmt_number());
            continue;
        }

        if ch.is_ascii_alphabetic() {
            let start = idx;
            idx += ch.len_utf8();
            while idx < bytes_len {
                let next = code[idx..].chars().next().unwrap_or_default();
                if !next.is_ascii_alphabetic() {
                    break;
                }
                idx += next.len_utf8();
            }
            let token = &code[start..idx];
            let fmt = match token {
                "true" | "false" => json_fmt_bool(),
                "null" => json_fmt_null(),
                _ => json_fmt_default(),
            };
            job.append(token, 0.0, fmt);
            continue;
        }

        let next_idx = idx + ch.len_utf8();
        let fmt = match ch {
            '{' | '}' | '[' | ']' | ':' | ',' => json_fmt_punct(),
            _ => json_fmt_default(),
        };
        job.append(&code[idx..next_idx], 0.0, fmt);
        idx = next_idx;
    }

    if code.is_empty() {
        job.append("", 0.0, json_fmt_default());
    }

    job
}

fn json_fmt_default() -> TextFormat {
    TextFormat::simple(FontId::monospace(13.0), Color32::LIGHT_GRAY)
}

fn json_fmt_key() -> TextFormat {
    TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(242, 201, 76))
}

fn json_fmt_string() -> TextFormat {
    TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(158, 212, 158))
}

fn json_fmt_number() -> TextFormat {
    TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(255, 170, 120))
}

fn json_fmt_bool() -> TextFormat {
    TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(214, 154, 255))
}

fn json_fmt_null() -> TextFormat {
    TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(125, 174, 241))
}

fn json_fmt_punct() -> TextFormat {
    TextFormat::simple(FontId::monospace(13.0), Color32::from_rgb(210, 210, 210))
}

fn parse_lines(value: &str) -> Vec<String> {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn optional_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn render_tool_log_summary(
    ui: &mut egui::Ui,
    audit: &ToolAuditRecord,
    active_tab: &mut ToolLogSummaryTab,
) {
    ui.strong("Summary");
    egui::Grid::new(("tool-log-summary", audit.id.as_str()))
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.label("tool");
            ui.monospace(&audit.tool_name);
            ui.end_row();
            ui.label("session");
            ui.monospace(&audit.session_key);
            ui.end_row();
            ui.label("chat");
            ui.monospace(&audit.chat_id);
            ui.end_row();
            ui.label("status");
            ui.monospace(match audit.status {
                klaw_session::ToolAuditStatus::Success => "success",
                klaw_session::ToolAuditStatus::Failed => "failed",
            });
            ui.end_row();
            ui.label("seq");
            ui.monospace(format!("{}/{}", audit.request_seq, audit.tool_call_seq));
            ui.end_row();
            ui.label("started");
            ui.monospace(format_timestamp_millis(audit.started_at_ms));
            ui.end_row();
            ui.label("duration");
            ui.monospace(format!(
                "{} ms",
                audit.finished_at_ms.saturating_sub(audit.started_at_ms)
            ));
            ui.end_row();
            ui.label("error_code");
            ui.monospace(audit.error_code.as_deref().unwrap_or("-"));
            ui.end_row();
        });
    ui.separator();

    ui.horizontal(|ui| {
        ui.selectable_value(active_tab, ToolLogSummaryTab::Arguments, "Arguments");
        ui.selectable_value(active_tab, ToolLogSummaryTab::Result, "Result");
        ui.selectable_value(active_tab, ToolLogSummaryTab::Metadata, "Metadata");
    });
    ui.separator();

    match active_tab {
        ToolLogSummaryTab::Arguments => render_json_section(
            ui,
            "Arguments",
            &audit.arguments_json,
            &format!("tool-log-arguments:{}", audit.id),
        ),
        ToolLogSummaryTab::Result => render_text_section(
            ui,
            "Result",
            &audit.result_content,
            &format!("tool-log-result:{}", audit.id),
        ),
        ToolLogSummaryTab::Metadata => render_optional_json_section(
            ui,
            "Metadata",
            audit.metadata_json.as_deref(),
            &format!("tool-log-metadata:{}", audit.id),
        ),
    }

    if let Some(error_message) = audit.error_message.as_deref() {
        ui.separator();
        render_text_section(
            ui,
            "Error",
            error_message,
            &format!("tool-log-error:{}", audit.id),
        );
    }
    if let Some(raw) = audit.error_details_json.as_deref() {
        ui.separator();
        render_json_section(
            ui,
            "Error Details",
            raw,
            &format!("tool-log-error-details:{}", audit.id),
        );
    }
    if let Some(raw) = audit.signals_json.as_deref() {
        ui.separator();
        render_json_section(
            ui,
            "Signals",
            raw,
            &format!("tool-log-signals:{}", audit.id),
        );
    }
}

fn render_text_section(ui: &mut egui::Ui, title: &str, body: &str, scroll_id: &str) {
    ui.strong(title);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let section_size = egui::vec2(ui.available_width(), LOG_DETAIL_SECTION_HEIGHT);
        ui.set_min_height(LOG_DETAIL_SECTION_HEIGHT);
        ui.set_max_height(LOG_DETAIL_SECTION_HEIGHT);
        ui.allocate_ui_with_layout(
            section_size,
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::ScrollArea::both()
                    .id_salt(scroll_id)
                    .max_height(LOG_DETAIL_SECTION_HEIGHT)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.code(body);
                    });
            },
        );
    });
}

fn tool_call_id_label(audit: &ToolAuditRecord) -> String {
    audit
        .metadata_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|value| {
            value
                .get("tool_call_id")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "-".to_string())
}

fn render_json_section(ui: &mut egui::Ui, title: &str, raw: &str, scroll_id: &str) {
    ui.strong(title);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let section_size = egui::vec2(ui.available_width(), LOG_DETAIL_SECTION_HEIGHT);
        ui.set_min_height(LOG_DETAIL_SECTION_HEIGHT);
        ui.set_max_height(LOG_DETAIL_SECTION_HEIGHT);
        ui.allocate_ui_with_layout(
            section_size,
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::ScrollArea::both()
                    .id_salt(scroll_id)
                    .max_height(LOG_DETAIL_SECTION_HEIGHT)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        match serde_json::from_str::<serde_json::Value>(raw) {
                            Ok(value) => show_json_tree_with_id(ui, &value, scroll_id),
                            Err(_) => {
                                ui.code(raw);
                            }
                        }
                    });
            },
        );
    });
}

fn render_optional_json_section(
    ui: &mut egui::Ui,
    title: &str,
    raw: Option<&str>,
    scroll_id: &str,
) {
    match raw {
        Some(raw) => render_json_section(ui, title, raw, scroll_id),
        None => render_text_section(ui, title, "<empty>", scroll_id),
    }
}

fn preferred_inspect_window_height() -> f32 {
    INSPECT_WINDOW_CHROME_HEIGHT
        + INSPECT_DESCRIPTION_HEIGHT
        + INSPECT_SCHEMA_HEIGHT
        + INSPECT_SECTION_CHROME_HEIGHT * 2.0
}

fn preferred_tool_log_summary_window_height(audit: &ToolAuditRecord) -> f32 {
    let mut section_count = 1.0;
    if audit.error_message.is_some() {
        section_count += 1.0;
    }
    if audit.error_details_json.is_some() {
        section_count += 1.0;
    }
    if audit.signals_json.is_some() {
        section_count += 1.0;
    }

    LOGS_SUMMARY_WINDOW_CHROME_HEIGHT
        + LOGS_SUMMARY_STATIC_HEIGHT
        + section_count * LOG_DETAIL_SECTION_BLOCK_HEIGHT
}

fn constrained_window_size(
    ctx: &egui::Context,
    desired_width: f32,
    desired_height: f32,
) -> egui::Vec2 {
    let (viewport_width, viewport_height) = ctx.input(|input| {
        input
            .viewport()
            .inner_rect
            .map(|rect| (rect.width(), rect.height()))
            .unwrap_or((desired_width, desired_height))
    });
    egui::vec2(
        desired_width.min((viewport_width - LOG_WINDOW_VIEWPORT_MARGIN).max(320.0)),
        desired_height.min((viewport_height - LOG_WINDOW_VIEWPORT_MARGIN).max(320.0)),
    )
}

fn viewport_ratio_window_size(ctx: &egui::Context, ratio: f32) -> egui::Vec2 {
    let (viewport_width, viewport_height) = ctx.input(|input| {
        input
            .viewport()
            .inner_rect
            .map(|rect| (rect.width(), rect.height()))
            .unwrap_or((720.0, 540.0))
    });
    egui::vec2(
        (viewport_width * ratio).max(320.0),
        (viewport_height * ratio).max(320.0),
    )
}

fn run_session_task<T, F, Fut>(op: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(SqliteSessionManager) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, klaw_session::SessionError>> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let result = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("tokio runtime build failed: {err}"))
            .and_then(|runtime| {
                runtime.block_on(async {
                    let manager = SqliteSessionManager::open_default()
                        .await
                        .map_err(|err| format!("failed to open session store: {err}"))?;
                    op(manager)
                        .await
                        .map_err(|err| format!("session task failed: {err}"))
                })
            });
        let _ = tx.send(result);
    });
    rx.recv()
        .map_err(|_| "session task thread closed".to_string())?
}

fn render_date_picker(ui: &mut egui::Ui, value: &mut Option<NaiveDate>, id: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        if let Some(date) = value.as_mut() {
            if ui
                .add(DatePickerButton::new(date).id_salt(id).format("%Y/%m/%d"))
                .changed()
            {
                changed = true;
            }
            if ui.small_button("×").clicked() {
                *value = None;
                changed = true;
            }
        }
    });
    changed
}

fn date_start_ms(date: NaiveDate) -> Option<i64> {
    date_boundary_ms(date, Time::MIDNIGHT)
}

fn date_end_ms(date: NaiveDate) -> Option<i64> {
    let time = Time::from_hms_milli(23, 59, 59, 999).ok()?;
    date_boundary_ms(date, time)
}

fn date_boundary_ms(date: NaiveDate, time: Time) -> Option<i64> {
    let month = Month::try_from(date.month() as u8).ok()?;
    let date = time::Date::from_calendar_date(date.year(), month, date.day() as u8).ok()?;
    let datetime = PrimitiveDateTime::new(date, time).assume_utc();
    Some(offset_to_ms(datetime))
}

fn offset_to_ms(datetime: OffsetDateTime) -> i64 {
    datetime.unix_timestamp_nanos().saturating_div(1_000_000) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lines_ignores_blanks() {
        let lines = parse_lines("a\n\n b \n  ");
        assert_eq!(lines, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn shell_form_rejects_invalid_numbers() {
        let form = ShellForm {
            enabled: true,
            workspace: String::new(),
            blocked_patterns_text: String::new(),
            unsafe_patterns_text: String::new(),
            allow_login_shell: true,
            max_timeout_ms: "abc".to_string(),
            max_output_bytes: "1024".to_string(),
        };

        let err = form.to_config().expect_err("should reject invalid timeout");
        assert!(err.contains("max_timeout_ms"));
    }

    #[test]
    fn channel_attachment_form_parses_values() {
        let form = ChannelAttachmentForm {
            enabled: true,
            allowlist_text: "/tmp\n /Users/zhubby/Downloads ".to_string(),
            max_bytes: "2048".to_string(),
        };

        let config = form.to_config().expect("should parse");
        assert!(config.enabled);
        assert_eq!(config.local_attachments.max_bytes, 2048);
        assert_eq!(
            config.local_attachments.allowlist,
            vec!["/tmp".to_string(), "/Users/zhubby/Downloads".to_string()]
        );
    }

    #[test]
    fn sub_agent_form_parses_values() {
        let form = SubAgentForm {
            enabled: true,
            max_iterations: "6".to_string(),
            max_tool_calls: "12".to_string(),
            inherit_parent_tools: true,
            exclude_tools_text: "sub_agent\nweb_search".to_string(),
        };

        let config = form.to_config().expect("should parse");
        assert_eq!(config.max_iterations, 6);
        assert_eq!(config.max_tool_calls, 12);
        assert_eq!(
            config.exclude_tools,
            vec!["sub_agent".to_string(), "web_search".to_string()]
        );
    }

    #[test]
    fn tool_descriptors_are_sorted_alphabetically() {
        let panel = ToolPanel::default();
        let tools = panel.tools();
        let names = tools.iter().map(|tool| tool.name).collect::<Vec<_>>();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }
}
