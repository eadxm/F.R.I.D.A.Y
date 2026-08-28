use crate::tools::{open_application, read_system_info, run_terminal_command};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize)]
struct ContentPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    function_call: Option<FunctionCall>,
    #[serde(rename = "functionResponse", skip_serializing_if = "Option::is_none")]
    function_response: Option<FunctionResponse>,
}

#[derive(Serialize, Deserialize, Clone)]
struct FunctionCall {
    name: String,
    args: serde_json::Value,
}

#[derive(Serialize)]
struct FunctionResponse {
    name: String,
    response: serde_json::Value,
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
}

impl FridayBrain {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
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
                                "description": "Executable name (e.g., 'spotify', 'notepad') or full URL (e.g., 'https://youtube.com')."
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

    pub async fn ask_gemini(&self, prompt: &str) -> Result<String, String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.7-flash:generateContent?key={}",
            self.api_key
        );

        let mut contents = vec![Content {
            role: "user".to_string(),
            parts: vec![ContentPart {
                text: Some(prompt.to_string()),
                function_call: None,
                function_response: None,
            }],
        }];

        let request_body = GeminiRequest {
            contents: contents.drain(..).collect(),
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
            return Err(format!("Gemini returned HTTP error: {}", response.status()));
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
                    _ => format!("Unknown tool call: {}", call.name),
                };

                return Ok(format!("[Action Executed: {}]\n{}", call.name, tool_output));
            }

            if let Some(text) = part.text {
                return Ok(text);
            }
        }

        Ok("No readable response or tool call received.".to_string())
    }
}