use async_trait::async_trait;
use klaw_core::MediaReference;
use std::collections::BTreeMap;
use std::error::Error;
use std::time::Duration;

pub mod dingtalk;
pub mod manager;
pub mod media;
pub mod render;
pub mod stdio;
pub mod telegram;

pub use manager::{
    ChannelConfigSnapshot, ChannelDriverFactory, ChannelInstanceConfig, ChannelInstanceKey,
    ChannelInstanceStatus, ChannelKind, ChannelLifecycleState, ChannelManager, ChannelSyncResult,
    DefaultChannelDriverFactory, ManagedChannelDriver,
};

pub type ChannelResult<T> = Result<T, Box<dyn Error>>;

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
}

#[async_trait(?Send)]
pub trait ChannelRuntime {
    async fn submit(&self, request: ChannelRequest) -> ChannelResult<Option<ChannelResponse>>;

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
