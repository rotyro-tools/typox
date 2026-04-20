use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::AiProvider;

pub struct ClaudeProvider {
    model: String,
    api_key: String,
    system_prompt: String,
}

impl ClaudeProvider {
    pub fn new(model: String, api_key: String, system_prompt: String) -> Self {
        Self {
            model: if model.is_empty() {
                "claude-haiku-4-5-20251001".to_string()
            } else {
                model
            },
            api_key,
            system_prompt,
        }
    }
}

#[derive(Deserialize)]
struct ClaudeContent {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}

#[derive(Deserialize)]
struct ClaudeResponse {
    content: Vec<ClaudeContent>,
}

#[async_trait]
impl AiProvider for ClaudeProvider {
    async fn correct_text(&self, text: &str) -> Result<String, String> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;

        let body = json!({
            "model": self.model,
            "max_tokens": 1024,
            "temperature": 0.1,
            "top_p": 0.9,
            "top_k": 10,
            "system": self.system_prompt,
            "messages": [
                { "role": "user", "content": text }
            ],
        });

        let resp = client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Claude request failed: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => "Invalid Claude API key — check your key in Settings.".to_string(),
                429 => "Claude rate limit exceeded — try again in a moment.".to_string(),
                _ => format!("Claude returned {status}"),
            });
        }

        let body = super::read_body_limited(resp).await?;
        let parsed: ClaudeResponse = serde_json::from_slice(&body)
            .map_err(|e| format!("Failed to parse Claude response: {e}"))?;

        parsed
            .content
            .into_iter()
            .find(|c| c.content_type == "text")
            .map(|c| c.text.trim().to_string())
            .ok_or_else(|| "No text content in Claude response".to_string())
    }
}
