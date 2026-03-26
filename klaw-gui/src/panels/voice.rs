use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::voice_test::{RecordingCapture, RecordingHandle};
use egui::{Color32, RichText};
use egui_phosphor::regular;
use klaw_config::{
    AppConfig, AssemblyAiVoiceConfig, ConfigError, ConfigSnapshot, ConfigStore,
    DeepgramVoiceConfig, ElevenLabsVoiceConfig, SttProviderKind, TtsProviderKind, VoiceConfig,
};
use klaw_voice::{SttInput, TtsInput, VoiceService};
use rodio::{Decoder, OutputStream, Sink};
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};
use tokio::runtime::Builder;
use uuid::Uuid;

const VOICE_POLL_INTERVAL: Duration = Duration::from_millis(200);
const TTS_INPUT_ROWS: usize = 6;

#[derive(Debug, Clone)]
struct VoiceConfigForm {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VoiceTestMode {
    Stt,
    Tts,
}

impl VoiceTestMode {
    fn label(self) -> &'static str {
        match self {
            Self::Stt => "STT Test",
            Self::Tts => "TTS Test",
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
    config_path: Option<PathBuf>,
    config: AppConfig,
    config_form: VoiceConfigForm,
    config_window_open: bool,
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
            config_path: None,
            config: AppConfig::default(),
            config_form: VoiceConfigForm::default(),
            config_window_open: false,
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
        self.config_path = Some(snapshot.path);
        self.config = snapshot.config;
        self.config_form = VoiceConfigForm::from_config(&self.config.voice);
    }

    fn open_config_window(&mut self) {
        self.config_form = VoiceConfigForm::from_config(&self.config.voice);
        self.config_window_open = true;
    }

    fn save_config(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
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
                notifications.success("Voice config saved");
            }
            Err(err) => notifications.error(format!("Save failed: {err}")),
        }
    }

    fn reload_config(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };
        match store.reload() {
            Ok(snapshot) => {
                self.apply_snapshot(snapshot);
                notifications.success("Voice config reloaded from disk");
            }
            Err(err) => notifications.error(format!("Reload failed: {err}")),
        }
    }

    fn poll_stt_result(&mut self, notifications: &mut NotificationCenter) {
        let Some(rx) = self.stt_result_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(result)) => {
                self.stt_result_rx = None;
                self.stt_state = SttTestState::Completed(result);
                notifications.success("Voice transcription test completed");
            }
            Ok(Err(err)) => {
                self.stt_result_rx = None;
                self.stt_state = SttTestState::Failed(err.clone());
                notifications.error(err);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.stt_result_rx = None;
                let message = "Voice STT test worker disconnected unexpectedly".to_string();
                self.stt_state = SttTestState::Failed(message.clone());
                notifications.error(message);
            }
        }
    }

    fn poll_tts_result(&mut self, notifications: &mut NotificationCenter) {
        let Some(rx) = self.tts_result_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(result)) => {
                self.tts_result_rx = None;
                self.tts_state = TtsTestState::Completed(result.clone());
                notifications.success(format!(
                    "Voice synthesis completed and saved to {}",
                    result.output_path.display()
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
                let message = "Voice TTS test worker disconnected unexpectedly".to_string();
                self.tts_state = TtsTestState::Failed(message.clone());
                notifications.error(message);
            }
        }
    }

    fn start_recording(&mut self, notifications: &mut NotificationCenter) {
        if self.recording.is_some() {
            notifications.info("Recording is already in progress");
            return;
        }
        if self.stt_result_rx.is_some() {
            notifications.info("Transcription is still running");
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
                notifications.success("Microphone recording started");
                self.recording = Some(handle);
            }
            Err(err) => {
                self.stt_state = SttTestState::Failed(err.clone());
                notifications.error(err);
            }
        }
    }

    fn stop_recording(&mut self, notifications: &mut NotificationCenter) {
        let Some(handle) = self.recording.take() else {
            notifications.info("No recording is currently running");
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
        notifications.info("Recording stopped. Uploading audio for transcription...");
    }

    fn start_tts_generation(&mut self, notifications: &mut NotificationCenter) {
        if self.tts_result_rx.is_some() {
            notifications.info("Synthesis is still running");
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
        notifications.info("Submitting text to the configured TTS provider...");
    }

    fn play_tts_output(&mut self, notifications: &mut NotificationCenter) {
        let TtsTestState::Completed(result) = &self.tts_state else {
            notifications.info("Generate TTS audio before playback");
            return;
        };
        let result = result.clone();

        self.stop_playback();

        let file = match File::open(&result.output_path) {
            Ok(file) => file,
            Err(err) => {
                notifications.error(format!("Failed to open generated audio: {err}"));
                return;
            }
        };
        let stream = match OutputStream::try_default() {
            Ok(stream) => stream,
            Err(err) => {
                notifications.error(format!("Failed to open audio output device: {err}"));
                return;
            }
        };
        let sink = match Sink::try_new(&stream.1) {
            Ok(sink) => sink,
            Err(err) => {
                notifications.error(format!("Failed to create playback sink: {err}"));
                return;
            }
        };
        let decoder = match Decoder::new(BufReader::new(file)) {
            Ok(decoder) => decoder,
            Err(err) => {
                notifications.error(format!("Failed to decode generated audio: {err}"));
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
        notifications.success("Playing generated audio");
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
        let mut open = self.config_window_open;
        egui::Window::new("Voice Config")
            .id(egui::Id::new("voice-config-window"))
            .open(&mut open)
            .resizable(true)
            .default_width(720.0)
            .show(ctx, |ui| {
                ui.label("Edit voice provider configuration stored in config.toml.");
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Grid::new("voice-config-grid")
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Default Language");
                            ui.text_edit_singleline(&mut self.config_form.default_language);
                            ui.end_row();

                            ui.label("Default Voice ID");
                            ui.text_edit_singleline(&mut self.config_form.default_voice_id);
                            ui.end_row();

                            ui.label("STT Provider");
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

                            ui.label("TTS Provider");
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

                    ui.separator();
                    ui.strong("Deepgram");
                    render_secret_provider_section(
                        ui,
                        "voice-deepgram",
                        &mut self.config_form.deepgram_api_key,
                        &mut self.config_form.deepgram_api_key_env,
                        &mut self.config_form.deepgram_base_url,
                        &mut self.config_form.deepgram_streaming_base_url,
                        Some((&mut self.config_form.deepgram_stt_model, "STT Model")),
                        None,
                    );

                    ui.separator();
                    ui.strong("AssemblyAI");
                    render_secret_provider_section(
                        ui,
                        "voice-assemblyai",
                        &mut self.config_form.assemblyai_api_key,
                        &mut self.config_form.assemblyai_api_key_env,
                        &mut self.config_form.assemblyai_base_url,
                        &mut self.config_form.assemblyai_streaming_base_url,
                        Some((&mut self.config_form.assemblyai_stt_model, "STT Model")),
                        None,
                    );

                    ui.separator();
                    ui.strong("ElevenLabs");
                    render_secret_provider_section(
                        ui,
                        "voice-elevenlabs",
                        &mut self.config_form.elevenlabs_api_key,
                        &mut self.config_form.elevenlabs_api_key_env,
                        &mut self.config_form.elevenlabs_base_url,
                        &mut self.config_form.elevenlabs_streaming_base_url,
                        Some((
                            &mut self.config_form.elevenlabs_default_model,
                            "Default Model",
                        )),
                        Some((
                            &mut self.config_form.elevenlabs_default_voice_id,
                            "Provider Default Voice ID",
                        )),
                    );
                });

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Reload").clicked() {
                        self.reload_config(notifications);
                    }
                    if ui.button("Save").clicked() {
                        self.save_config(notifications);
                    }
                });
            });
        self.config_window_open = open;
    }

    fn render_test_mode_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for mode in [VoiceTestMode::Stt, VoiceTestMode::Tts] {
                let selected = self.test_mode == mode;
                let label = format!("{} {}", mode.icon(), mode.label());
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
        ui.label("Capture live microphone audio and send it to the configured STT provider.");
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            let recording = self.recording.is_some();
            if ui
                .add_enabled(
                    !recording && self.stt_result_rx.is_none(),
                    egui::Button::new(format!("{} Start Recording", regular::MICROPHONE)),
                )
                .clicked()
            {
                self.start_recording(notifications);
            }
            if ui
                .add_enabled(
                    recording,
                    egui::Button::new(format!("{} Stop Recording", regular::STOP)),
                )
                .clicked()
            {
                self.stop_recording(notifications);
            }
        });

        match &self.stt_state {
            SttTestState::Idle => {
                ui.label("Press Start Recording to begin a microphone-to-transcript test.");
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
                        RichText::new("Recording")
                            .color(Color32::from_rgb(220, 38, 38))
                            .strong(),
                    );
                });
                let elapsed_ms = started_at.elapsed().as_millis() as u64;
                ui.label(format!(
                    "Recording from {device_name} at {sample_rate_hz} Hz / {channels} ch for {elapsed_ms} ms"
                ));
            }
            SttTestState::Transcribing {
                started_at,
                capture_duration_ms,
            } => {
                ui.label(format!(
                    "Transcribing {capture_duration_ms} ms recording... queued for {} ms",
                    started_at.elapsed().as_millis()
                ));
            }
            SttTestState::Completed(result) => {
                egui::Grid::new("voice-stt-result-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Provider");
                        ui.monospace(&result.provider_name);
                        ui.end_row();

                        ui.label("Input Device");
                        ui.label(&result.device_name);
                        ui.end_row();

                        ui.label("Capture Duration");
                        ui.label(format!("{} ms", result.capture_duration_ms));
                        ui.end_row();

                        ui.label("Audio Format");
                        ui.label(format!(
                            "{} Hz / {} ch / {} samples",
                            result.sample_rate_hz, result.channels, result.sample_count
                        ));
                        ui.end_row();

                        ui.label("Detected Language");
                        ui.label(result.language.as_deref().unwrap_or("-"));
                        ui.end_row();

                        ui.label("Confidence");
                        ui.label(
                            result
                                .confidence
                                .map(|value| format!("{value:.3}"))
                                .unwrap_or_else(|| "-".to_string()),
                        );
                        ui.end_row();

                        ui.label("Provider Duration");
                        ui.label(
                            result
                                .duration_ms
                                .map(|value| format!("{value} ms"))
                                .unwrap_or_else(|| "-".to_string()),
                        );
                        ui.end_row();
                    });
                ui.add_space(8.0);
                ui.strong("Transcript");
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
        ui.label("Enter text, synthesize it through the configured TTS provider, save it into tmp, and play it back inside the GUI.");
        ui.add_space(6.0);

        ui.label("Text");
        ui.add(
            egui::TextEdit::multiline(&mut self.tts_input_text)
                .desired_rows(TTS_INPUT_ROWS)
                .hint_text("Type text to synthesize into speech..."),
        );
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            ui.label("Voice ID");
            ui.add(
                egui::TextEdit::singleline(&mut self.tts_voice_id)
                    .hint_text("Optional override; otherwise config default is used"),
            );
        });
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.tts_result_rx.is_none(),
                    egui::Button::new(format!("{} Generate Audio", regular::WAVEFORM)),
                )
                .clicked()
            {
                self.start_tts_generation(notifications);
            }

            let can_play = matches!(self.tts_state, TtsTestState::Completed(_));
            if ui
                .add_enabled(
                    can_play,
                    egui::Button::new(format!("{} Play", regular::PLAY)),
                )
                .clicked()
            {
                self.play_tts_output(notifications);
            }

            let playing = self.playback.is_some();
            if ui
                .add_enabled(
                    playing,
                    egui::Button::new(format!("{} Stop", regular::STOP)),
                )
                .clicked()
            {
                self.stop_playback();
                notifications.info("Stopped generated audio playback");
            }
        });

        match &self.tts_state {
            TtsTestState::Idle => {
                ui.label("Generate audio to save a tmp file and enable in-app playback.");
            }
            TtsTestState::Synthesizing { started_at } => {
                ui.label(format!(
                    "Synthesizing audio... queued for {} ms",
                    started_at.elapsed().as_millis()
                ));
            }
            TtsTestState::Completed(result) => {
                egui::Grid::new("voice-tts-result-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Provider");
                        ui.monospace(&result.provider_name);
                        ui.end_row();

                        ui.label("MIME Type");
                        ui.monospace(&result.mime_type);
                        ui.end_row();

                        ui.label("Output Size");
                        ui.label(format!("{} bytes", result.output_size_bytes));
                        ui.end_row();

                        ui.label("Provider Duration");
                        ui.label(
                            result
                                .duration_ms
                                .map(|value| format!("{value} ms"))
                                .unwrap_or_else(|| "-".to_string()),
                        );
                        ui.end_row();

                        ui.label("Voice ID");
                        ui.label(
                            result
                                .requested_voice_id
                                .as_deref()
                                .unwrap_or("(config default)"),
                        );
                        ui.end_row();

                        ui.label("Saved To");
                        ui.monospace(result.output_path.display().to_string());
                        ui.end_row();

                        ui.label("Playback");
                        if let Some(playback) = self.playback.as_ref() {
                            ui.label(format!("Playing {}", playback.path.display()));
                        } else {
                            ui.label("Idle");
                        }
                        ui.end_row();
                    });
            }
            TtsTestState::Failed(err) => {
                ui.colored_label(ui.visuals().error_fg_color, err);
            }
        }
    }

    fn status_label(path: Option<&Path>) -> String {
        match path {
            Some(path) => format!("Path: {}", path.display()),
            None => "Path: (not loaded)".to_string(),
        }
    }
}

impl PanelRenderer for VoicePanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_store_loaded(notifications);
        self.poll_stt_result(notifications);
        self.poll_tts_result(notifications);
        self.poll_playback();

        ui.heading(ctx.tab_title);
        ui.label(Self::status_label(self.config_path.as_deref()));
        ui.label("Manage voice providers and run split STT/TTS voice tests.");
        ui.separator();

        ui.horizontal(|ui| {
            if ui.button("Config").clicked() {
                self.open_config_window();
            }
            if ui.button("Reload").clicked() {
                self.reload_config(notifications);
            }
        });

        ui.add_space(8.0);
        ui.strong("Current Config");
        egui::Grid::new("voice-summary-grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("STT Provider");
                ui.monospace(self.config.voice.stt_provider.as_str());
                ui.end_row();

                ui.label("TTS Provider");
                ui.monospace(self.config.voice.tts_provider.as_str());
                ui.end_row();

                ui.label("Default Language");
                ui.label(&self.config.voice.default_language);
                ui.end_row();

                ui.label("Default Voice ID");
                ui.label(self.config.voice.default_voice_id.as_deref().unwrap_or("-"));
                ui.end_row();

                ui.label("Deepgram Key Source");
                ui.label(key_source_label(
                    self.config.voice.providers.deepgram.api_key.as_deref(),
                    &self.config.voice.providers.deepgram.api_key_env,
                ));
                ui.end_row();

                ui.label("AssemblyAI Key Source");
                ui.label(key_source_label(
                    self.config.voice.providers.assemblyai.api_key.as_deref(),
                    &self.config.voice.providers.assemblyai.api_key_env,
                ));
                ui.end_row();

                ui.label("ElevenLabs Key Source");
                ui.label(key_source_label(
                    self.config.voice.providers.elevenlabs.api_key.as_deref(),
                    &self.config.voice.providers.elevenlabs.api_key_env,
                ));
                ui.end_row();
            });

        ui.separator();
        ui.strong("Voice Tests");
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

fn render_secret_provider_section(
    ui: &mut egui::Ui,
    id_prefix: &str,
    api_key: &mut String,
    api_key_env: &mut String,
    base_url: &mut String,
    streaming_base_url: &mut String,
    primary_extra: Option<(&mut String, &str)>,
    secondary_extra: Option<(&mut String, &str)>,
) {
    egui::Grid::new(id_prefix)
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("API Key");
            ui.add(egui::TextEdit::singleline(api_key).password(true));
            ui.end_row();

            ui.label("API Key Env");
            ui.text_edit_singleline(api_key_env);
            ui.end_row();

            ui.label("Base URL");
            ui.text_edit_singleline(base_url);
            ui.end_row();

            ui.label("Streaming Base URL");
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

fn key_source_label(direct_key: Option<&str>, env_key: &str) -> String {
    if direct_key.is_some_and(|value| !value.trim().is_empty()) {
        "direct api_key".to_string()
    } else if !env_key.trim().is_empty() {
        format!("env {}", env_key.trim())
    } else {
        "not configured".to_string()
    }
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
        let mut config = AppConfig::default();
        config.voice = VoiceConfig {
            enabled: true,
            stt_provider: SttProviderKind::Deepgram,
            tts_provider: TtsProviderKind::Elevenlabs,
            default_language: "zh-CN".to_string(),
            default_voice_id: Some("voice-1".to_string()),
            ..VoiceConfig::default()
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
