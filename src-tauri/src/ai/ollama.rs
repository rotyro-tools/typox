use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::AiProvider;

pub struct OllamaProvider {
    model: String,
    system_prompt: String,
}

impl OllamaProvider {
    pub fn new(model: String, system_prompt: String) -> Self {
        Self {
            model: if model.is_empty() {
                "llama3.2".to_string()
            } else {
                model
            },
            system_prompt,
        }
    }
}

#[derive(Deserialize)]
struct OllamaMessage {
    content: String,
}

#[derive(Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
}

#[async_trait]
impl AiProvider for OllamaProvider {
    async fn correct_text(&self, text: &str) -> Result<String, String> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;

        let body = json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": self.system_prompt },
                { "role": "user",   "content": text }
            ],
            "stream": false,
            "options": {
                "temperature": 0.1,
                "top_k": 10,
                "top_p": 0.9,
                "repeat_penalty": 1.0
            }
        });

        let resp = client
            .post("http://localhost:11434/api/chat")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() {
                    "Ollama is not running — start it with `ollama serve`".to_string()
                } else {
                    format!("Ollama request failed: {e}")
                }
            })?;

        if !resp.status().is_success() {
            return Err(format!("Ollama returned {}", resp.status()));
        }

        let body = super::read_body_limited(resp).await?;
        let parsed: OllamaResponse = serde_json::from_slice(&body)
            .map_err(|e| format!("Failed to parse Ollama response: {e}"))?;

        Ok(parsed.message.content.trim().to_string())
    }
}
