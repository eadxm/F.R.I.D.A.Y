use cpal::traits::{DeviceTrait, HostTrait};

pub fn check_audio_hardware() -> Result<String, String> {
    let host = cpal::default_host();
    match host.default_input_device() {
        Some(device) => {
            let mic_name = device.name().unwrap_or_else(|_| "Unknown Microphone".to_string());
            Ok(format!("Hardware Linked: {}", mic_name))
        }
        None => Err("CRITICAL: No microphone hardware detected.".to_string()),
    }
}