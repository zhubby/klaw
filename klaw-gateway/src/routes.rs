use strum::IntoStaticStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
pub enum Route {
    #[strum(serialize = "/")]
    Home,
    #[strum(serialize = "/logo.webp")]
    HomeLogo,
    #[strum(serialize = "/favicon.ico")]
    Favicon,
    #[strum(serialize = "/images/{filename}")]
    Images,
    #[strum(serialize = "/chat")]
    Chat,
    #[strum(serialize = "/chat/dist/klaw_webui.js")]
    ChatDistJs,
    #[strum(serialize = "/chat/dist/klaw_webui_bg.wasm")]
    ChatDistWasm,
    #[strum(serialize = "/ws/chat")]
    WsChat,
    #[strum(serialize = "/webhook/events")]
    WebhookEvents,
    #[strum(serialize = "/webhook/agents")]
    WebhookAgents,
    #[strum(serialize = "/archive/upload")]
    ArchiveUpload,
    #[strum(serialize = "/archive/download/{id}")]
    ArchiveDownload,
    #[strum(serialize = "/archive/list")]
    ArchiveList,
    #[strum(serialize = "/archive/{id}")]
    ArchiveGet,
    #[strum(serialize = "/health/live")]
    HealthLive,
    #[strum(serialize = "/health/ready")]
    HealthReady,
    #[strum(serialize = "/health/status")]
    HealthStatus,
    #[strum(serialize = "/metrics")]
    Metrics,
}

impl Route {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}
