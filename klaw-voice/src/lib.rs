pub mod error;
pub mod model;
pub mod provider;
pub mod providers;
pub mod service;
pub mod stream;

pub use error::VoiceError;
pub use model::{
    AudioByteStream, SttInput, SttOutput, SttSegment, SttStreamInput, SttStreamOutput, TtsInput,
    TtsOutput, TtsStreamInput, TtsStreamOutput,
};
pub use provider::{VoiceCapabilities, VoiceProvider};
pub use service::{VoiceRuntimeConfig, VoiceService};
