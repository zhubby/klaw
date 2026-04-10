use crate::{
    AppConfig, BraveWebSearchConfig, ChannelsConfig, ConfigError, McpServerMode, SttProviderKind,
    TavilyWebSearchConfig, TtsProviderKind, VoiceConfig,
};
use std::net::IpAddr;

pub(crate) fn validate(config: &AppConfig) -> Result<(), ConfigError> {
    if config.model_provider.trim().is_empty() {
        return Err(ConfigError::InvalidConfig(
            "model_provider cannot be empty".to_string(),
        ));
    }
    if config
        .model
        .as_deref()
        .is_some_and(|model| model.trim().is_empty())
    {
        return Err(ConfigError::InvalidConfig(
            "model cannot be empty when configured".to_string(),
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

    config.gateway.listen_ip.parse::<IpAddr>().map_err(|err| {
        ConfigError::InvalidConfig(format!(
            "gateway.listen_ip '{}' is invalid: {}",
            config.gateway.listen_ip, err
        ))
    })?;
    if config.gateway.tls.enabled {
        let cert_path = config
            .gateway
            .tls
            .cert_path
            .as_deref()
            .map(str::trim)
            .unwrap_or_default();
        if cert_path.is_empty() {
            return Err(ConfigError::InvalidConfig(
                "gateway.tls.cert_path cannot be empty when gateway.tls.enabled=true".to_string(),
            ));
        }
        let key_path = config
            .gateway
            .tls
            .key_path
            .as_deref()
            .map(str::trim)
            .unwrap_or_default();
        if key_path.is_empty() {
            return Err(ConfigError::InvalidConfig(
                "gateway.tls.key_path cannot be empty when gateway.tls.enabled=true".to_string(),
            ));
        }
    }
    validate_gateway_webhook_endpoint(
        "gateway.webhook.events",
        config.gateway.webhook.events.max_body_bytes,
    )?;
    validate_gateway_webhook_endpoint(
        "gateway.webhook.agents",
        config.gateway.webhook.agents.max_body_bytes,
    )?;

    validate_channels(&config.channels)?;
    validate_local_attachments(
        &config.tools.channel_attachment.local_attachments,
        "tools.channel_attachment.local_attachments",
    )?;
    validate_voice(&config.voice)?;

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
        if server.tool_timeout_seconds == 0 {
            return Err(ConfigError::InvalidConfig(format!(
                "mcp.servers '{}' tool_timeout_seconds must be greater than 0",
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

    if config.tools.apply_patch.enabled {
        for root in &config.tools.apply_patch.allowed_roots {
            if root.trim().is_empty() {
                return Err(ConfigError::InvalidConfig(
                    "tools.apply_patch.allowed_roots cannot contain empty paths".to_string(),
                ));
            }
        }
        if config
            .tools
            .apply_patch
            .workspace
            .as_deref()
            .is_some_and(|workspace| workspace.trim().is_empty())
        {
            return Err(ConfigError::InvalidConfig(
                "tools.apply_patch.workspace cannot be empty".to_string(),
            ));
        }
    }

    if config.tools.sub_agent.enabled && config.tools.sub_agent.max_iterations == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.sub_agent.max_iterations must be greater than 0".to_string(),
        ));
    }
    if config.tools.sub_agent.enabled && config.tools.sub_agent.max_tool_calls == 0 {
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
    if config.tools.memory.enabled && config.tools.memory.search_limit == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.memory.search_limit must be greater than 0".to_string(),
        ));
    }
    if config.tools.memory.enabled && config.tools.memory.fts_limit == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.memory.fts_limit must be greater than 0".to_string(),
        ));
    }
    if config.tools.memory.enabled && config.tools.memory.vector_limit == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.memory.vector_limit must be greater than 0".to_string(),
        ));
    }
    if config.tools.shell.enabled && config.tools.shell.max_timeout_ms == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.shell.max_timeout_ms must be greater than 0".to_string(),
        ));
    }
    if config.tools.shell.enabled && config.tools.shell.max_output_bytes == 0 {
        return Err(ConfigError::InvalidConfig(
            "tools.shell.max_output_bytes must be greater than 0".to_string(),
        ));
    }
    if config.tools.shell.enabled
        && config
            .tools
            .shell
            .workspace
            .as_deref()
            .is_some_and(|workspace| workspace.trim().is_empty())
    {
        return Err(ConfigError::InvalidConfig(
            "tools.shell.workspace cannot be empty".to_string(),
        ));
    }
    if config
        .storage
        .root_dir
        .as_deref()
        .is_some_and(|root_dir| root_dir.trim().is_empty())
    {
        return Err(ConfigError::InvalidConfig(
            "storage.root_dir cannot be empty when configured".to_string(),
        ));
    }
    validate_observability(config)?;

    Ok(())
}

fn validate_gateway_webhook_endpoint(
    field_prefix: &str,
    max_body_bytes: usize,
) -> Result<(), ConfigError> {
    if max_body_bytes == 0 {
        return Err(ConfigError::InvalidConfig(format!(
            "{field_prefix}.max_body_bytes must be greater than 0"
        )));
    }
    Ok(())
}

fn validate_channels(channels: &ChannelsConfig) -> Result<(), ConfigError> {
    let mut ids = std::collections::BTreeSet::new();
    for account in &channels.dingtalk {
        require_non_empty(&account.id, "channels.dingtalk.id")?;
        if !ids.insert(account.id.trim().to_string()) {
            return Err(ConfigError::InvalidConfig(format!(
                "channels.dingtalk contains duplicated id '{}'",
                account.id
            )));
        }
        if !account.enabled {
            continue;
        }
        require_non_empty(&account.client_id, "channels.dingtalk.client_id")?;
        require_non_empty(&account.client_secret, "channels.dingtalk.client_secret")?;
        require_non_empty(&account.bot_title, "channels.dingtalk.bot_title")?;
        if account.proxy.enabled {
            require_non_empty(&account.proxy.url, "channels.dingtalk.proxy.url")?;
            let parsed = url::Url::parse(account.proxy.url.trim()).map_err(|err| {
                ConfigError::InvalidConfig(format!(
                    "channels.dingtalk '{}' has invalid proxy url '{}': {}",
                    account.id,
                    account.proxy.url.trim(),
                    err
                ))
            })?;
            let scheme = parsed.scheme();
            if scheme != "http" && scheme != "https" {
                return Err(ConfigError::InvalidConfig(format!(
                    "channels.dingtalk '{}' proxy url scheme must be http or https",
                    account.id
                )));
            }
        }
    }
    ids.clear();
    for account in &channels.telegram {
        require_non_empty(&account.id, "channels.telegram.id")?;
        if !ids.insert(account.id.trim().to_string()) {
            return Err(ConfigError::InvalidConfig(format!(
                "channels.telegram contains duplicated id '{}'",
                account.id
            )));
        }
        if !account.enabled {
            continue;
        }
        require_non_empty(&account.bot_token, "channels.telegram.bot_token")?;
        if account.proxy.enabled {
            require_non_empty(&account.proxy.url, "channels.telegram.proxy.url")?;
            let parsed = url::Url::parse(account.proxy.url.trim()).map_err(|err| {
                ConfigError::InvalidConfig(format!(
                    "channels.telegram '{}' has invalid proxy url '{}': {}",
                    account.id,
                    account.proxy.url.trim(),
                    err
                ))
            })?;
            let scheme = parsed.scheme();
            if scheme != "http" && scheme != "https" {
                return Err(ConfigError::InvalidConfig(format!(
                    "channels.telegram '{}' proxy url scheme must be http or https",
                    account.id
                )));
            }
        }
    }
    ids.clear();
    for channel in &channels.websocket {
        require_non_empty(&channel.id, "channels.websocket.id")?;
        if !ids.insert(channel.id.trim().to_string()) {
            return Err(ConfigError::InvalidConfig(format!(
                "channels.websocket contains duplicated id '{}'",
                channel.id
            )));
        }
    }
    for channel in &channels.disable_session_commands_for {
        require_non_empty(channel, "channels.disable_session_commands_for")?;
    }
    Ok(())
}

fn validate_local_attachments(
    config: &crate::LocalAttachmentConfig,
    field_prefix: &str,
) -> Result<(), ConfigError> {
    if config.max_bytes == 0 {
        return Err(ConfigError::InvalidConfig(format!(
            "{field_prefix}.max_bytes must be greater than 0"
        )));
    }
    for path in &config.allowlist {
        require_non_empty(path, &format!("{field_prefix}.allowlist"))?;
        let trimmed = path.trim();
        if !trimmed.starts_with('/') {
            return Err(ConfigError::InvalidConfig(format!(
                "{field_prefix}.allowlist entry '{}' must be an absolute path",
                trimmed
            )));
        }
    }
    Ok(())
}

fn validate_voice(voice: &VoiceConfig) -> Result<(), ConfigError> {
    if !voice.enabled {
        return Ok(());
    }

    require_non_empty(&voice.default_language, "voice.default_language")?;

    match voice.stt_provider {
        SttProviderKind::Deepgram => {
            require_non_empty(
                &voice.providers.deepgram.base_url,
                "voice.providers.deepgram.base_url",
            )?;
            require_non_empty(
                &voice.providers.deepgram.streaming_base_url,
                "voice.providers.deepgram.streaming_base_url",
            )?;
            require_non_empty(
                &voice.providers.deepgram.stt_model,
                "voice.providers.deepgram.stt_model",
            )?;
            if voice.providers.deepgram.resolve_api_key().is_none() {
                return Err(ConfigError::InvalidConfig(
                    "voice.providers.deepgram requires api_key or api_key_env when voice.enabled=true".to_string(),
                ));
            }
        }
        SttProviderKind::Assemblyai => {
            require_non_empty(
                &voice.providers.assemblyai.base_url,
                "voice.providers.assemblyai.base_url",
            )?;
            require_non_empty(
                &voice.providers.assemblyai.streaming_base_url,
                "voice.providers.assemblyai.streaming_base_url",
            )?;
            require_non_empty(
                &voice.providers.assemblyai.stt_model,
                "voice.providers.assemblyai.stt_model",
            )?;
            if voice.providers.assemblyai.resolve_api_key().is_none() {
                return Err(ConfigError::InvalidConfig(
                    "voice.providers.assemblyai requires api_key or api_key_env when voice.enabled=true".to_string(),
                ));
            }
        }
    }

    match voice.tts_provider {
        TtsProviderKind::Elevenlabs => {
            require_non_empty(
                &voice.providers.elevenlabs.base_url,
                "voice.providers.elevenlabs.base_url",
            )?;
            require_non_empty(
                &voice.providers.elevenlabs.streaming_base_url,
                "voice.providers.elevenlabs.streaming_base_url",
            )?;
            require_non_empty(
                &voice.providers.elevenlabs.default_model,
                "voice.providers.elevenlabs.default_model",
            )?;
            if voice.providers.elevenlabs.resolve_api_key().is_none() {
                return Err(ConfigError::InvalidConfig(
                    "voice.providers.elevenlabs requires api_key or api_key_env when voice.enabled=true".to_string(),
                ));
            }
        }
    }

    Ok(())
}

fn require_non_empty(value: &str, field_name: &str) -> Result<(), ConfigError> {
    if value.trim().is_empty() {
        return Err(ConfigError::InvalidConfig(format!(
            "{field_name} cannot be empty"
        )));
    }
    Ok(())
}

fn validate_observability(config: &AppConfig) -> Result<(), ConfigError> {
    require_non_empty(
        &config.observability.service_name,
        "observability.service_name",
    )?;
    require_non_empty(
        &config.observability.service_version,
        "observability.service_version",
    )?;

    if config.observability.metrics.enabled
        && config.observability.metrics.export_interval_seconds == 0
    {
        return Err(ConfigError::InvalidConfig(
            "observability.metrics.export_interval_seconds must be greater than 0".to_string(),
        ));
    }

    if config.observability.traces.enabled {
        let sample_rate = config.observability.traces.sample_rate;
        if !sample_rate.is_finite() {
            return Err(ConfigError::InvalidConfig(
                "observability.traces.sample_rate must be a finite number".to_string(),
            ));
        }
        if !(0.0..=1.0).contains(&sample_rate) {
            return Err(ConfigError::InvalidConfig(
                "observability.traces.sample_rate must be in range [0.0, 1.0]".to_string(),
            ));
        }
    }

    if config.observability.otlp.enabled {
        let endpoint = config.observability.otlp.endpoint.trim();
        if endpoint.is_empty() {
            return Err(ConfigError::InvalidConfig(
                "observability.otlp.endpoint cannot be empty when observability.otlp.enabled=true"
                    .to_string(),
            ));
        }
        let parsed = url::Url::parse(endpoint).map_err(|err| {
            ConfigError::InvalidConfig(format!(
                "observability.otlp.endpoint '{}' is invalid: {}",
                endpoint, err
            ))
        })?;
        let scheme = parsed.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(ConfigError::InvalidConfig(
                "observability.otlp.endpoint scheme must be http or https".to_string(),
            ));
        }
    }

    if config.observability.prometheus.enabled {
        if config.observability.prometheus.listen_port == 0 {
            return Err(ConfigError::InvalidConfig(
                "observability.prometheus.listen_port must be greater than 0".to_string(),
            ));
        }
        let path = config.observability.prometheus.path.trim();
        if path.is_empty() {
            return Err(ConfigError::InvalidConfig(
                "observability.prometheus.path cannot be empty when observability.prometheus.enabled=true"
                    .to_string(),
            ));
        }
        if !path.starts_with('/') {
            return Err(ConfigError::InvalidConfig(
                "observability.prometheus.path must start with '/'".to_string(),
            ));
        }
    }

    if config
        .observability
        .audit
        .output_path
        .as_deref()
        .is_some_and(|path| path.trim().is_empty())
    {
        return Err(ConfigError::InvalidConfig(
            "observability.audit.output_path cannot be empty when configured".to_string(),
        ));
    }

    if config.observability.local_store.enabled {
        if config.observability.local_store.retention_days == 0 {
            return Err(ConfigError::InvalidConfig(
                "observability.local_store.retention_days must be greater than 0".to_string(),
            ));
        }
        if config.observability.local_store.flush_interval_seconds == 0 {
            return Err(ConfigError::InvalidConfig(
                "observability.local_store.flush_interval_seconds must be greater than 0"
                    .to_string(),
            ));
        }
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
