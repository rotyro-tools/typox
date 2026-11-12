use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::AiProvider;

pub const BASE_URL: &str = "https://api.openai.com";
/// OpenAI's replacement for gpt-4o-mini and gpt-4.1-mini, which leave the API on 2027-01-14.
pub const DEFAULT_MODEL: &str = "gpt-6-luna";
/// Offered in Settings when the model list can't be fetched.
pub const DEFAULT_MODELS: [&str; 5] = [
    "gpt-6-luna",
    "gpt-6-sol",
    "gpt-5.6-luna",
    "gpt-5.6-terra",
    "gpt-5.6-sol",
];
const TIMEOUT: Duration = Duration::from_secs(60);
/// Extra output room for reasoning models, whose hidden reasoning counts toward the cap.
const REASONING_HEADROOM: u32 = 4_096;

pub struct OpenAiProvider {
    model: String,
    api_key: String,
    base_url: String,
}

impl OpenAiProvider {
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

/// GPT-4-generation models take `temperature`; the reasoning models since reject it.
fn takes_temperature(model: &str) -> bool {
    model.starts_with("gpt-4") || model.starts_with("gpt-3.5")
}

/// Whether a model id from `/v1/models` is a text chat model (not audio, images, embeddings…).
pub fn is_chat_model(id: &str) -> bool {
    const FAMILIES: [&str; 5] = ["gpt-", "chatgpt-", "o1", "o3", "o4"];
    const NOT_CHAT: [&str; 11] = [
        "audio",
        "realtime",
        "image",
        "transcribe",
        "tts",
        "search",
        "embedding",
        "moderation",
        "instruct",
        "codex",
        "live",
    ];
    FAMILIES.iter().any(|f| id.starts_with(f)) && !NOT_CHAT.iter().any(|w| id.contains(w))
}

#[derive(Deserialize)]
struct ChatMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    refusal: Option<String>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChatMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    async fn complete(&self, system: &str, user: &str, max_tokens: u32) -> Result<String, String> {
        let classic = takes_temperature(&self.model);
        let mut body = json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user",   "content": user }
            ],
            "max_completion_tokens": if classic { max_tokens } else { max_tokens + REASONING_HEADROOM },
        });
        if classic {
            body["temperature"] = json!(0);
        }

        let resp = super::remote_client()
            .post(format!("{}/v1/chat/completions", self.base_url))
            .timeout(TIMEOUT)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    "OpenAI took too long to answer. Try again.".to_string()
                } else {
                    format!("Couldn't reach OpenAI: {e}")
                }
            })?;

        let status = resp.status();
        let bytes = super::read_body_limited(resp).await?;
        if !status.is_success() {
            let detail = super::error_message(&bytes);
            return Err(match status.as_u16() {
                401 | 403 => "Invalid OpenAI API key. Check your key in Settings.".to_string(),
                404 => format!(
                    "OpenAI doesn't offer the model \"{}\". Pick another model in Settings.",
                    self.model
                ),
                // 429 covers both rate limits and an exhausted quota; OpenAI's text says which.
                429 => detail.unwrap_or_else(|| {
                    "OpenAI rate limit reached. Try again in a moment.".to_string()
                }),
                _ => match detail {
                    Some(detail) => format!("OpenAI returned {status}: {detail}"),
                    None => format!("OpenAI returned {status}"),
                },
            });
        }

        let parsed: ChatResponse = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Failed to parse OpenAI response: {e}"))?;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| "OpenAI's reply had no choices.".to_string())?;
        match choice.finish_reason.as_deref() {
            Some("length") => return Err(
                "The correction was cut off before the end of the text. Try a shorter selection."
                    .to_string(),
            ),
            Some("content_filter") => {
                return Err("OpenAI declined to correct this text.".to_string())
            }
            _ => {}
        }
        if choice.message.refusal.is_some() {
            return Err("OpenAI declined to correct this text.".to_string());
        }
        choice
            .message
            .content
            .filter(|c| !c.trim().is_empty())
            .ok_or_else(|| "OpenAI's reply had no text.".to_string())
    }
}

/// Chat model ids the key can use, or the built-in list if they can't be fetched.
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
        .get(format!("{}/v1/models", base_url.trim_end_matches('/')))
        .timeout(Duration::from_secs(10))
        .bearer_auth(api_key)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body = super::read_body_limited(resp).await.ok()?;
    let parsed: ModelsResponse = serde_json::from_slice(&body).ok()?;
    let mut models: Vec<String> = parsed
        .data
        .into_iter()
        .map(|m| m.id)
        .filter(|id| is_chat_model(id))
        .collect();
    models.sort();
    Some(models)
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn reply(message: serde_json::Value, finish_reason: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": message, "finish_reason": finish_reason }]
        }))
    }

    fn provider(server: &MockServer, model: &str) -> OpenAiProvider {
        OpenAiProvider::with_base_url(model.into(), "key".into(), &server.uri())
    }

    async fn sent_body(server: &MockServer) -> serde_json::Value {
        let requests = server.received_requests().await.unwrap();
        serde_json::from_slice(&requests[0].body).unwrap()
    }

    #[test]
    fn chat_model_filter_drops_non_chat_models() {
        for id in ["gpt-6-luna", "gpt-5.6-sol", "gpt-4o-mini", "o3", "o4-mini"] {
            assert!(is_chat_model(id), "{id}");
        }
        for id in [
            "gpt-image-2.6-flare",
            "gpt-realtime-2.1-mini",
            "gpt-audio-1.5",
            "gpt-4o-mini-transcribe",
            "gpt-live-transcribe",
            "gpt-4o-mini-tts",
            "text-embedding-3-small",
            "dall-e-3",
            "whisper-1",
        ] {
            assert!(!is_chat_model(id), "{id}");
        }
    }

    #[test]
    fn default_models_include_the_default_and_are_chat_models() {
        assert!(DEFAULT_MODELS.contains(&DEFAULT_MODEL));
        assert!(DEFAULT_MODELS.iter().all(|m| is_chat_model(m)));
    }

    #[tokio::test]
    async fn reasoning_models_get_no_temperature_and_more_room() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer key"))
            .and(body_partial_json(json!({
                "model": "gpt-6-luna",
                "messages": [
                    { "role": "system", "content": "sys" },
                    { "role": "user", "content": "user" }
                ],
                "max_completion_tokens": 2_000 + REASONING_HEADROOM
            })))
            .respond_with(reply(json!({ "content": "Fixed." }), "stop"))
            .expect(1)
            .mount(&server)
            .await;
        let text = provider(&server, "gpt-6-luna")
            .complete("sys", "user", 2_000)
            .await
            .unwrap();
        assert_eq!(text, "Fixed.");
        assert!(sent_body(&server).await.get("temperature").is_none());
    }

    #[tokio::test]
    async fn classic_models_get_temperature_zero() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_partial_json(
                json!({ "temperature": 0, "max_completion_tokens": 2_000 }),
            ))
            .respond_with(reply(json!({ "content": "Fixed." }), "stop"))
            .expect(1)
            .mount(&server)
            .await;
        provider(&server, "gpt-4o-mini")
            .complete("s", "u", 2_000)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn truncated_filtered_and_refused_replies_are_errors() {
        let cases = [
            (json!({ "content": "Half" }), "length", "cut off"),
            (json!({ "content": "" }), "content_filter", "declined"),
            (
                json!({ "content": null, "refusal": "no" }),
                "stop",
                "declined",
            ),
        ];
        for (message, finish_reason, expected) in cases {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(reply(message, finish_reason))
                .mount(&server)
                .await;
            let err = provider(&server, "gpt-6-luna")
                .complete("s", "u", 10)
                .await
                .unwrap_err();
            assert!(err.contains(expected), "{finish_reason}: {err}");
        }
    }

    #[tokio::test]
    async fn quota_errors_show_openais_message() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).set_body_json(json!({
                "error": { "message": "You exceeded your current quota.", "type": "insufficient_quota" }
            })))
            .mount(&server)
            .await;
        let err = provider(&server, "gpt-6-luna")
            .complete("s", "u", 10)
            .await
            .unwrap_err();
        assert_eq!(err, "You exceeded your current quota.");
    }

    #[tokio::test]
    async fn lists_only_chat_models_sorted() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{ "id": "gpt-6-sol" }, { "id": "gpt-image-2.6-flare" }, { "id": "gpt-6-luna" }]
            })))
            .mount(&server)
            .await;
        assert_eq!(
            list_models_at(&server.uri(), "key").await.unwrap(),
            vec!["gpt-6-luna", "gpt-6-sol"]
        );
    }
}
