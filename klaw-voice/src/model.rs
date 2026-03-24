use crate::VoiceError;
use bytes::Bytes;
use futures_util::Stream;
use std::pin::Pin;

pub type AudioByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, VoiceError>> + Send>>;
pub type TranscriptStream = Pin<Box<dyn Stream<Item = Result<SttSegment, VoiceError>> + Send>>;

#[derive(Debug, Clone)]
pub struct SttInput {
    pub audio_bytes: Vec<u8>,
    pub mime_type: String,
    pub language: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SttOutput {
    pub text: String,
    pub language: Option<String>,
    pub confidence: Option<f32>,
    pub duration_ms: Option<u64>,
}

pub struct SttStreamInput {
    pub audio_stream: AudioByteStream,
    pub mime_type: String,
    pub language: Option<String>,
    pub sample_rate_hz: Option<u32>,
}

impl std::fmt::Debug for SttStreamInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SttStreamInput")
            .field("mime_type", &self.mime_type)
            .field("language", &self.language)
            .field("sample_rate_hz", &self.sample_rate_hz)
            .finish_non_exhaustive()
    }
}

pub struct SttStreamOutput {
    pub transcript_stream: TranscriptStream,
}

impl std::fmt::Debug for SttStreamOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SttStreamOutput").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct SttSegment {
    pub text: String,
    pub is_final: bool,
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct TtsInput {
    pub text: String,
    pub voice_id: Option<String>,
    pub language: Option<String>,
    pub speed: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct TtsOutput {
    pub audio_bytes: Vec<u8>,
    pub mime_type: String,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct TtsStreamInput {
    pub text: String,
    pub voice_id: Option<String>,
    pub language: Option<String>,
    pub speed: Option<f32>,
}

pub struct TtsStreamOutput {
    pub audio_stream: AudioByteStream,
    pub mime_type: String,
}

impl std::fmt::Debug for TtsStreamOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TtsStreamOutput")
            .field("mime_type", &self.mime_type)
            .finish_non_exhaustive()
    }
}
