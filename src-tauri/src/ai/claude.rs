use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::AiProvider;

pub const BASE_URL: &str = "https://api.anthropic.com";
/// Fast and inexpensive, which suits short proofreading requests.
pub const DEFAULT_MODEL: &str = "claude-haiku-5-5";
/// Offered in Settings when the model list can't be fetched.
pub const DEFAULT_MODELS: [&str; 4] = [
    "claude-haiku-5-5",
    "claude-sonnet-5-5",
    "claude-opus-5-5",
    "claude-haiku-4-5",
];
const API_VERSION: &str = "2023-06-01";
const TIMEOUT: Duration = Duration::from_secs(60);

pub struct ClaudeProvider {
    model: String,
    api_key: String,
    base_url: String,
}

impl ClaudeProvider {
    pub fn new(model: String, api_key: String) -> Self {
        Self::with_base_url(model, api_key, BASE_URL)
    }

    pub fn with_base_url(model: String, api_key: String, base_url: &str) -> Self {
        Self {
            model: if model.is_empty() {
                DEFAULT_MODEL.to_string()
            } else {
                model
            },
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }
}

/// Whether the model accepts `output_config.effort` (Claude 4.6 and later); older ones reject it.
pub fn supports_effort(model: &str) -> bool {
    let Some(rest) = model.strip_prefix("claude-") else {
        return false;
    };
    let mut parts = rest.split('-');
    // claude-3-5-sonnet-…: the older naming starts with the version.
    if parts
        .next()
        .is_none_or(|family| family.chars().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    let Some(major) = parts.next().and_then(|p| p.parse::<u32>().ok()) else {
        return false;
    };
    // A short second number is the minor version ("4-6"); a long one is a date ("4-20250514").
    let minor = parts
        .next()
        .filter(|p| p.len() <= 2)
        .and_then(|p| p.parse::<u32>().ok())
        .unwrap_or(0);
    major > 4 || (major == 4 && minor >= 6)
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct MessageResponse {
    content: Vec<ContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[async_trait]
impl AiProvider for ClaudeProvider {
    async fn complete(&self, system: &str, user: &str, max_tokens: u32) -> Result<String, String> {
        // No sampling parameters: current models reject them, and proofreading doesn't need them.
        let mut body = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "system": system,
            "messages": [{ "role": "user", "content": user }],
        });
        if supports_effort(&self.model) {
            // Proofreading needs little reasoning; low effort keeps it fast.
            body["output_config"] = json!({ "effort": "low" });
        }

        let resp = super::remote_client()
            .post(format!("{}/v1/messages", self.base_url))
            .timeout(TIMEOUT)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    "Claude took too long to answer. Try again.".to_string()
                } else {
                    format!("Couldn't reach Claude: {e}")
                }
            })?;

        let status = resp.status();
        let bytes = super::read_body_limited(resp).await?;
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => "Invalid Claude API key. Check your key in Settings.".to_string(),
                404 => format!(
                    "Claude doesn't offer the model \"{}\". Pick another model in Settings.",
                    self.model
                ),
                429 => "Claude rate limit reached. Try again in a moment.".to_string(),
                529 => "Claude is overloaded right now. Try again in a moment.".to_string(),
                _ => match super::error_message(&bytes) {
                    Some(detail) => format!("Claude returned {status}: {detail}"),
                    None => format!("Claude returned {status}"),
                },
            });
        }

        let parsed: MessageResponse = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Failed to parse Claude response: {e}"))?;
        match parsed.stop_reason.as_deref() {
            Some("max_tokens") => return Err(
                "The correction was cut off before the end of the text. Try a shorter selection."
                    .to_string(),
            ),
            Some("refusal") => return Err("Claude declined to correct this text.".to_string()),
            _ => {}
        }
        // Read blocks by type: a reply can start with thinking blocks.
        let text: String = parsed
            .content
            .into_iter()
            .filter(|block| block.kind == "text")
            .map(|block| block.text)
            .collect();
        if text.trim().is_empty() {
            return Err("Claude's reply had no text.".to_string());
        }
        Ok(text)
    }
}

/// Model ids the key can use, or the built-in list if they can't be fetched.
pub async fn list_models(api_key: &str) -> Vec<String> {
    list_models_at(BASE_URL, api_key)
        .await
        .filter(|models| !models.is_empty())
        .unwrap_or_else(|| DEFAULT_MODELS.iter().map(|m| m.to_string()).collect())
}

async fn list_models_at(base_url: &str, api_key: &str) -> Option<Vec<String>> {
    #[derive(Deserialize)]
    struct Model {
        id: String,
    }
    #[derive(Deserialize)]
    struct ModelsResponse {
        data: Vec<Model>,
    }

    if api_key.is_empty() {
        return None;
    }
    let resp = super::remote_client()
        .get(format!(
            "{}/v1/models?limit=100",
            base_url.trim_end_matches('/')
        ))
        .timeout(Duration::from_secs(10))
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body = super::read_body_limited(resp).await.ok()?;
    let parsed: ModelsResponse = serde_json::from_slice(&body).ok()?;
    Some(
        parsed
            .data
            .into_iter()
            .map(|m| m.id)
            .filter(|id| id.starts_with("claude-"))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    use super::*;

    fn reply(content: serde_json::Value, stop_reason: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "content": content,
            "stop_reason": stop_reason
        }))
    }

    fn provider(server: &MockServer, model: &str) -> ClaudeProvider {
        ClaudeProvider::with_base_url(model.into(), "key".into(), &server.uri())
    }

    #[test]
    fn effort_is_sent_only_to_models_that_accept_it() {
        for model in [
            "claude-haiku-5-5",
            "claude-sonnet-5-5",
            "claude-opus-5",
            "claude-sonnet-4-6",
            "claude-fable-5-2",
        ] {
            assert!(supports_effort(model), "{model}");
        }
        for model in [
            "claude-haiku-4-5",
            "claude-haiku-4-5-20251001",
            "claude-sonnet-4-20250514",
            "claude-opus-4-1",
            "claude-3-5-sonnet-20241022",
            "gpt-4o",
            "claude-",
        ] {
            assert!(!supports_effort(model), "{model}");
        }
    }

    #[test]
    fn an_empty_model_falls_back_to_the_default() {
        let p = ClaudeProvider::new(String::new(), "key".into());
        assert_eq!(p.model, DEFAULT_MODEL);
    }

    #[tokio::test]
    async fn sends_no_sampling_parameters_and_low_effort() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "key"))
            .and(header("anthropic-version", API_VERSION))
            .and(body_partial_json(json!({
                "model": "claude-haiku-5-5",
                "max_tokens": 3_000,
                "system": "sys",
                "messages": [{ "role": "user", "content": "user" }],
                "output_config": { "effort": "low" }
            })))
            .respond_with(reply(
                json!([{ "type": "text", "text": "Fixed." }]),
                "end_turn",
            ))
            .expect(1)
            .mount(&server)
            .await;

        let text = provider(&server, "claude-haiku-5-5")
            .complete("sys", "user", 3_000)
            .await
            .unwrap();
        assert_eq!(text, "Fixed.");

        let requests: Vec<Request> = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        for param in ["temperature", "top_p", "top_k"] {
            assert!(body.get(param).is_none(), "{param} must not be sent");
        }
    }

    #[tokio::test]
    async fn older_models_get_no_effort() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(reply(
                json!([{ "type": "text", "text": "Fixed." }]),
                "end_turn",
            ))
            .mount(&server)
            .await;
        provider(&server, "claude-haiku-4-5")
            .complete("s", "u", 10)
            .await
            .unwrap();
        let requests = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert!(body.get("output_config").is_none());
    }

    #[tokio::test]
    async fn reads_text_blocks_after_thinking_blocks() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(reply(
                json!([{ "type": "thinking", "thinking": "" }, { "type": "text", "text": "Fixed." }]),
                "end_turn",
            ))
            .mount(&server)
            .await;
        let text = provider(&server, "claude-haiku-5-5")
            .complete("s", "u", 10)
            .await
            .unwrap();
        assert_eq!(text, "Fixed.");
    }

    #[tokio::test]
    async fn truncated_and_refused_replies_are_errors() {
        for (stop_reason, expected) in [("max_tokens", "cut off"), ("refusal", "declined")] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(reply(
                    json!([{ "type": "text", "text": "Half" }]),
                    stop_reason,
                ))
                .mount(&server)
                .await;
            let err = provider(&server, "claude-haiku-5-5")
                .complete("s", "u", 10)
                .await
                .unwrap_err();
            assert!(err.contains(expected), "{stop_reason}: {err}");
        }
    }

    #[tokio::test]
    async fn http_errors_are_explained() {
        for (status, expected) in [
            (401, "Invalid Claude API key"),
            (404, "Pick another model"),
            (529, "overloaded"),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(status))
                .mount(&server)
                .await;
            let err = provider(&server, "claude-haiku-5-5")
                .complete("s", "u", 10)
                .await
                .unwrap_err();
            assert!(err.contains(expected), "{status}: {err}");
        }
    }

    #[tokio::test]
    async fn lists_claude_models() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{ "id": "claude-haiku-5-5" }, { "id": "claude-opus-5-5" }]
            })))
            .mount(&server)
            .await;
        assert_eq!(
            list_models_at(&server.uri(), "key").await.unwrap(),
            vec!["claude-haiku-5-5", "claude-opus-5-5"]
        );
        assert!(list_models_at(&server.uri(), "").await.is_none());
    }

    #[test]
    fn default_models_include_the_default() {
        assert!(DEFAULT_MODELS.contains(&DEFAULT_MODEL));
    }
}
