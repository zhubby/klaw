use crate::{AppConfig, BraveWebSearchConfig, ConfigError, McpServerMode, TavilyWebSearchConfig};

pub(crate) fn validate(config: &AppConfig) -> Result<(), ConfigError> {
    if config.model_provider.trim().is_empty() {
        return Err(ConfigError::InvalidConfig(
            "model_provider cannot be empty".to_string(),
        ));
    }

    let active = config
        .model_providers
        .get(&config.model_provider)
        .ok_or_else(|| {
            ConfigError::InvalidConfig(format!(
                "model_provider '{}' not found in model_providers",
                config.model_provider
            ))
        })?;

    if active.base_url.trim().is_empty() {
        return Err(ConfigError::InvalidConfig(format!(
            "provider '{}' base_url cannot be empty",
            config.model_provider
        )));
    }
    if active.default_model.trim().is_empty() {
        return Err(ConfigError::InvalidConfig(format!(
            "provider '{}' default_model cannot be empty",
            config.model_provider
        )));
    }
    if active.wire_api.trim().is_empty() {
        return Err(ConfigError::InvalidConfig(format!(
            "provider '{}' wire_api cannot be empty",
            config.model_provider
        )));
    }

    if config.memory.embedding.enabled {
        if config.memory.embedding.provider.trim().is_empty() {
            return Err(ConfigError::InvalidConfig(
                "memory.embedding.provider cannot be empty when memory.embedding.enabled=true"
                    .to_string(),
            ));
        }
        if config.memory.embedding.model.trim().is_empty() {
            return Err(ConfigError::InvalidConfig(
                "memory.embedding.model cannot be empty when memory.embedding.enabled=true"
                    .to_string(),
            ));
        }
        if !config
            .model_providers
            .contains_key(&config.memory.embedding.provider)
        {
            return Err(ConfigError::InvalidConfig(format!(
                "memory.embedding.provider '{}' not found in model_providers",
                config.memory.embedding.provider
            )));
        }
    }

    if config.mcp.startup_timeout_seconds == 0 {
        return Err(ConfigError::InvalidConfig(
            "mcp.startup_timeout_seconds must be greater than 0".to_string(),
        ));
    }
    let mut mcp_ids = std::collections::BTreeSet::new();
    for server in &config.mcp.servers {
        if server.id.trim().is_empty() {
            return Err(ConfigError::InvalidConfig(
                "mcp.servers.id cannot be empty".to_string(),
            ));
        }
        if !mcp_ids.insert(server.id.trim().to_string()) {
            return Err(ConfigError::InvalidConfig(format!(
                "mcp.servers contains duplicated id '{}'",
                server.id
            )));
        }
        match server.mode {
            McpServerMode::Stdio => {
                let command = server.command.as_deref().map(str::trim).unwrap_or_default();
                if command.is_empty() {
                    return Err(ConfigError::InvalidConfig(format!(
                        "mcp.servers '{}' requires non-empty command when mode=stdio",
                        server.id
                    )));
                }
            }
            McpServerMode::Sse => {
                let url = server.url.as_deref().map(str::trim).unwrap_or_default();
                if url.is_empty() {
                    return Err(ConfigError::InvalidConfig(format!(
                        "mcp.servers '{}' requires non-empty url when mode=sse",
                        server.id
                    )));
                }
                let parsed = url::Url::parse(url).map_err(|err| {
                    ConfigError::InvalidConfig(format!(
                        "mcp.servers '{}' has invalid url '{}': {}",
                        server.id, url, err
                    ))
                })?;
                let scheme = parsed.scheme();
                if scheme != "http" && scheme != "https" {
                    return Err(ConfigError::InvalidConfig(format!(
                        "mcp.servers '{}' url scheme must be http or https",
                        server.id
                    )));
                }
            }
        }
    }

    if config.tools.web_search.enabled {
        if config.tools.web_search.provider.trim().is_empty() {
            return Err(ConfigError::InvalidConfig(
                "tools.web_search.provider cannot be empty when enabled".to_string(),
            ));
        }

        match config.tools.web_search.provider.as_str() {
            "tavily" => {
                if !has_tavily_web_search_key_source(&config.tools.web_search.tavily) {
                    return Err(ConfigError::InvalidConfig(
                        "tools.web_search.tavily requires api_key or env_key".to_string(),
                    ));
                }
            }
            "brave" => {
                if !has_brave_web_search_key_source(&config.tools.web_search.brave) {
                    return Err(ConfigError::InvalidConfig(
                        "tools.web_search.brave requires api_key or env_key".to_string(),
                    ));
                }
            }
            other => {
                return Err(ConfigError::InvalidConfig(format!(
                    "tools.web_search.provider '{}' is not supported, expected one of: tavily, brave",
                    other
                )));
            }
        }
    }

    if config.tools.web_fetch.enabled {
        if config.tools.web_fetch.max_chars == 0 {
            return Err(ConfigError::InvalidConfig(
                "tools.web_fetch.max_chars must be greater than 0".to_string(),
            ));
        }
        if config.tools.web_fetch.timeout_seconds == 0 {
            return Err(ConfigError::InvalidConfig(
                "tools.web_fetch.timeout_seconds must be greater than 0".to_string(),
            ));
        }
    }

    if config.tools.sub_agent.max_iterations == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.sub_agent.max_iterations must be greater than 0".to_string(),
        ));
    }
    if config.tools.sub_agent.max_tool_calls == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.sub_agent.max_tool_calls must be greater than 0".to_string(),
        ));
    }
    if config.skills.sync_timeout == 0 {
        return Err(ConfigError::InvalidConfig(
            "skills.sync_timeout must be greater than 0".to_string(),
        ));
    }
    for (registry_name, registry) in &config.skills.registries {
        if registry_name.trim().is_empty() || registry.address.trim().is_empty() {
            return Err(ConfigError::InvalidConfig(
                "skills.<registry>.address cannot be empty".to_string(),
            ));
        }
        let mut names = std::collections::BTreeSet::new();
        for skill_name in &registry.installed {
            let skill_name = skill_name.trim();
            if skill_name.is_empty() {
                return Err(ConfigError::InvalidConfig(format!(
                    "skills.{registry_name}.installed contains empty skill name"
                )));
            }
            if !names.insert(skill_name.to_string()) {
                return Err(ConfigError::InvalidConfig(format!(
                    "skills.{registry_name}.installed contains duplicated skill '{}'",
                    skill_name
                )));
            }
        }
    }
    if config.cron.tick_ms == 0 {
        return Err(ConfigError::InvalidConfig(
            "cron.tick_ms must be greater than 0".to_string(),
        ));
    }
    if config.cron.runtime_tick_ms == 0 {
        return Err(ConfigError::InvalidConfig(
            "cron.runtime_tick_ms must be greater than 0".to_string(),
        ));
    }
    if config.cron.runtime_drain_batch == 0 {
        return Err(ConfigError::InvalidConfig(
            "cron.runtime_drain_batch must be greater than 0".to_string(),
        ));
    }
    if config.cron.batch_limit <= 0 {
        return Err(ConfigError::InvalidConfig(
            "cron.batch_limit must be greater than 0".to_string(),
        ));
    }
    if config.tools.memory.search_limit == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.memory.search_limit must be greater than 0".to_string(),
        ));
    }
    if config.tools.memory.fts_limit == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.memory.fts_limit must be greater than 0".to_string(),
        ));
    }
    if config.tools.memory.vector_limit == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.memory.vector_limit must be greater than 0".to_string(),
        ));
    }
    if config.tools.shell.safe_commands.is_empty() {
        return Err(ConfigError::InvalidConfig(
            "tools.shell.safe_commands must contain at least one command".to_string(),
        ));
    }
    if config.tools.shell.max_timeout_ms == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.shell.max_timeout_ms must be greater than 0".to_string(),
        ));
    }
    if config.tools.shell.max_output_bytes == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.shell.max_output_bytes must be greater than 0".to_string(),
        ));
    }

    Ok(())
}

fn has_tavily_web_search_key_source(provider: &TavilyWebSearchConfig) -> bool {
    provider
        .api_key
        .as_ref()
        .is_some_and(|v| !v.trim().is_empty())
        || provider
            .env_key
            .as_ref()
            .is_some_and(|v| !v.trim().is_empty())
}

fn has_brave_web_search_key_source(provider: &BraveWebSearchConfig) -> bool {
    provider
        .api_key
        .as_ref()
        .is_some_and(|v| !v.trim().is_empty())
        || provider
            .env_key
            .as_ref()
            .is_some_and(|v| !v.trim().is_empty())
}
