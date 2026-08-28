use crate::tools::{open_application, read_system_info, run_terminal_command};
use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Cursor;

#[derive(Serialize)]
struct ContentPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(rename = "inlineData", skip_serializing_if = "Option::is_none")]
    inline_data: Option<InlineData>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    function_call: Option<FunctionCall>,
}

#[derive(Serialize)]
struct InlineData {
    #[serde(rename = "mimeType")]
    mime_type: String,
    data: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct FunctionCall {
    name: String,
    args: serde_json::Value,
}

#[derive(Serialize)]
struct Content {
    role: String,
    parts: Vec<ContentPart>,
}

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
}

#[derive(Deserialize)]
struct CandidatePart {
    text: Option<String>,
    #[serde(rename = "functionCall")]
    function_call: Option<FunctionCall>,
}

#[derive(Deserialize)]
struct CandidateContent {
    parts: Option<Vec<CandidatePart>>,
}

#[derive(Deserialize)]
struct Candidate {
    content: Option<CandidateContent>,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<Candidate>>,
}

pub struct FridayBrain {
    pub client: Client,
    pub api_key: String,
    pub eleven_key: Option<String>,
}

impl FridayBrain {
    pub fn new(api_key: String, eleven_key: Option<String>) -> Self {
        Self {
            client: Client::new(),
            api_key,
            eleven_key,
        }
    }

    fn get_tool_declarations() -> Vec<serde_json::Value> {
        vec![json!({
            "function_declarations": [
                {
                    "name": "run_terminal_command",
                    "description": "Execute a silent Windows PowerShell command in the background.",
                    "parameters": {
                        "type": "OBJECT",
                        "properties": {
                            "command": {
                                "type": "STRING",
                                "description": "The exact PowerShell command to run."
                            }
                        },
                        "required": ["command"]
                    }
                },
                {
                    "name": "open_application",
                    "description": "Launch an application, executable path, or website URL in the default browser.",
                    "parameters": {
                        "type": "OBJECT",
                        "properties": {
                            "target": {
                                "type": "STRING",
                                "description": "Executable name (e.g., 'spotify', 'notepad') or full URL."
                            }
                        },
                        "required": ["target"]
                    }
                },
                {
                    "name": "read_system_info",
                    "description": "Retrieve current CPU load, RAM utilization, and disk storage metrics.",
                    "parameters": {
                        "type": "OBJECT",
                        "properties": {}
                    }
                }
            ]
        })]
    }

    pub fn pcm_to_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
        let mut wav = Vec::new();
        let num_samples = samples.len() as u32;
        let byte_rate = sample_rate * 2;
        let block_align = 2u16;

        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + num_samples * 2).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM format
        wav.extend_from_slice(&1u16.to_le_bytes()); // Mono
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&byte_rate.to_le_bytes());
        wav.extend_from_slice(&block_align.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes()); // 16-bit
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(num_samples * 2).to_le_bytes());

        for &sample in samples {
            let clamped = sample.clamp(-1.0, 1.0);
            let sample_i16 = (clamped * i16::MAX as f32) as i16;
            wav.extend_from_slice(&sample_i16.to_le_bytes());
        }

        wav
    }

    pub async fn process_voice_input(&self, samples: Vec<f32>) -> Result<String, String> {
        if samples.is_empty() {
            return Ok(String::new());
        }

        let wav_data = Self::pcm_to_wav(&samples, 44100);
        let b64_audio = base64::engine::general_purpose::STANDARD.encode(wav_data);

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.7-flash:generateContent?key={}",
            self.api_key
        );

        let contents = vec![Content {
            role: "user".to_string(),
            parts: vec![
                ContentPart {
                    text: Some("You are F.R.I.D.A.Y., an intelligent desktop assistant. Listen to this user query and either respond directly or execute the requested tool command concisely.".to_string()),
                    inline_data: None,
                    function_call: None,
                },
                ContentPart {
                    text: None,
                    inline_data: Some(InlineData {
                        mime_type: "audio/wav".to_string(),
                        data: b64_audio,
                    }),
                    function_call: None,
                },
            ],
        }];

        let request_body = GeminiRequest {
            contents,
            tools: Some(Self::get_tool_declarations()),
        };

        let response = self
            .client
            .post(&url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("Network request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("Gemini HTTP error: {}", response.status()));
        }

        let parsed: GeminiResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Gemini response: {}", e))?;

        let candidate = parsed
            .candidates
            .and_then(|mut c| c.pop())
            .and_then(|c| c.content)
            .and_then(|c| c.parts)
            .and_then(|mut p| p.pop());

        if let Some(part) = candidate {
            if let Some(call) = part.function_call {
                let tool_output = match call.name.as_str() {
                    "run_terminal_command" => {
                        let cmd = call.args.get("command").and_then(|v| v.as_str()).unwrap_or("");
                        run_terminal_command(cmd)
                    }
                    "open_application" => {
                        let target = call.args.get("target").and_then(|v| v.as_str()).unwrap_or("");
                        open_application(target)
                    }
                    "read_system_info" => read_system_info(),
                    _ => format!("Unknown action: {}", call.name),
                };

                let report = format!("Task completed: {}", tool_output);
                self.speak_reply(&report).await;
                return Ok(report);
            }

            if let Some(text) = part.text {
                self.speak_reply(&text).await;
                return Ok(text);
            }
        }

        Ok("I could not process that request.".to_string())
    }

    pub async fn speak_reply(&self, text: &str) {
        if let Some(ref e_key) = self.eleven_key {
            if e_key.trim().is_empty() {
                return;
            }
            let voice_id = "21m00Tcm4TlvDq8ikWAM"; // Default Rachel Voice
            let url = format!("https://api.elevenlabs.io/v1/text-to-speech/{}/stream", voice_id);

            let body = json!({
                "text": text,
                "model_id": "eleven_monolingual_v1",
                "voice_settings": {
                    "stability": 0.5,
                    "similarity_boost": 0.75
                }
            });

            if let Ok(res) = self.client.post(&url)
                .header("xi-api-key", e_key)
                .json(&body)
                .send()
                .await
            {
                if let Ok(bytes) = res.bytes().await {
                    std::thread::spawn(move || {
                        if let Ok((_stream, stream_handle)) = rodio::OutputStream::try_default() {
                            if let Ok(sink) = rodio::Sink::try_new(&stream_handle) {
                                if let Ok(source) = rodio::Decoder::new(Cursor::new(bytes)) {
                                    sink.append(source);
                                    sink.sleep_until_end();
                                }
                            }
                        }
                    });
                }
            }
        }
    }
}