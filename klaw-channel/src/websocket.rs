use crate::{
    ChannelRequest, ChannelResult, ChannelRuntime, SessionChannel,
    manager::{ChannelKind, ChannelSupervisorReporter, ManagedChannelDriver},
};
use async_trait::async_trait;
use klaw_config::WebsocketConfig;
use serde_json::Value;
use std::collections::BTreeMap;
use tokio::sync::watch;

pub const META_WEBSOCKET_CONNECTION_ID: &str = "channel.websocket.connection_id";
pub const META_WEBSOCKET_CHANNEL_ID: &str = "channel.websocket.channel_id";
pub const META_WEBSOCKET_REQUEST_ID: &str = "channel.websocket.request_id";

#[derive(Debug, Clone)]
pub struct WebsocketSubmitEnvelope {
    pub channel_id: String,
    pub connection_id: String,
    pub request_id: String,
    pub session_key: String,
    pub chat_id: String,
    pub input: String,
    pub metadata: BTreeMap<String, Value>,
}

impl WebsocketSubmitEnvelope {
    #[must_use]
    pub fn into_channel_request(self) -> ChannelRequest {
        let mut metadata = self.metadata;
        metadata.insert(
            META_WEBSOCKET_CONNECTION_ID.to_string(),
            Value::String(self.connection_id),
        );
        metadata.insert(
            META_WEBSOCKET_CHANNEL_ID.to_string(),
            Value::String(self.channel_id),
        );
        metadata.insert(
            META_WEBSOCKET_REQUEST_ID.to_string(),
            Value::String(self.request_id),
        );
        ChannelRequest {
            channel: SessionChannel::Websocket.to_string(),
            input: self.input,
            session_key: self.session_key,
            chat_id: self.chat_id,
            media_references: Vec::new(),
            metadata,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WebsocketChannel {
    config: WebsocketConfig,
}

impl WebsocketChannel {
    #[must_use]
    pub fn from_app_config(config: WebsocketConfig) -> Self {
        Self { config }
    }

    #[must_use]
    pub fn config(&self) -> &WebsocketConfig {
        &self.config
    }
}

#[async_trait(?Send)]
impl ManagedChannelDriver for WebsocketChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Websocket
    }

    fn instance_id(&self) -> &str {
        &self.config.id
    }

    async fn run_until_shutdown(
        &mut self,
        _runtime: &dyn ChannelRuntime,
        shutdown: &mut watch::Receiver<bool>,
        reporter: ChannelSupervisorReporter,
    ) -> ChannelResult<()> {
        reporter.mark_running("websocket channel initialized");
        loop {
            shutdown.changed().await?;
            if *shutdown.borrow() {
                return Ok(());
            }
        }
    }
}
