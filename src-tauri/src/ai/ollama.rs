use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::AiProvider;

pub const BASE_URL: &str = "http://127.0.0.1:11434";
/// How long Ollama keeps the model in memory after a correction. Short, because a 12B model
/// holds 8–12 GB; the next correction after a longer break waits a few seconds for it to load.
const KEEP_ALIVE: &str = "5m";
/// Context windows to pick from. Ollama reloads the model when the size changes, so they are
/// coarse; a prompt longer than the window silently loses its start, instructions included.
const CONTEXT_SIZES: [u32; 4] = [8_192, 16_384, 32_768, 65_536];
/// Loading a model from disk and correcting a long text can take a while on a laptop.
const TIMEOUT: Duration = Duration::from_secs(120);

pub struct OllamaProvider {
    model: String,
    base_url: String,
}

impl OllamaProvider {
    pub fn new(model: String) -> Self {
        Self::with_base_url(model, BASE_URL)
    }

    pub fn with_base_url(model: String, base_url: &str) -> Self {
        Self {
            model,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }
}

#[derive(Deserialize)]
struct ChatMessage {
    #[serde(default)]
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    message: ChatMessage,
    #[serde(default)]
    done_reason: Option<String>,
}

/// Smallest context window that fits the prompt and the longest allowed reply.
fn context_size(system: &str, user: &str, max_tokens: u32) -> u32 {
    // About 3 characters per token over-estimates most scripts, which is the safe side.
    let prompt_tokens = ((system.chars().count() + user.chars().count()) / 3) as u32;
    let needed = prompt_tokens + max_tokens;
    CONTEXT_SIZES
        .into_iter()
        .find(|&size| size >= needed)
        .unwrap_or(CONTEXT_SIZES[CONTEXT_SIZES.len() - 1])
}

fn request_error(e: &reqwest::Error) -> String {
    if e.is_connect() {
        "Ollama isn't running. Open the Ollama app (or run `ollama serve`) and try again."
            .to_string()
    } else if e.is_timeout() {
        "Ollama took too long to answer. A smaller model would be faster.".to_string()
    } else {
        format!("Ollama request failed: {e}")
    }
}

#[async_trait]
impl AiProvider for OllamaProvider {
    async fn complete(&self, system: &str, user: &str, max_tokens: u32) -> Result<String, String> {
        if self.model.is_empty() {
            return Err("Pick an Ollama model in Settings first.".to_string());
        }
        let body = json!({
            "model": self.model,
            "stream": false,
            // Thinking models (Gemma 4, Qwen 3.5…) would otherwise reason before every reply.
            "think": false,
            "keep_alive": KEEP_ALIVE,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user",   "content": user }
            ],
            "options": {
                // Corrections should be deterministic: the same text gets the same fix.
                "temperature": 0,
                "num_predict": max_tokens,
                "num_ctx": context_size(system, user, max_tokens)
            }
        });

        let resp = super::local_client()
            .post(format!("{}/api/chat", self.base_url))
            .timeout(TIMEOUT)
            .json(&body)
            .send()
            .await
            .map_err(|e| request_error(&e))?;

        let status = resp.status();
        let bytes = super::read_body_limited(resp).await?;
        if !status.is_success() {
            return Err(match status.as_u16() {
                404 => format!(
                    "The model \"{0}\" isn't installed. Run `ollama pull {0}` or pick another model in Settings.",
                    self.model
                ),
                _ => match super::error_message(&bytes) {
                    Some(detail) => format!("Ollama returned {status}: {detail}"),
                    None => format!("Ollama returned {status}"),
                },
            });
        }

        let parsed: ChatResponse = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Failed to parse Ollama response: {e}"))?;
        if parsed.done_reason.as_deref() == Some("length") {
            return Err(
                "The correction was cut off before the end of the text. Try a shorter selection."
                    .to_string(),
            );
        }
        Ok(parsed.message.content)
    }
}

/// Names of the models installed in Ollama.
pub async fn list_models() -> Result<Vec<String>, String> {
    list_models_at(BASE_URL).await
}

async fn list_models_at(base_url: &str) -> Result<Vec<String>, String> {
    #[derive(Deserialize)]
    struct Model {
        name: String,
    }
    #[derive(Deserialize)]
    struct TagsResponse {
        models: Vec<Model>,
    }

    let resp = super::local_client()
        .get(format!("{}/api/tags", base_url.trim_end_matches('/')))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|_| "Ollama isn't running".to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Ollama returned {}", resp.status()));
    }
    let body = super::read_body_limited(resp).await?;
    let parsed: TagsResponse = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    Ok(parsed.models.into_iter().map(|m| m.name).collect())
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn reply(content: &str, done_reason: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "message": { "role": "assistant", "content": content },
            "done": true,
            "done_reason": done_reason
        }))
    }

    #[test]
    fn context_size_fits_the_request_in_coarse_steps() {
        assert_eq!(context_size("system", "short text", 2_100), 8_192);
        let long = "x".repeat(30_000);
        assert_eq!(context_size("system", &long, 16_000), 32_768);
        let huge = "x".repeat(1_000_000);
        assert_eq!(context_size("system", &huge, 16_000), 65_536);
    }

    #[tokio::test]
    async fn sends_a_deterministic_non_thinking_request() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(json!({
                "model": "gemma4:12b-it-qat",
                "stream": false,
                "think": false,
                "keep_alive": "5m",
                "messages": [
                    { "role": "system", "content": "sys" },
                    { "role": "user", "content": "user" }
                ],
                "options": { "temperature": 0, "num_predict": 2_100, "num_ctx": 8_192 }
            })))
            .respond_with(reply("Fixed.", "stop"))
            .expect(1)
            .mount(&server)
            .await;

        let provider = OllamaProvider::with_base_url("gemma4:12b-it-qat".into(), &server.uri());
        assert_eq!(
            provider.complete("sys", "user", 2_100).await.unwrap(),
            "Fixed."
        );
    }

    #[tokio::test]
    async fn a_cut_off_reply_is_an_error_not_a_correction() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(reply("Half of the te", "length"))
            .mount(&server)
            .await;
        let provider = OllamaProvider::with_base_url("m".into(), &server.uri());
        let err = provider.complete("sys", "user", 10).await.unwrap_err();
        assert!(err.contains("cut off"), "{err}");
    }

    #[tokio::test]
    async fn a_missing_model_suggests_pulling_it() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(404).set_body_json(json!({ "error": "model 'm' not found" })),
            )
            .mount(&server)
            .await;
        let provider = OllamaProvider::with_base_url("m".into(), &server.uri());
        let err = provider.complete("sys", "user", 10).await.unwrap_err();
        assert!(err.contains("ollama pull m"), "{err}");
    }

    #[tokio::test]
    async fn other_errors_include_ollamas_message() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(500).set_body_json(json!({ "error": "out of memory" })),
            )
            .mount(&server)
            .await;
        let provider = OllamaProvider::with_base_url("m".into(), &server.uri());
        let err = provider.complete("sys", "user", 10).await.unwrap_err();
        assert!(err.ends_with("out of memory"), "{err}");
    }

    #[tokio::test]
    async fn no_model_selected_is_reported_without_a_request() {
        let provider = OllamaProvider::with_base_url(String::new(), "http://127.0.0.1:9");
        let err = provider.complete("sys", "user", 10).await.unwrap_err();
        assert!(err.contains("Pick an Ollama model"), "{err}");
    }

    #[tokio::test]
    async fn ollama_not_running_is_explained() {
        // Nothing listens on the discard port.
        let provider = OllamaProvider::with_base_url("m".into(), "http://127.0.0.1:9");
        let err = provider.complete("sys", "user", 10).await.unwrap_err();
        assert!(err.contains("isn't running"), "{err}");
    }

    #[tokio::test]
    async fn lists_installed_models() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "models": [{ "name": "gemma4:12b-it-qat" }, { "name": "qwen3.5:9b" }]
            })))
            .mount(&server)
            .await;
        assert_eq!(
            list_models_at(&server.uri()).await.unwrap(),
            vec!["gemma4:12b-it-qat", "qwen3.5:9b"]
        );
    }
}
