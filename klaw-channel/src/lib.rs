use async_trait::async_trait;
use klaw_core::MediaReference;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;
use std::time::Duration;

pub mod dingtalk;
pub mod im_card;
pub mod manager;
pub mod media;
pub mod outbound;
pub mod render;
pub mod telegram;
pub mod terminal;
pub mod websocket;

pub use manager::{
    ChannelConfigSnapshot, ChannelDriverFactory, ChannelInstanceConfig, ChannelInstanceKey,
    ChannelInstanceStatus, ChannelKind, ChannelLifecycleState, ChannelManager, ChannelSyncResult,
    DefaultChannelDriverFactory, ManagedChannelDriver,
};

pub type ChannelResult<T> = Result<T, Box<dyn Error>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboundAttachmentKind {
    Image,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OutboundAttachmentSource {
    ArchiveId { archive_id: String },
    LocalPath { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundAttachment {
    #[serde(flatten)]
    pub source: OutboundAttachmentSource,
    pub kind: OutboundAttachmentKind,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub caption: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalAttachmentPolicy {
    pub workspace_root: PathBuf,
    pub allowlist: Vec<PathBuf>,
    pub max_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct ChannelRequest {
    pub channel: String,
    pub input: String,
    pub session_key: String,
    pub chat_id: String,
    pub media_references: Vec<MediaReference>,
    pub metadata: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct ChannelResponse {
    pub content: String,
    pub reasoning: Option<String>,
    pub metadata: BTreeMap<String, serde_json::Value>,
    pub attachments: Vec<OutboundAttachment>,
}

#[derive(Debug, Clone)]
pub enum ChannelStreamEvent {
    Snapshot(ChannelResponse),
    Clear,
}

#[async_trait(?Send)]
pub trait ChannelStreamWriter {
    async fn write(&mut self, event: ChannelStreamEvent) -> ChannelResult<()>;
}

#[async_trait(?Send)]
pub trait ChannelRuntime {
    async fn submit(&self, request: ChannelRequest) -> ChannelResult<Option<ChannelResponse>>;

    async fn submit_streaming(
        &self,
        request: ChannelRequest,
        writer: &mut dyn ChannelStreamWriter,
    ) -> ChannelResult<Option<ChannelResponse>> {
        let response = self.submit(request).await?;
        match response.clone() {
            Some(output) => writer.write(ChannelStreamEvent::Snapshot(output)).await?,
            None => writer.write(ChannelStreamEvent::Clear).await?,
        }
        Ok(response)
    }

    fn cron_tick_interval(&self) -> Duration;

    fn runtime_tick_interval(&self) -> Duration;

    async fn on_cron_tick(&self);

    async fn on_runtime_tick(&self);
}

#[async_trait(?Send)]
pub trait Channel {
    fn name(&self) -> &'static str;

    async fn run(&mut self, runtime: &dyn ChannelRuntime) -> ChannelResult<()>;
}
