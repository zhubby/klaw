use crate::{ChannelResult, LocalAttachmentPolicy};
use klaw_config::LocalAttachmentConfig;
use klaw_util::{default_data_dir, workspace_dir};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DingtalkChannelConfig {
    pub account_id: String,
    pub client_id: String,
    pub client_secret: String,
    pub bot_title: String,
    pub show_reasoning: bool,
    pub stream_output: bool,
    pub allowlist: Vec<String>,
    pub local_attachments: LocalAttachmentConfig,
    pub proxy: DingtalkProxyConfig,
}

impl Default for DingtalkChannelConfig {
    fn default() -> Self {
        Self {
            account_id: "default".to_string(),
            client_id: String::new(),
            client_secret: String::new(),
            bot_title: "Klaw".to_string(),
            show_reasoning: false,
            stream_output: false,
            allowlist: Vec::new(),
            local_attachments: LocalAttachmentConfig::default(),
            proxy: DingtalkProxyConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DingtalkProxyConfig {
    pub enabled: bool,
    pub url: String,
}

impl Default for DingtalkProxyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
        }
    }
}

pub(super) fn resolve_local_attachment_policy(
    config: &LocalAttachmentConfig,
) -> ChannelResult<LocalAttachmentPolicy> {
    let root = default_data_dir().ok_or_else(|| "failed to resolve home dir".to_string())?;
    let workspace = workspace_dir(&root);
    std::fs::create_dir_all(&workspace)?;
    let workspace_root = std::fs::canonicalize(&workspace)?;
    let allowlist = config
        .allowlist
        .iter()
        .map(|path| PathBuf::from(path.trim()))
        .collect();
    Ok(LocalAttachmentPolicy {
        workspace_root,
        allowlist,
        max_bytes: config.max_bytes,
    })
}
