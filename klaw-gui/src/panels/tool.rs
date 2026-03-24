use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use klaw_config::{
    AppConfig, ApplyPatchConfig, ConfigSnapshot, ConfigStore, MemoryToolConfig, ShellConfig,
    SubAgentConfig, WebFetchConfig, WebSearchConfig,
};
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct ToolPanel {
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    revision: Option<u64>,
    config: AppConfig,
    form: Option<ToolForm>,
}

#[derive(Debug, Clone)]
enum ToolForm {
    ApplyPatch(ApplyPatchForm),
    Shell(ShellForm),
    Archive(ToggleForm),
    Voice(ToggleForm),
    Approval(ToggleForm),
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
            | ToolForm::LocalSearch(form)
            | ToolForm::TerminalMultiplexers(form)
            | ToolForm::CronManager(form)
            | ToolForm::HeartbeatManager(form)
            | ToolForm::SkillsRegistry(form)
            | ToolForm::SkillsManager(form) => form.title,
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
                notifications.success("Tool config loaded from disk");
            }
            Err(err) => notifications.error(format!("Failed to load config: {err}")),
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config_path = Some(snapshot.path);
        self.revision = Some(snapshot.revision);
        self.config = snapshot.config;
    }

    fn status_label(path: Option<&Path>) -> String {
        match path {
            Some(path) => format!("Path: {}", path.display()),
            None => "Path: (not loaded)".to_string(),
        }
    }

    fn save_config(
        &mut self,
        next: AppConfig,
        notifications: &mut NotificationCenter,
        success_message: &str,
    ) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };
        match toml::to_string_pretty(&next) {
            Ok(raw) => match store.save_raw_toml(&raw) {
                Ok(snapshot) => {
                    self.apply_snapshot(snapshot);
                    notifications.success(success_message);
                }
                Err(err) => notifications.error(format!("Save failed: {err}")),
            },
            Err(err) => notifications.error(format!("Failed to render config TOML: {err}")),
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
                notifications.success("Configuration reloaded from disk");
            }
            Err(err) => notifications.error(format!("Reload failed: {err}")),
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

    fn open_toggle(&mut self, key: &str, title: &'static str, enabled: bool) {
        let form = ToggleForm { title, enabled };
        self.form = Some(match key {
            "archive" => ToolForm::Archive(form),
            "voice" => ToolForm::Voice(form),
            "approval" => ToolForm::Approval(form),
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
        let mut next = self.config.clone();

        let apply_result = match form {
            ToolForm::ApplyPatch(form) => {
                next.tools.apply_patch = form.to_config();
                Ok(())
            }
            ToolForm::Shell(form) => match form.to_config() {
                Ok(value) => {
                    next.tools.shell = value;
                    Ok(())
                }
                Err(err) => Err(err),
            },
            ToolForm::Archive(form) => {
                next.tools.archive.enabled = form.enabled;
                Ok(())
            }
            ToolForm::Voice(form) => {
                next.tools.voice.enabled = form.enabled;
                Ok(())
            }
            ToolForm::Approval(form) => {
                next.tools.approval.enabled = form.enabled;
                Ok(())
            }
            ToolForm::LocalSearch(form) => {
                next.tools.local_search.enabled = form.enabled;
                Ok(())
            }
            ToolForm::TerminalMultiplexers(form) => {
                next.tools.terminal_multiplexers.enabled = form.enabled;
                Ok(())
            }
            ToolForm::CronManager(form) => {
                next.tools.cron_manager.enabled = form.enabled;
                Ok(())
            }
            ToolForm::HeartbeatManager(form) => {
                next.tools.heartbeat_manager.enabled = form.enabled;
                Ok(())
            }
            ToolForm::SkillsRegistry(form) => {
                next.tools.skills_registry.enabled = form.enabled;
                Ok(())
            }
            ToolForm::SkillsManager(form) => {
                next.tools.skills_manager.enabled = form.enabled;
                Ok(())
            }
            ToolForm::Memory(form) => match form.to_config() {
                Ok(value) => {
                    next.tools.memory = value;
                    Ok(())
                }
                Err(err) => Err(err),
            },
            ToolForm::WebFetch(form) => match form.to_config() {
                Ok(value) => {
                    next.tools.web_fetch = value;
                    Ok(())
                }
                Err(err) => Err(err),
            },
            ToolForm::WebSearch(form) => {
                form.apply_to(&mut next.tools.web_search);
                Ok(())
            }
            ToolForm::SubAgent(form) => match form.to_config() {
                Ok(value) => {
                    next.tools.sub_agent = value;
                    Ok(())
                }
                Err(err) => Err(err),
            },
        };

        match apply_result {
            Ok(()) => {
                self.save_config(next, notifications, "Tool config saved");
                self.form = None;
            }
            Err(err) => notifications.error(err),
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

    fn render_tool_card(ui: &mut egui::Ui, name: &str, description: &str, enabled: bool) -> bool {
        let mut edit_clicked = false;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_width(320.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.strong(name);
                    ui.add_space(8.0);
                    let status = if enabled { "enabled" } else { "disabled" };
                    let color = if enabled {
                        egui::Color32::LIGHT_GREEN
                    } else {
                        egui::Color32::LIGHT_RED
                    };
                    ui.colored_label(color, status);
                });
                ui.add_space(4.0);
                ui.label(description);
                ui.add_space(8.0);
                if ui.button("Edit").clicked() {
                    edit_clicked = true;
                }
            });
        });
        edit_clicked
    }
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
        ui.label(Self::status_label(self.config_path.as_deref()));
        ui.horizontal(|ui| {
            ui.label(format!("Revision: {}", self.revision.unwrap_or_default()));
            ui.label("Manage tool enablement and per-tool settings.");
            if ui.button("Reload").clicked() {
                self.reload(notifications);
            }
        });
        ui.separator();

        let mut edit_key: Option<&'static str> = None;

        egui::ScrollArea::vertical()
            .id_salt("tool-card-scroll")
            .show(ui, |ui| {
                let cards: [(&str, &str, bool, &str); 15] = [
                    (
                        "apply_patch",
                        "Patch workspace files with constrained path policy.",
                        self.config.tools.apply_patch.enabled,
                        "apply_patch",
                    ),
                    (
                        "shell",
                        "Execute local shell commands with approval policy.",
                        self.config.tools.shell.enabled,
                        "shell",
                    ),
                    (
                        "archive",
                        "Manage archived attachments from conversations.",
                        self.config.tools.archive.enabled,
                        "archive",
                    ),
                    (
                        "voice",
                        "Transcribe audio and synthesize text into speech.",
                        self.config.tools.voice.enabled,
                        "voice",
                    ),
                    (
                        "approval",
                        "Manage approval lifecycle for high-risk actions.",
                        self.config.tools.approval.enabled,
                        "approval",
                    ),
                    (
                        "local_search",
                        "Search local workspace files and snippets.",
                        self.config.tools.local_search.enabled,
                        "local_search",
                    ),
                    (
                        "terminal_multiplexers",
                        "Operate tmux/zellij sessions for long-running tasks.",
                        self.config.tools.terminal_multiplexers.enabled,
                        "terminal_multiplexers",
                    ),
                    (
                        "cron_manager",
                        "Create and control scheduled cron jobs.",
                        self.config.tools.cron_manager.enabled,
                        "cron_manager",
                    ),
                    (
                        "heartbeat_manager",
                        "Manage session-bound heartbeat jobs.",
                        self.config.tools.heartbeat_manager.enabled,
                        "heartbeat_manager",
                    ),
                    (
                        "skills_registry",
                        "Browse and inspect read-only registry catalogs.",
                        self.config.tools.skills_registry.enabled,
                        "skills_registry",
                    ),
                    (
                        "skills_manager",
                        "Install, uninstall, and load installed skills.",
                        self.config.tools.skills_manager.enabled,
                        "skills_manager",
                    ),
                    (
                        "memory",
                        "Persist and retrieve long-term memory records.",
                        self.config.tools.memory.enabled,
                        "memory",
                    ),
                    (
                        "web_fetch",
                        "Fetch and extract web page content safely.",
                        self.config.tools.web_fetch.enabled,
                        "web_fetch",
                    ),
                    (
                        "web_search",
                        "Search web results via configured provider.",
                        self.config.tools.web_search.enabled,
                        "web_search",
                    ),
                    (
                        "sub_agent",
                        "Delegate focused tasks to a bounded child agent.",
                        self.config.tools.sub_agent.enabled,
                        "sub_agent",
                    ),
                ];

                let min_card_width = 340.0_f32;
                let available_width = ui.available_width().max(min_card_width);
                let columns = (available_width / min_card_width).floor().max(1.0) as usize;
                egui::Grid::new("tool-card-grid")
                    .num_columns(columns)
                    .spacing([12.0, 12.0])
                    .show(ui, |ui| {
                        for (idx, (name, description, enabled, key)) in cards.iter().enumerate() {
                            if Self::render_tool_card(ui, name, description, *enabled) {
                                edit_key = Some(key);
                            }
                            let is_row_end = (idx + 1) % columns == 0;
                            let is_last = idx + 1 == cards.len();
                            if is_row_end || is_last {
                                ui.end_row();
                            }
                        }
                    });
            });

        match edit_key {
            Some("apply_patch") => self.open_apply_patch(),
            Some("shell") => self.open_shell(),
            Some("archive") => self.open_toggle(
                "archive",
                "Edit Tool: archive",
                self.config.tools.archive.enabled,
            ),
            Some("voice") => {
                self.open_toggle("voice", "Edit Tool: voice", self.config.tools.voice.enabled)
            }
            Some("approval") => self.open_toggle(
                "approval",
                "Edit Tool: approval",
                self.config.tools.approval.enabled,
            ),
            Some("local_search") => self.open_toggle(
                "local_search",
                "Edit Tool: local_search",
                self.config.tools.local_search.enabled,
            ),
            Some("terminal_multiplexers") => self.open_toggle(
                "terminal_multiplexers",
                "Edit Tool: terminal_multiplexers",
                self.config.tools.terminal_multiplexers.enabled,
            ),
            Some("cron_manager") => self.open_toggle(
                "cron_manager",
                "Edit Tool: cron_manager",
                self.config.tools.cron_manager.enabled,
            ),
            Some("heartbeat_manager") => self.open_toggle(
                "heartbeat_manager",
                "Edit Tool: heartbeat_manager",
                self.config.tools.heartbeat_manager.enabled,
            ),
            Some("skills_registry") => self.open_toggle(
                "skills_registry",
                "Edit Tool: skills_registry",
                self.config.tools.skills_registry.enabled,
            ),
            Some("skills_manager") => self.open_toggle(
                "skills_manager",
                "Edit Tool: skills_manager",
                self.config.tools.skills_manager.enabled,
            ),
            Some("memory") => self.open_memory(),
            Some("web_fetch") => self.open_web_fetch(),
            Some("web_search") => self.open_web_search(),
            Some("sub_agent") => self.open_sub_agent(),
            _ => {}
        }

        self.render_form_window(ui, notifications);
    }
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
}
