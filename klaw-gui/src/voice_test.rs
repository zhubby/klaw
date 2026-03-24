use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::io::Cursor;
use std::sync::{Arc, Mutex};
pub struct RecordingHandle {
    device_name: String,
    sample_rate_hz: u32,
    channels: u16,
    samples: Arc<Mutex<Vec<i16>>>,
    callback_error: Arc<Mutex<Option<String>>>,
    stream: cpal::Stream,
}

#[derive(Debug, Clone)]
pub struct RecordingCapture {
    pub device_name: String,
    pub wav_bytes: Vec<u8>,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub duration_ms: u64,
    pub sample_count: usize,
}

impl RecordingHandle {
    pub fn start_default() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "No default input device is available".to_string())?;
        let device_name = device
            .name()
            .unwrap_or_else(|_| "Default Input".to_string());
        let supported_config = device
            .default_input_config()
            .map_err(|err| format!("Failed to query default input config: {err}"))?;
        let sample_rate_hz = supported_config.sample_rate().0;
        let channels = supported_config.channels();
        let stream_config: cpal::StreamConfig = supported_config.config();
        let samples = Arc::new(Mutex::new(Vec::new()));
        let callback_error = Arc::new(Mutex::new(None));
        let stream = match supported_config.sample_format() {
            cpal::SampleFormat::I16 => build_input_stream_i16(
                &device,
                &stream_config,
                Arc::clone(&samples),
                Arc::clone(&callback_error),
            )?,
            cpal::SampleFormat::U16 => build_input_stream_u16(
                &device,
                &stream_config,
                Arc::clone(&samples),
                Arc::clone(&callback_error),
            )?,
            cpal::SampleFormat::F32 => build_input_stream_f32(
                &device,
                &stream_config,
                Arc::clone(&samples),
                Arc::clone(&callback_error),
            )?,
            other => {
                return Err(format!("Unsupported microphone sample format: {other:?}"));
            }
        };

        stream
            .play()
            .map_err(|err| format!("Failed to start microphone stream: {err}"))?;

        Ok(Self {
            device_name,
            sample_rate_hz,
            channels,
            samples,
            callback_error,
            stream,
        })
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }
    pub fn finish(self) -> Result<RecordingCapture, String> {
        drop(self.stream);

        let callback_error = self
            .callback_error
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        if let Some(err) = callback_error {
            return Err(format!("Microphone capture failed: {err}"));
        }

        let samples = self
            .samples
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        if samples.is_empty() {
            return Err("No audio data was captured from the microphone".to_string());
        }

        let wav_bytes = encode_wav(&samples, self.sample_rate_hz, self.channels)?;
        let frames = samples.len() as u64 / u64::from(self.channels.max(1));
        let duration_ms = frames.saturating_mul(1000) / u64::from(self.sample_rate_hz.max(1));

        Ok(RecordingCapture {
            device_name: self.device_name,
            wav_bytes,
            sample_rate_hz: self.sample_rate_hz,
            channels: self.channels,
            duration_ms,
            sample_count: samples.len(),
        })
    }
}

fn build_input_stream_i16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Arc<Mutex<Vec<i16>>>,
    callback_error: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, String> {
    device
        .build_input_stream(
            config,
            move |data: &[i16], _| {
                let mut guard = samples.lock().unwrap_or_else(|err| err.into_inner());
                guard.extend_from_slice(data);
            },
            move |err| {
                let mut guard = callback_error
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if guard.is_none() {
                    *guard = Some(err.to_string());
                }
            },
            None,
        )
        .map_err(|err| format!("Failed to build microphone stream: {err}"))
}

fn build_input_stream_u16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Arc<Mutex<Vec<i16>>>,
    callback_error: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, String> {
    device
        .build_input_stream(
            config,
            move |data: &[u16], _| {
                let mut guard = samples.lock().unwrap_or_else(|err| err.into_inner());
                guard.extend(data.iter().map(|sample| (*sample as i32 - 32_768) as i16));
            },
            move |err| {
                let mut guard = callback_error
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if guard.is_none() {
                    *guard = Some(err.to_string());
                }
            },
            None,
        )
        .map_err(|err| format!("Failed to build microphone stream: {err}"))
}

fn build_input_stream_f32(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Arc<Mutex<Vec<i16>>>,
    callback_error: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, String> {
    device
        .build_input_stream(
            config,
            move |data: &[f32], _| {
                let mut guard = samples.lock().unwrap_or_else(|err| err.into_inner());
                guard.extend(data.iter().map(|sample| float_to_i16(*sample)));
            },
            move |err| {
                let mut guard = callback_error
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if guard.is_none() {
                    *guard = Some(err.to_string());
                }
            },
            None,
        )
        .map_err(|err| format!("Failed to build microphone stream: {err}"))
}

fn float_to_i16(sample: f32) -> i16 {
    let clamped = sample.clamp(-1.0, 1.0);
    (clamped * i16::MAX as f32).round() as i16
}

fn encode_wav(samples: &[i16], sample_rate_hz: u32, channels: u16) -> Result<Vec<u8>, String> {
    let mut cursor = Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels,
        sample_rate: sample_rate_hz,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|err| format!("Failed to initialize WAV writer: {err}"))?;
        for sample in samples {
            writer
                .write_sample(*sample)
                .map_err(|err| format!("Failed to write WAV sample: {err}"))?;
        }
        writer
            .finalize()
            .map_err(|err| format!("Failed to finalize WAV buffer: {err}"))?;
    }
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::{encode_wav, float_to_i16};

    #[test]
    fn float_samples_convert_to_pcm_i16() {
        assert_eq!(float_to_i16(-1.0), -32_767);
        assert_eq!(float_to_i16(0.0), 0);
        assert_eq!(float_to_i16(1.0), i16::MAX);
    }

    #[test]
    fn wav_encoder_produces_riff_header() {
        let wav = encode_wav(&[0, 1000, -1000], 16_000, 1).expect("wav should encode");
        assert!(wav.starts_with(b"RIFF"));
        assert!(wav.windows(4).any(|window| window == b"WAVE"));
    }
}
