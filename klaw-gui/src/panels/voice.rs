use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::settings::current_ui_language;
use crate::voice_test::{RecordingCapture, RecordingHandle};
use egui::{Color32, RichText};
use egui_dock::{AllowedSplits, DockArea, DockState, NodeIndex, Style, SurfaceIndex, TabIndex};
use egui_phosphor::regular;
use klaw_config::{
    AppConfig, AssemblyAiVoiceConfig, ConfigError, ConfigSnapshot, ConfigStore,
    DeepgramVoiceConfig, ElevenLabsVoiceConfig, SttProviderKind, TtsProviderKind, VoiceConfig,
};
use klaw_ui_kit::{LocaleDomain, Translator, label_with_hint, toggle::toggle};
use klaw_voice::{SttInput, TtsInput, VoiceService};
use rodio::{Decoder, OutputStream, Sink};
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};
use tokio::runtime::Builder;
use uuid::Uuid;

const VOICE_POLL_INTERVAL: Duration = Duration::from_millis(200);
const TTS_INPUT_ROWS: usize = 6;

#[derive(Debug, Clone)]
struct VoiceConfigForm {
    enabled: bool,
    stt_provider: SttProviderKind,
    tts_provider: TtsProviderKind,
    default_language: String,
    default_voice_id: String,
    deepgram_api_key: String,
    deepgram_api_key_env: String,
    deepgram_base_url: String,
    deepgram_streaming_base_url: String,
    deepgram_stt_model: String,
    assemblyai_api_key: String,
    assemblyai_api_key_env: String,
    assemblyai_base_url: String,
    assemblyai_streaming_base_url: String,
    assemblyai_stt_model: String,
    elevenlabs_api_key: String,
    elevenlabs_api_key_env: String,
    elevenlabs_base_url: String,
    elevenlabs_streaming_base_url: String,
    elevenlabs_default_model: String,
    elevenlabs_default_voice_id: String,
}

impl Default for VoiceConfigForm {
    fn default() -> Self {
        Self::from_config(&VoiceConfig::default())
    }
}

impl VoiceConfigForm {
    fn from_config(config: &VoiceConfig) -> Self {
        Self {
            enabled: config.enabled,
            stt_provider: config.stt_provider,
            tts_provider: config.tts_provider,
            default_language: config.default_language.clone(),
            default_voice_id: config.default_voice_id.clone().unwrap_or_default(),
            deepgram_api_key: config
                .providers
                .deepgram
                .api_key
                .clone()
                .unwrap_or_default(),
            deepgram_api_key_env: config.providers.deepgram.api_key_env.clone(),
            deepgram_base_url: config.providers.deepgram.base_url.clone(),
            deepgram_streaming_base_url: config.providers.deepgram.streaming_base_url.clone(),
            deepgram_stt_model: config.providers.deepgram.stt_model.clone(),
            assemblyai_api_key: config
                .providers
                .assemblyai
                .api_key
                .clone()
                .unwrap_or_default(),
            assemblyai_api_key_env: config.providers.assemblyai.api_key_env.clone(),
            assemblyai_base_url: config.providers.assemblyai.base_url.clone(),
            assemblyai_streaming_base_url: config.providers.assemblyai.streaming_base_url.clone(),
            assemblyai_stt_model: config.providers.assemblyai.stt_model.clone(),
            elevenlabs_api_key: config
                .providers
                .elevenlabs
                .api_key
                .clone()
                .unwrap_or_default(),
            elevenlabs_api_key_env: config.providers.elevenlabs.api_key_env.clone(),
            elevenlabs_base_url: config.providers.elevenlabs.base_url.clone(),
            elevenlabs_streaming_base_url: config.providers.elevenlabs.streaming_base_url.clone(),
            elevenlabs_default_model: config.providers.elevenlabs.default_model.clone(),
            elevenlabs_default_voice_id: config
                .providers
                .elevenlabs
                .default_voice_id
                .clone()
                .unwrap_or_default(),
        }
    }

    fn apply_to_config(&self, config: &mut AppConfig) -> Result<(), String> {
        let default_language = self.default_language.trim();
        if default_language.is_empty() {
            return Err("default language cannot be empty".to_string());
        }

        for (label, value) in [
            ("Deepgram base URL", self.deepgram_base_url.trim()),
            (
                "Deepgram streaming base URL",
                self.deepgram_streaming_base_url.trim(),
            ),
            ("Deepgram STT model", self.deepgram_stt_model.trim()),
            ("AssemblyAI base URL", self.assemblyai_base_url.trim()),
            (
                "AssemblyAI streaming base URL",
                self.assemblyai_streaming_base_url.trim(),
            ),
            ("AssemblyAI STT model", self.assemblyai_stt_model.trim()),
            ("ElevenLabs base URL", self.elevenlabs_base_url.trim()),
            (
                "ElevenLabs streaming base URL",
                self.elevenlabs_streaming_base_url.trim(),
            ),
            (
                "ElevenLabs default model",
                self.elevenlabs_default_model.trim(),
            ),
        ] {
            if value.is_empty() {
                return Err(format!("{label} cannot be empty"));
            }
        }

        config.voice.enabled = self.enabled;
        config.voice.stt_provider = self.stt_provider;
        config.voice.tts_provider = self.tts_provider;
        config.voice.default_language = default_language.to_string();
        config.voice.default_voice_id = normalize_optional(&self.default_voice_id);
        config.voice.providers.deepgram = DeepgramVoiceConfig {
            api_key: normalize_optional(&self.deepgram_api_key),
            api_key_env: self.deepgram_api_key_env.trim().to_string(),
            base_url: self.deepgram_base_url.trim().to_string(),
            streaming_base_url: self.deepgram_streaming_base_url.trim().to_string(),
            stt_model: self.deepgram_stt_model.trim().to_string(),
        };
        config.voice.providers.assemblyai = AssemblyAiVoiceConfig {
            api_key: normalize_optional(&self.assemblyai_api_key),
            api_key_env: self.assemblyai_api_key_env.trim().to_string(),
            base_url: self.assemblyai_base_url.trim().to_string(),
            streaming_base_url: self.assemblyai_streaming_base_url.trim().to_string(),
            stt_model: self.assemblyai_stt_model.trim().to_string(),
        };
        config.voice.providers.elevenlabs = ElevenLabsVoiceConfig {
            api_key: normalize_optional(&self.elevenlabs_api_key),
            api_key_env: self.elevenlabs_api_key_env.trim().to_string(),
            base_url: self.elevenlabs_base_url.trim().to_string(),
            streaming_base_url: self.elevenlabs_streaming_base_url.trim().to_string(),
            default_model: self.elevenlabs_default_model.trim().to_string(),
            default_voice_id: normalize_optional(&self.elevenlabs_default_voice_id),
        };

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum VoiceConfigTab {
    #[default]
    General,
    Deepgram,
    Assemblyai,
    Elevenlabs,
}

impl VoiceConfigTab {
    const ALL: [Self; 4] = [
        Self::General,
        Self::Deepgram,
        Self::Assemblyai,
        Self::Elevenlabs,
    ];

    fn title_key(self) -> &'static str {
        match self {
            Self::General => "voice-config-tab-general",
            Self::Deepgram => "voice-config-tab-deepgram",
            Self::Assemblyai => "voice-config-tab-assemblyai",
            Self::Elevenlabs => "voice-config-tab-elevenlabs",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::General => regular::GEAR_SIX,
            Self::Deepgram => regular::MICROPHONE,
            Self::Assemblyai => regular::WAVEFORM,
            Self::Elevenlabs => regular::SPEAKER_HIGH,
        }
    }

    fn tab_id(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Deepgram => "deepgram",
            Self::Assemblyai => "assemblyai",
            Self::Elevenlabs => "elevenlabs",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VoiceTestMode {
    Stt,
    Tts,
}

impl VoiceTestMode {
    fn label(self, t: &Translator) -> String {
        match self {
            Self::Stt => t.text("voice-stt-tab"),
            Self::Tts => t.text("voice-tts-tab"),
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Stt => regular::MICROPHONE,
            Self::Tts => regular::SPEAKER_HIGH,
        }
    }
}

#[derive(Debug, Clone)]
struct SttTestResult {
    transcript: String,
    provider_name: String,
    language: Option<String>,
    confidence: Option<f32>,
    duration_ms: Option<u64>,
    capture_duration_ms: u64,
    sample_rate_hz: u32,
    channels: u16,
    device_name: String,
    sample_count: usize,
}

#[derive(Debug, Clone)]
enum SttTestState {
    Idle,
    Recording {
        started_at: Instant,
        device_name: String,
        sample_rate_hz: u32,
        channels: u16,
    },
    Transcribing {
        started_at: Instant,
        capture_duration_ms: u64,
    },
    Completed(SttTestResult),
    Failed(String),
}

#[derive(Debug, Clone)]
struct TtsTestResult {
    provider_name: String,
    mime_type: String,
    duration_ms: Option<u64>,
    output_path: PathBuf,
    output_size_bytes: usize,
    requested_voice_id: Option<String>,
}

#[derive(Debug, Clone)]
enum TtsTestState {
    Idle,
    Synthesizing { started_at: Instant },
    Completed(TtsTestResult),
    Failed(String),
}

struct PlaybackHandle {
    _stream: OutputStream,
    sink: Sink,
    path: PathBuf,
}

pub struct VoicePanel {
    store: Option<ConfigStore>,
    config: AppConfig,
    config_form: VoiceConfigForm,
    config_window_open: bool,
    config_tab: VoiceConfigTab,
    config_dock_state: DockState<VoiceConfigTab>,
    test_mode: VoiceTestMode,
    recording: Option<RecordingHandle>,
    stt_state: SttTestState,
    stt_result_rx: Option<Receiver<Result<SttTestResult, String>>>,
    tts_input_text: String,
    tts_voice_id: String,
    tts_state: TtsTestState,
    tts_result_rx: Option<Receiver<Result<TtsTestResult, String>>>,
    playback: Option<PlaybackHandle>,
}

impl Default for VoicePanel {
    fn default() -> Self {
        Self {
            store: None,
            config: AppConfig::default(),
            config_form: VoiceConfigForm::default(),
            config_window_open: false,
            config_tab: VoiceConfigTab::default(),
            config_dock_state: Self::config_dock_state(VoiceConfigTab::default()),
            test_mode: VoiceTestMode::Stt,
            recording: None,
            stt_state: SttTestState::Idle,
            stt_result_rx: None,
            tts_input_text: String::new(),
            tts_voice_id: String::new(),
            tts_state: TtsTestState::Idle,
            tts_result_rx: None,
            playback: None,
        }
    }
}

impl VoicePanel {
    fn translator() -> Translator {
        Translator::new(LocaleDomain::Gui, current_ui_language())
    }

    fn config_dock_state(active_tab: VoiceConfigTab) -> DockState<VoiceConfigTab> {
        let mut dock_state = DockState::new(VoiceConfigTab::ALL.to_vec());
        let active_index = VoiceConfigTab::ALL
            .iter()
            .position(|tab| *tab == active_tab)
            .unwrap_or_default();
        dock_state.set_active_tab((
            SurfaceIndex::main(),
            NodeIndex::root(),
            TabIndex(active_index),
        ));
        dock_state
    }

    fn ensure_store_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.store.is_some() {
            return;
        }
        match ConfigStore::open(None) {
            Ok(store) => {
                let snapshot = store.snapshot();
                self.store = Some(store);
                self.apply_snapshot(snapshot);
            }
            Err(err) => notifications.error(format!("Failed to load config: {err}")),
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config = snapshot.config;
        self.config_form = VoiceConfigForm::from_config(&self.config.voice);
    }

    fn open_config_window(&mut self) {
        self.config_form = VoiceConfigForm::from_config(&self.config.voice);
        self.config_tab = VoiceConfigTab::General;
        self.config_dock_state = Self::config_dock_state(self.config_tab);
        self.config_window_open = true;
    }

    fn save_config(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let Some(store) = self.store.as_ref() else {
            notifications.error(t.text("voice-notify-store-unavailable"));
            return;
        };

        let config_form = self.config_form.clone();
        match store.update_config(|config| {
            config_form
                .apply_to_config(config)
                .map_err(ConfigError::InvalidConfig)?;
            Ok(())
        }) {
            Ok((snapshot, ())) => {
                self.apply_snapshot(snapshot);
                self.config_window_open = false;
                notifications.success(t.text("voice-notify-config-saved"));
            }
            Err(err) => notifications.error(t.text_args(
                "voice-notify-save-failed",
                HashMap::from([("error", err.to_string())]),
            )),
        }
    }

    fn reload_config(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let Some(store) = self.store.as_ref() else {
            notifications.error(t.text("voice-notify-store-unavailable"));
            return;
        };
        match store.reload() {
            Ok(snapshot) => {
                self.apply_snapshot(snapshot);
                notifications.success(t.text("voice-notify-config-reloaded"));
            }
            Err(err) => notifications.error(t.text_args(
                "voice-notify-reload-failed",
                HashMap::from([("error", err.to_string())]),
            )),
        }
    }

    fn poll_stt_result(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let Some(rx) = self.stt_result_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(result)) => {
                self.stt_result_rx = None;
                self.stt_state = SttTestState::Completed(result);
                notifications.success(t.text("voice-notify-stt-completed"));
            }
            Ok(Err(err)) => {
                self.stt_result_rx = None;
                self.stt_state = SttTestState::Failed(err.clone());
                notifications.error(err);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.stt_result_rx = None;
                let message = t.text("voice-notify-stt-disconnected");
                self.stt_state = SttTestState::Failed(message.clone());
                notifications.error(message);
            }
        }
    }

    fn poll_tts_result(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let Some(rx) = self.tts_result_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(result)) => {
                self.tts_result_rx = None;
                self.tts_state = TtsTestState::Completed(result.clone());
                notifications.success(t.text_args(
                    "voice-notify-tts-completed",
                    HashMap::from([("path", result.output_path.display().to_string())]),
                ));
            }
            Ok(Err(err)) => {
                self.tts_result_rx = None;
                self.tts_state = TtsTestState::Failed(err.clone());
                notifications.error(err);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.tts_result_rx = None;
                let message = t.text("voice-notify-tts-disconnected");
                self.tts_state = TtsTestState::Failed(message.clone());
                notifications.error(message);
            }
        }
    }

    fn start_recording(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        if self.recording.is_some() {
            notifications.info(t.text("voice-notify-recording-in-progress"));
            return;
        }
        if self.stt_result_rx.is_some() {
            notifications.info(t.text("voice-notify-transcription-running"));
            return;
        }
        match RecordingHandle::start_default() {
            Ok(handle) => {
                self.stt_state = SttTestState::Recording {
                    started_at: Instant::now(),
                    device_name: handle.device_name().to_string(),
                    sample_rate_hz: handle.sample_rate_hz(),
                    channels: handle.channels(),
                };
                notifications.success(t.text("voice-notify-recording-started"));
                self.recording = Some(handle);
            }
            Err(err) => {
                self.stt_state = SttTestState::Failed(err.clone());
                notifications.error(err);
            }
        }
    }

    fn stop_recording(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let Some(handle) = self.recording.take() else {
            notifications.info(t.text("voice-notify-no-recording"));
            return;
        };

        let capture = match handle.finish() {
            Ok(capture) => capture,
            Err(err) => {
                self.stt_state = SttTestState::Failed(err.clone());
                notifications.error(err);
                return;
            }
        };

        if let Err(err) = validate_stt_test_config(&self.config) {
            self.stt_state = SttTestState::Failed(err.clone());
            notifications.error(err);
            return;
        }

        let voice_config = self.config.voice.clone();
        let (tx, rx) = mpsc::channel();
        let capture_duration_ms = capture.duration_ms;
        self.stt_state = SttTestState::Transcribing {
            started_at: Instant::now(),
            capture_duration_ms,
        };
        self.stt_result_rx = Some(rx);

        thread::spawn(move || {
            let outcome = run_transcription_test(capture, voice_config);
            let _ = tx.send(outcome);
        });
        notifications.info(t.text("voice-notify-recording-stopped"));
    }

    fn start_tts_generation(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        if self.tts_result_rx.is_some() {
            notifications.info(t.text("voice-notify-synthesis-running"));
            return;
        }
        let text = self.tts_input_text.trim().to_string();
        if text.is_empty() {
            self.tts_state = TtsTestState::Failed("TTS input cannot be empty".to_string());
            notifications.error("TTS input cannot be empty");
            return;
        }
        if let Err(err) = validate_tts_test_config(&self.config) {
            self.tts_state = TtsTestState::Failed(err.clone());
            notifications.error(err);
            return;
        }

        self.stop_playback();
        let voice_config = self.config.voice.clone();
        let requested_voice_id = normalize_optional(&self.tts_voice_id);
        let (tx, rx) = mpsc::channel();
        self.tts_state = TtsTestState::Synthesizing {
            started_at: Instant::now(),
        };
        self.tts_result_rx = Some(rx);

        thread::spawn(move || {
            let outcome = run_tts_test(text, requested_voice_id, voice_config);
            let _ = tx.send(outcome);
        });
        notifications.info(t.text("voice-notify-tts-submitting"));
    }

    fn play_tts_output(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let TtsTestState::Completed(result) = &self.tts_state else {
            notifications.info(t.text("voice-notify-tts-no-output"));
            return;
        };
        let result = result.clone();

        self.stop_playback();

        let file = match File::open(&result.output_path) {
            Ok(file) => file,
            Err(err) => {
                notifications.error(t.text_args(
                    "voice-notify-tts-open-failed",
                    HashMap::from([("error", err.to_string())]),
                ));
                return;
            }
        };
        let stream = match OutputStream::try_default() {
            Ok(stream) => stream,
            Err(err) => {
                notifications.error(t.text_args(
                    "voice-notify-tts-device-failed",
                    HashMap::from([("error", err.to_string())]),
                ));
                return;
            }
        };
        let sink = match Sink::try_new(&stream.1) {
            Ok(sink) => sink,
            Err(err) => {
                notifications.error(t.text_args(
                    "voice-notify-tts-sink-failed",
                    HashMap::from([("error", err.to_string())]),
                ));
                return;
            }
        };
        let decoder = match Decoder::new(BufReader::new(file)) {
            Ok(decoder) => decoder,
            Err(err) => {
                notifications.error(t.text_args(
                    "voice-notify-tts-decode-failed",
                    HashMap::from([("error", err.to_string())]),
                ));
                return;
            }
        };

        sink.append(decoder);
        sink.play();
        self.playback = Some(PlaybackHandle {
            _stream: stream.0,
            sink,
            path: result.output_path.clone(),
        });
        notifications.success(t.text("voice-notify-tts-playing"));
    }

    fn stop_playback(&mut self) {
        if let Some(playback) = self.playback.take() {
            playback.sink.stop();
        }
    }

    fn poll_playback(&mut self) {
        if self
            .playback
            .as_ref()
            .is_some_and(|playback| playback.sink.empty())
        {
            self.playback = None;
        }
    }

    fn render_config_window(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        let t = Self::translator();
        let mut open = self.config_window_open;
        egui::Window::new(t.text("voice-config-title"))
            .id(egui::Id::new("voice-config-window"))
            .open(&mut open)
            .resizable(true)
            .default_width(720.0)
            .show(ctx, |ui| {
                ui.label(t.text("voice-config-subtitle"));
                ui.separator();

                self.render_config_dock(ui);

                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .button(t.text_args(
                            "voice-config-btn-reload",
                            HashMap::from([("icon", regular::ARROWS_CLOCKWISE.to_string())]),
                        ))
                        .clicked()
                    {
                        self.reload_config(notifications);
                    }
                    if ui
                        .button(t.text_args(
                            "voice-config-btn-save",
                            HashMap::from([("icon", regular::FLOPPY_DISK.to_string())]),
                        ))
                        .clicked()
                    {
                        self.save_config(notifications);
                    }
                });
            });
        self.config_window_open = open;
    }

    fn render_config_dock(&mut self, ui: &mut egui::Ui) {
        let t = Self::translator();
        let mut dock_state = std::mem::replace(
            &mut self.config_dock_state,
            Self::config_dock_state(self.config_tab),
        );
        let mut style = Style::from_egui(ui.style().as_ref());
        style.tab_bar.show_scroll_bar_on_overflow = false;

        ui.set_min_height(380.0);
        DockArea::new(&mut dock_state)
            .id(egui::Id::new("voice-config-dock"))
            .style(style)
            .show_add_buttons(false)
            .show_close_buttons(false)
            .show_leaf_close_all_buttons(false)
            .show_leaf_collapse_buttons(false)
            .tab_context_menus(false)
            .draggable_tabs(false)
            .allowed_splits(AllowedSplits::None)
            .show_inside(
                ui,
                &mut VoiceConfigTabViewer {
                    panel: self,
                    translator: &t,
                },
            );

        self.config_dock_state = dock_state;
    }

    fn render_config_tab_content(&mut self, ui: &mut egui::Ui, tab: VoiceConfigTab) {
        let t = Self::translator();
        match tab {
            VoiceConfigTab::General => self.render_general_config_tab(ui),
            VoiceConfigTab::Deepgram => {
                ui.strong(t.text("voice-config-tab-deepgram"));
                ui.add_space(6.0);
                render_secret_provider_section(
                    ui,
                    "voice-deepgram",
                    &mut self.config_form.deepgram_api_key,
                    &mut self.config_form.deepgram_api_key_env,
                    &mut self.config_form.deepgram_base_url,
                    &mut self.config_form.deepgram_streaming_base_url,
                    Some((
                        &mut self.config_form.deepgram_stt_model,
                        t.text("voice-cfg-label-stt-model"),
                    )),
                    None,
                    &t,
                );
            }
            VoiceConfigTab::Assemblyai => {
                ui.strong(t.text("voice-config-tab-assemblyai"));
                ui.add_space(6.0);
                render_secret_provider_section(
                    ui,
                    "voice-assemblyai",
                    &mut self.config_form.assemblyai_api_key,
                    &mut self.config_form.assemblyai_api_key_env,
                    &mut self.config_form.assemblyai_base_url,
                    &mut self.config_form.assemblyai_streaming_base_url,
                    Some((
                        &mut self.config_form.assemblyai_stt_model,
                        t.text("voice-cfg-label-stt-model"),
                    )),
                    None,
                    &t,
                );
            }
            VoiceConfigTab::Elevenlabs => {
                ui.strong(t.text("voice-config-tab-elevenlabs"));
                ui.add_space(6.0);
                render_secret_provider_section(
                    ui,
                    "voice-elevenlabs",
                    &mut self.config_form.elevenlabs_api_key,
                    &mut self.config_form.elevenlabs_api_key_env,
                    &mut self.config_form.elevenlabs_base_url,
                    &mut self.config_form.elevenlabs_streaming_base_url,
                    Some((
                        &mut self.config_form.elevenlabs_default_model,
                        t.text("voice-cfg-label-default-model"),
                    )),
                    Some((
                        &mut self.config_form.elevenlabs_default_voice_id,
                        t.text("voice-cfg-label-provider-default-voice-id"),
                    )),
                    &t,
                );
            }
        }
    }

    fn render_general_config_tab(&mut self, ui: &mut egui::Ui) {
        let t = Self::translator();
        ui.strong(t.text("voice-config-tab-general"));
        ui.add_space(6.0);
        egui::Grid::new("voice-config-general-grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                label_with_hint(
                    ui,
                    &t.text("voice-cfg-label-enabled"),
                    &t.text("voice-cfg-hint-enabled"),
                );
                ui.add(toggle(&mut self.config_form.enabled));
                ui.end_row();

                label_with_hint(
                    ui,
                    &t.text("voice-cfg-label-default-language"),
                    &t.text("voice-cfg-label-default-language"),
                );
                ui.text_edit_singleline(&mut self.config_form.default_language);
                ui.end_row();

                label_with_hint(
                    ui,
                    &t.text("voice-cfg-label-default-voice-id"),
                    &t.text("voice-cfg-label-default-voice-id"),
                );
                ui.text_edit_singleline(&mut self.config_form.default_voice_id);
                ui.end_row();

                label_with_hint(
                    ui,
                    &t.text("voice-cfg-label-stt-provider"),
                    &t.text("voice-cfg-label-stt-provider"),
                );
                egui::ComboBox::from_id_salt("voice-stt-provider")
                    .selected_text(self.config_form.stt_provider.as_str())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.config_form.stt_provider,
                            SttProviderKind::Deepgram,
                            SttProviderKind::Deepgram.as_str(),
                        );
                        ui.selectable_value(
                            &mut self.config_form.stt_provider,
                            SttProviderKind::Assemblyai,
                            SttProviderKind::Assemblyai.as_str(),
                        );
                    });
                ui.end_row();

                label_with_hint(
                    ui,
                    &t.text("voice-cfg-label-tts-provider"),
                    &t.text("voice-cfg-label-tts-provider"),
                );
                egui::ComboBox::from_id_salt("voice-tts-provider")
                    .selected_text(self.config_form.tts_provider.as_str())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.config_form.tts_provider,
                            TtsProviderKind::Elevenlabs,
                            TtsProviderKind::Elevenlabs.as_str(),
                        );
                    });
                ui.end_row();
            });
    }

    fn render_test_mode_tabs(&mut self, ui: &mut egui::Ui) {
        let t = Self::translator();
        ui.horizontal(|ui| {
            for mode in [VoiceTestMode::Stt, VoiceTestMode::Tts] {
                let selected = self.test_mode == mode;
                let label = format!("{} {}", mode.icon(), mode.label(&t));
                if ui.selectable_label(selected, label).clicked() {
                    self.test_mode = mode;
                }
            }
        });
    }

    fn render_stt_test_section(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
    ) {
        let t = Self::translator();
        ui.label(t.text("voice-stt-subtitle"));
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            let recording = self.recording.is_some();
            if ui
                .add_enabled(
                    !recording && self.stt_result_rx.is_none(),
                    egui::Button::new(t.text_args(
                        "voice-stt-btn-start",
                        HashMap::from([("icon", regular::MICROPHONE.to_string())]),
                    )),
                )
                .clicked()
            {
                self.start_recording(notifications);
            }
            if ui
                .add_enabled(
                    recording,
                    egui::Button::new(t.text_args(
                        "voice-stt-btn-stop",
                        HashMap::from([("icon", regular::STOP.to_string())]),
                    )),
                )
                .clicked()
            {
                self.stop_recording(notifications);
            }
        });

        match &self.stt_state {
            SttTestState::Idle => {
                ui.label(t.text("voice-stt-idle-hint"));
            }
            SttTestState::Recording {
                started_at,
                device_name,
                sample_rate_hz,
                channels,
            } => {
                ui.horizontal(|ui| {
                    ui.colored_label(Color32::from_rgb(220, 38, 38), "●");
                    ui.label(
                        RichText::new(t.text("voice-stt-recording-label"))
                            .color(Color32::from_rgb(220, 38, 38))
                            .strong(),
                    );
                });
                let elapsed_ms = started_at.elapsed().as_millis() as u64;
                ui.label(t.text_args(
                    "voice-stt-recording-detail",
                    HashMap::from([
                        ("device", device_name.clone()),
                        ("sample_rate", sample_rate_hz.to_string()),
                        ("channels", channels.to_string()),
                        ("elapsed", elapsed_ms.to_string()),
                    ]),
                ));
            }
            SttTestState::Transcribing {
                started_at,
                capture_duration_ms,
            } => {
                let queued_ms = started_at.elapsed().as_millis();
                ui.label(t.text_args(
                    "voice-stt-transcribing-detail",
                    HashMap::from([
                        ("duration", capture_duration_ms.to_string()),
                        ("queued", queued_ms.to_string()),
                    ]),
                ));
            }
            SttTestState::Completed(result) => {
                egui::Grid::new("voice-stt-result-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(t.text("voice-stt-col-provider"));
                        ui.monospace(&result.provider_name);
                        ui.end_row();

                        ui.label(t.text("voice-stt-col-input-device"));
                        ui.label(&result.device_name);
                        ui.end_row();

                        ui.label(t.text("voice-stt-col-capture-duration"));
                        ui.label(t.text_args(
                            "voice-stt-duration-ms",
                            HashMap::from([("value", result.capture_duration_ms.to_string())]),
                        ));
                        ui.end_row();

                        ui.label(t.text("voice-stt-col-audio-format"));
                        ui.label(t.text_args(
                            "voice-stt-audio-format-detail",
                            HashMap::from([
                                ("sample_rate", result.sample_rate_hz.to_string()),
                                ("channels", result.channels.to_string()),
                                ("samples", result.sample_count.to_string()),
                            ]),
                        ));
                        ui.end_row();

                        ui.label(t.text("voice-stt-col-detected-language"));
                        ui.label(result.language.as_deref().unwrap_or("-"));
                        ui.end_row();

                        ui.label(t.text("voice-stt-col-confidence"));
                        ui.label(
                            result
                                .confidence
                                .map(|value| format!("{value:.3}"))
                                .unwrap_or_else(|| "-".to_string()),
                        );
                        ui.end_row();

                        ui.label(t.text("voice-stt-col-provider-duration"));
                        ui.label(
                            result
                                .duration_ms
                                .map(|value| {
                                    t.text_args(
                                        "voice-stt-provider-duration-value",
                                        HashMap::from([("value", value.to_string())]),
                                    )
                                })
                                .unwrap_or_else(|| "-".to_string()),
                        );
                        ui.end_row();
                    });
                ui.add_space(8.0);
                ui.strong(t.text("voice-stt-section-transcript"));
                let mut transcript = result.transcript.clone();
                ui.add(
                    egui::TextEdit::multiline(&mut transcript)
                        .desired_rows(6)
                        .interactive(false),
                );
            }
            SttTestState::Failed(err) => {
                ui.colored_label(ui.visuals().error_fg_color, err);
            }
        }
    }

    fn render_tts_test_section(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
    ) {
        let t = Self::translator();
        ui.label(t.text("voice-tts-subtitle"));
        ui.add_space(6.0);

        label_with_hint(
            ui,
            &t.text("voice-tts-label-text"),
            &t.text("voice-tts-hint-text"),
        );
        ui.add(
            egui::TextEdit::multiline(&mut self.tts_input_text)
                .desired_rows(TTS_INPUT_ROWS)
                .hint_text(t.text("voice-tts-hint-text")),
        );
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("voice-tts-label-voice-id"),
                &t.text("voice-tts-hint-voice-id"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.tts_voice_id)
                    .hint_text(t.text("voice-tts-hint-voice-id")),
            );
        });
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.tts_result_rx.is_none(),
                    egui::Button::new(t.text_args(
                        "voice-tts-btn-generate",
                        HashMap::from([("icon", regular::WAVEFORM.to_string())]),
                    )),
                )
                .clicked()
            {
                self.start_tts_generation(notifications);
            }

            let can_play = matches!(self.tts_state, TtsTestState::Completed(_));
            if ui
                .add_enabled(
                    can_play,
                    egui::Button::new(t.text_args(
                        "voice-tts-btn-play",
                        HashMap::from([("icon", regular::PLAY.to_string())]),
                    )),
                )
                .clicked()
            {
                self.play_tts_output(notifications);
            }

            let playing = self.playback.is_some();
            if ui
                .add_enabled(
                    playing,
                    egui::Button::new(t.text_args(
                        "voice-tts-btn-stop",
                        HashMap::from([("icon", regular::STOP.to_string())]),
                    )),
                )
                .clicked()
            {
                self.stop_playback();
                notifications.info(t.text("voice-notify-tts-stopped"));
            }
        });

        match &self.tts_state {
            TtsTestState::Idle => {
                ui.label(t.text("voice-tts-idle-hint"));
            }
            TtsTestState::Synthesizing { started_at } => {
                let queued_ms = started_at.elapsed().as_millis();
                ui.label(t.text_args(
                    "voice-tts-synthesizing-detail",
                    HashMap::from([("queued", queued_ms.to_string())]),
                ));
            }
            TtsTestState::Completed(result) => {
                egui::Grid::new("voice-tts-result-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(t.text("voice-tts-col-provider"));
                        ui.monospace(&result.provider_name);
                        ui.end_row();

                        ui.label(t.text("voice-tts-col-mime-type"));
                        ui.monospace(&result.mime_type);
                        ui.end_row();

                        ui.label(t.text("voice-tts-col-output-size"));
                        ui.label(t.text_args(
                            "voice-tts-output-size-detail",
                            HashMap::from([("value", result.output_size_bytes.to_string())]),
                        ));
                        ui.end_row();

                        ui.label(t.text("voice-tts-col-provider-duration"));
                        ui.label(
                            result
                                .duration_ms
                                .map(|value| {
                                    t.text_args(
                                        "voice-tts-provider-duration-value",
                                        HashMap::from([("value", value.to_string())]),
                                    )
                                })
                                .unwrap_or_else(|| "-".to_string()),
                        );
                        ui.end_row();

                        ui.label(t.text("voice-tts-col-voice-id"));
                        ui.label(
                            result
                                .requested_voice_id
                                .as_deref()
                                .unwrap_or(&t.text("voice-tts-voice-id-config-default")),
                        );
                        ui.end_row();

                        ui.label(t.text("voice-tts-col-saved-to"));
                        ui.monospace(result.output_path.display().to_string());
                        ui.end_row();

                        ui.label(t.text("voice-tts-col-playback"));
                        if let Some(playback) = self.playback.as_ref() {
                            ui.label(t.text_args(
                                "voice-tts-playback-playing",
                                HashMap::from([("path", playback.path.display().to_string())]),
                            ));
                        } else {
                            ui.label(t.text("voice-tts-playback-idle"));
                        }
                        ui.end_row();
                    });
            }
            TtsTestState::Failed(err) => {
                ui.colored_label(ui.visuals().error_fg_color, err);
            }
        }
    }
}

struct VoiceConfigTabViewer<'a> {
    panel: &'a mut VoicePanel,
    translator: &'a Translator,
}

impl egui_dock::TabViewer for VoiceConfigTabViewer<'_> {
    type Tab = VoiceConfigTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        format!("{} {}", tab.icon(), self.translator.text(tab.title_key())).into()
    }

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(("voice-config-tab", tab.tab_id()))
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        self.panel.config_tab = *tab;
        egui::ScrollArea::vertical()
            .id_salt(("voice-config-tab-scroll", tab.tab_id()))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.panel.render_config_tab_content(ui, *tab);
            });
    }

    fn is_closeable(&self, _tab: &Self::Tab) -> bool {
        false
    }

    fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &egui::Response) {
        if response.clicked() {
            self.panel.config_tab = *tab;
        }
    }

    fn allowed_in_windows(&self, _tab: &mut Self::Tab) -> bool {
        false
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false]
    }
}

impl PanelRenderer for VoicePanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        let t = Self::translator();
        self.ensure_store_loaded(notifications);
        self.poll_stt_result(notifications);
        self.poll_tts_result(notifications);
        self.poll_playback();

        ui.heading(ctx.tab_title);
        ui.label(t.text("voice-subtitle"));
        ui.separator();

        ui.horizontal(|ui| {
            if ui
                .button(t.text_args(
                    "voice-btn-config",
                    HashMap::from([("icon", regular::SLIDERS.to_string())]),
                ))
                .clicked()
            {
                self.open_config_window();
            }
            if ui
                .button(t.text_args(
                    "voice-btn-reload",
                    HashMap::from([("icon", regular::ARROWS_CLOCKWISE.to_string())]),
                ))
                .clicked()
            {
                self.reload_config(notifications);
            }
        });

        ui.add_space(8.0);
        ui.strong(t.text("voice-section-current-config"));
        egui::Grid::new("voice-summary-grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label(t.text("voice-col-enabled"));
                render_enabled_status(ui, self.config.voice.enabled);
                ui.end_row();

                ui.label(t.text("voice-col-stt-provider"));
                ui.monospace(self.config.voice.stt_provider.as_str());
                ui.end_row();

                ui.label(t.text("voice-col-tts-provider"));
                ui.monospace(self.config.voice.tts_provider.as_str());
                ui.end_row();

                ui.label(t.text("voice-col-default-language"));
                ui.label(&self.config.voice.default_language);
                ui.end_row();

                ui.label(t.text("voice-col-default-voice-id"));
                ui.label(self.config.voice.default_voice_id.as_deref().unwrap_or("-"));
                ui.end_row();
            });

        ui.separator();
        ui.strong(t.text("voice-section-voice-tests"));
        self.render_test_mode_tabs(ui);
        ui.add_space(8.0);

        match self.test_mode {
            VoiceTestMode::Stt => self.render_stt_test_section(ui, notifications),
            VoiceTestMode::Tts => self.render_tts_test_section(ui, notifications),
        }

        if matches!(
            self.stt_state,
            SttTestState::Recording { .. } | SttTestState::Transcribing { .. }
        ) || matches!(self.tts_state, TtsTestState::Synthesizing { .. })
            || self.playback.is_some()
        {
            ui.ctx().request_repaint_after(VOICE_POLL_INTERVAL);
        }

        if self.config_window_open {
            self.render_config_window(ui.ctx(), notifications);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_secret_provider_section(
    ui: &mut egui::Ui,
    id_prefix: &str,
    api_key: &mut String,
    api_key_env: &mut String,
    base_url: &mut String,
    streaming_base_url: &mut String,
    primary_extra: Option<(&mut String, String)>,
    secondary_extra: Option<(&mut String, String)>,
    t: &Translator,
) {
    egui::Grid::new(id_prefix)
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            label_with_hint(
                ui,
                &t.text("voice-cfg-label-api-key"),
                &t.text("voice-cfg-label-api-key"),
            );
            ui.add(egui::TextEdit::singleline(api_key).password(true));
            ui.end_row();

            label_with_hint(
                ui,
                &t.text("voice-cfg-label-api-key-env"),
                &t.text("voice-cfg-label-api-key-env"),
            );
            ui.text_edit_singleline(api_key_env);
            ui.end_row();

            label_with_hint(
                ui,
                &t.text("voice-cfg-label-base-url"),
                &t.text("voice-cfg-label-base-url"),
            );
            ui.text_edit_singleline(base_url);
            ui.end_row();

            label_with_hint(
                ui,
                &t.text("voice-cfg-label-streaming-base-url"),
                &t.text("voice-cfg-label-streaming-base-url"),
            );
            ui.text_edit_singleline(streaming_base_url);
            ui.end_row();

            if let Some((value, label)) = primary_extra {
                ui.label(label);
                ui.text_edit_singleline(value);
                ui.end_row();
            }

            if let Some((value, label)) = secondary_extra {
                ui.label(label);
                ui.text_edit_singleline(value);
                ui.end_row();
            }
        });
}

fn normalize_optional(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn render_enabled_status(ui: &mut egui::Ui, enabled: bool) {
    let (icon, color, label) = if enabled {
        (
            regular::CHECK_CIRCLE,
            Color32::from_rgb(0x22, 0xC5, 0x5E),
            "true",
        )
    } else {
        (
            regular::X_CIRCLE,
            Color32::from_rgb(0xEF, 0x44, 0x44),
            "false",
        )
    };
    ui.label(
        RichText::new(format!("{icon} {label}"))
            .color(color)
            .strong(),
    );
}

fn validate_stt_test_config(config: &AppConfig) -> Result<(), String> {
    let stt_has_key = match config.voice.stt_provider {
        SttProviderKind::Deepgram => config.voice.providers.deepgram.resolve_api_key().is_some(),
        SttProviderKind::Assemblyai => config
            .voice
            .providers
            .assemblyai
            .resolve_api_key()
            .is_some(),
    };
    if !stt_has_key {
        return Err(format!(
            "Selected STT provider '{}' is missing api_key or api_key_env",
            config.voice.stt_provider.as_str()
        ));
    }
    Ok(())
}

fn validate_tts_test_config(config: &AppConfig) -> Result<(), String> {
    let tts_has_key = match config.voice.tts_provider {
        TtsProviderKind::Elevenlabs => config
            .voice
            .providers
            .elevenlabs
            .resolve_api_key()
            .is_some(),
    };
    if !tts_has_key {
        return Err(format!(
            "Selected TTS provider '{}' is missing api_key or api_key_env",
            config.voice.tts_provider.as_str()
        ));
    }
    Ok(())
}

fn tts_file_extension_for_mime_type(mime_type: &str) -> &'static str {
    let normalized = mime_type.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
        "audio/ogg" => "ogg",
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" => "m4a",
        "audio/aac" => "aac",
        _ => "bin",
    }
}

fn build_tts_temp_path(mime_type: &str) -> PathBuf {
    let extension = tts_file_extension_for_mime_type(mime_type);
    std::env::temp_dir().join(format!("klaw-voice-tts-{}.{}", Uuid::new_v4(), extension))
}

fn run_transcription_test(
    capture: RecordingCapture,
    voice_config: VoiceConfig,
) -> Result<SttTestResult, String> {
    let provider_name = voice_config.stt_provider.as_str().to_string();
    let device_name = capture.device_name.clone();
    let capture_duration_ms = capture.duration_ms;
    let sample_rate_hz = capture.sample_rate_hz;
    let channels = capture.channels;
    let sample_count = capture.sample_count;
    let wav_bytes = capture.wav_bytes;

    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("Failed to build voice test runtime: {err}"))?;

    runtime.block_on(async move {
        let language = (!voice_config.default_language.trim().is_empty())
            .then(|| voice_config.default_language.clone());
        let service = VoiceService::from_config(&voice_config)
            .map_err(|err| format!("Voice config error: {err}"))?;
        let result = service
            .transcribe(SttInput {
                audio_bytes: wav_bytes,
                mime_type: "audio/wav".to_string(),
                language,
            })
            .await
            .map_err(|err| format!("Voice transcription failed: {err}"))?;

        Ok(SttTestResult {
            transcript: result.text,
            provider_name,
            language: result.language,
            confidence: result.confidence,
            duration_ms: result.duration_ms,
            capture_duration_ms,
            sample_rate_hz,
            channels,
            device_name,
            sample_count,
        })
    })
}

fn run_tts_test(
    text: String,
    requested_voice_id: Option<String>,
    voice_config: VoiceConfig,
) -> Result<TtsTestResult, String> {
    let provider_name = voice_config.tts_provider.as_str().to_string();

    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("Failed to build voice test runtime: {err}"))?;

    runtime.block_on(async move {
        let service = VoiceService::from_config(&voice_config)
            .map_err(|err| format!("Voice config error: {err}"))?;
        let output = service
            .synthesize(TtsInput {
                text,
                voice_id: requested_voice_id.clone(),
                language: None,
                speed: None,
            })
            .await
            .map_err(|err| format!("Voice synthesis failed: {err}"))?;

        let output_path = build_tts_temp_path(&output.mime_type);
        fs::write(&output_path, &output.audio_bytes)
            .map_err(|err| format!("Failed to write generated audio to tmp: {err}"))?;

        Ok(TtsTestResult {
            provider_name,
            mime_type: output.mime_type,
            duration_ms: output.duration_ms,
            output_size_bytes: output.audio_bytes.len(),
            output_path,
            requested_voice_id,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{
        VoiceConfigForm, build_tts_temp_path, normalize_optional, tts_file_extension_for_mime_type,
        validate_stt_test_config, validate_tts_test_config,
    };
    use klaw_config::{AppConfig, SttProviderKind, TtsProviderKind, VoiceConfig};

    fn sample_app_config() -> AppConfig {
        let mut config = AppConfig {
            voice: VoiceConfig {
                enabled: true,
                stt_provider: SttProviderKind::Deepgram,
                tts_provider: TtsProviderKind::Elevenlabs,
                default_language: "zh-CN".to_string(),
                default_voice_id: Some("voice-1".to_string()),
                ..VoiceConfig::default()
            },
            ..AppConfig::default()
        };
        config.voice.providers.deepgram.api_key = Some("dg".to_string());
        config.voice.providers.assemblyai.api_key = Some("aa".to_string());
        config.voice.providers.elevenlabs.api_key = Some("el".to_string());
        config
    }

    #[test]
    fn form_maps_back_to_voice_config() {
        let mut config = AppConfig::default();
        let form = VoiceConfigForm {
            enabled: true,
            stt_provider: SttProviderKind::Assemblyai,
            tts_provider: TtsProviderKind::Elevenlabs,
            default_language: "en-US".to_string(),
            default_voice_id: "voice-42".to_string(),
            deepgram_api_key: "dg-key".to_string(),
            deepgram_api_key_env: "DEEPGRAM_API_KEY".to_string(),
            deepgram_base_url: "https://api.deepgram.com".to_string(),
            deepgram_streaming_base_url: "wss://api.deepgram.com".to_string(),
            deepgram_stt_model: "nova-2".to_string(),
            assemblyai_api_key: "aa-key".to_string(),
            assemblyai_api_key_env: "ASSEMBLYAI_API_KEY".to_string(),
            assemblyai_base_url: "https://api.assemblyai.com".to_string(),
            assemblyai_streaming_base_url: "wss://streaming.assemblyai.com".to_string(),
            assemblyai_stt_model: "universal".to_string(),
            elevenlabs_api_key: "el-key".to_string(),
            elevenlabs_api_key_env: "ELEVENLABS_API_KEY".to_string(),
            elevenlabs_base_url: "https://api.elevenlabs.io".to_string(),
            elevenlabs_streaming_base_url: "wss://api.elevenlabs.io".to_string(),
            elevenlabs_default_model: "eleven_multilingual_v2".to_string(),
            elevenlabs_default_voice_id: "el-voice".to_string(),
        };

        form.apply_to_config(&mut config)
            .expect("form should apply");
        assert!(config.voice.enabled);
        assert_eq!(config.voice.stt_provider, SttProviderKind::Assemblyai);
        assert_eq!(config.voice.default_language, "en-US");
        assert_eq!(config.voice.default_voice_id.as_deref(), Some("voice-42"));
        assert_eq!(
            config.voice.providers.assemblyai.api_key.as_deref(),
            Some("aa-key")
        );
        assert_eq!(
            config
                .voice
                .providers
                .elevenlabs
                .default_voice_id
                .as_deref(),
            Some("el-voice")
        );
    }

    #[test]
    fn blank_option_fields_are_normalized_to_none() {
        assert_eq!(normalize_optional("   "), None);
        assert_eq!(normalize_optional(" voice "), Some("voice".to_string()));
    }

    #[test]
    fn provider_configs_are_preserved_when_switching_selection() {
        let config = sample_app_config();
        let mut form = VoiceConfigForm::from_config(&config.voice);
        form.stt_provider = SttProviderKind::Assemblyai;

        let mut next = config.clone();
        form.apply_to_config(&mut next).expect("form should apply");

        assert_eq!(next.voice.providers.deepgram.api_key.as_deref(), Some("dg"));
        assert_eq!(
            next.voice.providers.assemblyai.api_key.as_deref(),
            Some("aa")
        );
    }

    #[test]
    fn stt_validation_allows_disabled_voice_when_provider_key_exists() {
        let mut config = sample_app_config();
        config.voice.enabled = false;
        validate_stt_test_config(&config).expect("disabled voice flag should not block tests");
    }

    #[test]
    fn stt_validation_rejects_missing_selected_provider_key() {
        let mut config = sample_app_config();
        config.voice.providers.deepgram.api_key = None;
        config.voice.providers.deepgram.api_key_env.clear();
        let err = validate_stt_test_config(&config).expect_err("missing stt key should fail");
        assert!(err.contains("missing api_key or api_key_env"));
    }

    #[test]
    fn tts_validation_allows_disabled_voice_when_provider_key_exists() {
        let mut config = sample_app_config();
        config.voice.enabled = false;
        validate_tts_test_config(&config).expect("disabled voice flag should not block tests");
    }

    #[test]
    fn tts_validation_rejects_missing_selected_provider_key() {
        let mut config = sample_app_config();
        config.voice.providers.elevenlabs.api_key = None;
        config.voice.providers.elevenlabs.api_key_env.clear();
        let err = validate_tts_test_config(&config).expect_err("missing tts key should fail");
        assert!(err.contains("missing api_key or api_key_env"));
    }

    #[test]
    fn mime_type_maps_to_expected_tts_extension() {
        assert_eq!(tts_file_extension_for_mime_type("audio/mpeg"), "mp3");
        assert_eq!(tts_file_extension_for_mime_type("audio/wav"), "wav");
        assert_eq!(tts_file_extension_for_mime_type("audio/ogg"), "ogg");
        assert_eq!(tts_file_extension_for_mime_type("audio/mp4"), "m4a");
        assert_eq!(
            tts_file_extension_for_mime_type("application/octet-stream"),
            "bin"
        );
    }

    #[test]
    fn tts_temp_path_uses_expected_prefix_and_extension() {
        let path = build_tts_temp_path("audio/mpeg");
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        assert!(path.starts_with(std::env::temp_dir()));
        assert!(name.starts_with("klaw-voice-tts-"));
        assert!(name.ends_with(".mp3"));
    }
}
