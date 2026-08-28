use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

enum AudioCommand {
    Start,
    Stop(Sender<Vec<f32>>),
}

pub struct AudioRecorder {
    sender: Sender<AudioCommand>,
}

impl AudioRecorder {
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = channel::<AudioCommand>();

        // Dedicated audio thread so cpal::Stream stays on one thread
        thread::spawn(move || {
            let mut stream: Option<Stream> = None;
            let recorded_samples = Arc::new(Mutex::new(Vec::<f32>::new()));

            while let Ok(cmd) = cmd_rx.recv() {
                match cmd {
                    AudioCommand::Start => {
                        recorded_samples.lock().unwrap().clear();
                        let host = cpal::default_host();
                        if let Some(device) = host.default_input_device() {
                            if let Ok(config) = device.default_input_config() {
                                let sample_format = config.sample_format();
                                let stream_config: cpal::StreamConfig = config.into();
                                let samples_buffer = recorded_samples.clone();
                                let err_fn = |err| eprintln!("Audio stream error: {}", err);

                                let new_stream = match sample_format {
                                    SampleFormat::F32 => device.build_input_stream(
                                        &stream_config,
                                        move |data: &[f32], _| {
                                            if let Ok(mut buffer) = samples_buffer.lock() {
                                                buffer.extend_from_slice(data);
                                            }
                                        },
                                        err_fn,
                                        None,
                                    ),
                                    SampleFormat::I16 => device.build_input_stream(
                                        &stream_config,
                                        move |data: &[i16], _| {
                                            if let Ok(mut buffer) = samples_buffer.lock() {
                                                for &sample in data {
                                                    buffer.push(sample as f32 / i16::MAX as f32);
                                                }
                                            }
                                        },
                                        err_fn,
                                        None,
                                    ),
                                    SampleFormat::U16 => device.build_input_stream(
                                        &stream_config,
                                        move |data: &[u16], _| {
                                            if let Ok(mut buffer) = samples_buffer.lock() {
                                                for &sample in data {
                                                    buffer.push((sample as f32 - i16::MAX as f32) / i16::MAX as f32);
                                                }
                                            }
                                        },
                                        err_fn,
                                        None,
                                    ),
                                    _ => Err(cpal::BuildStreamError::DeviceNotAvailable),
                                };

                                if let Ok(s) = new_stream {
                                    if s.play().is_ok() {
                                        stream = Some(s);
                                    }
                                }
                            }
                        }
                    }
                    AudioCommand::Stop(resp_tx) => {
                        stream = None;
                        let samples = recorded_samples.lock().unwrap().clone();
                        recorded_samples.lock().unwrap().clear();
                        let _ = resp_tx.send(samples);
                    }
                }
            }
        });

        Self { sender: cmd_tx }
    }

    pub fn start_recording(&self) -> Result<(), String> {
        self.sender
            .send(AudioCommand::Start)
            .map_err(|e| format!("Failed to send start signal: {}", e))
    }

    pub fn stop_recording(&self) -> Vec<f32> {
        let (resp_tx, resp_rx) = channel();
        if self.sender.send(AudioCommand::Stop(resp_tx)).is_ok() {
            resp_rx.recv().unwrap_or_default()
        } else {
            Vec::new()
        }
    }
}