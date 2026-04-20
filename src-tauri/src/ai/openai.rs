use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::AiProvider;

pub struct OpenAiProvider {
    model: String,
    api_key: String,
    system_prompt: String,
}

impl OpenAiProvider {
    pub fn new(model: String, api_key: String, system_prompt: String) -> Self {
        Self {
            model: if model.is_empty() {
                "gpt-4o-mini".to_string()
            } else {
                model
            },
            api_key,
            system_prompt,
        }
    }
}

#[derive(Deserialize)]
struct OpenAiMessage {
    content: String,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    async fn correct_text(&self, text: &str) -> Result<String, String> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;

        let body = json!({
            "model": self.model,
            "temperature": 0.1,
            "top_p": 0.9,
            "messages": [
                { "role": "system", "content": self.system_prompt },
                { "role": "user",   "content": text }
            ],
        });

        let resp = client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("OpenAI request failed: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => "Invalid OpenAI API key — check your key in Settings.".to_string(),
                429 => "OpenAI rate limit exceeded — try again in a moment.".to_string(),
                _ => format!("OpenAI returned {status}"),
            });
        }

        let body = super::read_body_limited(resp).await?;
        let parsed: OpenAiResponse = serde_json::from_slice(&body)
            .map_err(|e| format!("Failed to parse OpenAI response: {e}"))?;

        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content.trim().to_string())
            .ok_or_else(|| "No choices in OpenAI response".to_string())
    }
}
