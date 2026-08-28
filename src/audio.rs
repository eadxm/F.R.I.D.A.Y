use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use std::sync::{Arc, Mutex};

pub struct AudioRecorder {
    stream: Option<Stream>,
    recorded_samples: Arc<Mutex<Vec<f32>>>,
}

impl AudioRecorder {
    pub fn new() -> Self {
        Self {
            stream: None,
            recorded_samples: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn start_recording(&mut self) -> Result<(), String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "No input device detected.".to_string())?;

        let config = device
            .default_input_config()
            .map_err(|e| format!("Failed to fetch default audio config: {}", e))?;

        let sample_format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.into();

        let samples_buffer = self.recorded_samples.clone();
        samples_buffer.lock().unwrap().clear();

        let err_fn = |err| eprintln!("Audio stream runtime error: {}", err);

        let stream = match sample_format {
            SampleFormat::F32 => device.build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mut buffer = samples_buffer.lock().unwrap();
                    buffer.extend_from_slice(data);
                },
                err_fn,
                None,
            ),
            SampleFormat::I16 => device.build_input_stream(
                &stream_config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let mut buffer = samples_buffer.lock().unwrap();
                    for &sample in data {
                        buffer.push(sample as f32 / i16::MAX as f32);
                    }
                },
                err_fn,
                None,
            ),
            SampleFormat::U16 => device.build_input_stream(
                &stream_config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    let mut buffer = samples_buffer.lock().unwrap();
                    for &sample in data {
                        buffer.push((sample as f32 - i16::MAX as f32) / i16::MAX as f32);
                    }
                },
                err_fn,
                None,
            ),
            _ => return Err("Unsupported audio sample format.".to_string()),
        }.map_err(|e| format!("Failed to build audio input stream: {}", e))?;

        stream.play().map_err(|e| format!("Failed to start audio stream: {}", e))?;
        self.stream = Some(stream);

        Ok(())
    }

    pub fn stop_recording(&mut self) -> Vec<f32> {
        self.stream = None;
        let mut buffer = self.recorded_samples.lock().unwrap();
        let captured = buffer.clone();
        buffer.clear();
        captured
    }
}