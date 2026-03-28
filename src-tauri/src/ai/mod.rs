use async_trait::async_trait;

pub mod claude;
pub mod ollama;
pub mod openai;

pub const SYSTEM_PROMPT: &str = "Correct the following text. Apply only these fixes:\n\
     1. Spelling: fix typos, misspelled words, and elongated words where letters are repeated for emphasis (e.g. \"sooooo\" → \"so\", \"yesssss\" → \"yes\").\n\
     2. Capitalization: capitalize the start of sentences and proper nouns.\n\
     3. Verb conjugation: fix wrong verb forms (e.g. infinitive used instead of conjugated form).\n\
     4. Obvious contextual errors: if a correctly-spelled word is clearly the wrong word \
        given the context (e.g. \"how\" instead of \"like\" in \"how usual\"), fix it.\n\
     \n\
     Rules:\n\
     - Never remove words. If a word is unrecognizable, correct it to the closest real, existing word.\n\
     - Only replace a word with a word that actually exists in the target language. Never invent words.\n\
     - Do not rephrase, restructure, or change the meaning.\n\
     - Preserve the original tone, style, and punctuation unless punctuation is clearly wrong.\n\
     - Never translate. Respond in the exact same language as the input.\n\
     - Return the full corrected text only — no explanations, no comments.\n\
     \n\
     Examples:\n\
     \"she was verey happy about the news.\" → \"She was very happy about the news.\"\n\
     \"i absolutly love the way this wroks!\" → \"I absolutely love the way this works!\"\n\
     \"Ther are sevral problmes with this apporach.\" → \"There are several problems with this approach.\"\n\
     \"The meeting is on Monday at 3pm.\" → \"The meeting is on Monday at 3pm.\"";

/// Maximum response body size accepted from any AI provider or local service (4 MB).
const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

/// Reads the full response body, enforcing the size cap regardless of whether
/// Content-Length is present (chunked/streaming responses are also bounded).
pub async fn read_body_limited(resp: reqwest::Response) -> Result<Vec<u8>, String> {
    // Fast path: reject immediately if the declared Content-Length is already over the limit.
    if let Some(len) = resp.content_length() {
        if len > MAX_RESPONSE_BYTES {
            return Err(format!("Response too large ({len} bytes)"));
        }
    }
    let body = resp.bytes().await.map_err(|e| e.to_string())?;
    if body.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(format!("Response too large ({} bytes)", body.len()));
    }
    Ok(body.to_vec())
}

#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn correct_text(&self, text: &str) -> Result<String, String>;
}

/// Constructs the appropriate AI provider, reading API keys from the store.
pub fn make_provider(
    app: &tauri::AppHandle,
    settings: &crate::settings::Settings,
) -> Box<dyn AiProvider> {
    let prompt = settings
        .custom_prompt
        .as_deref()
        .unwrap_or(SYSTEM_PROMPT)
        .to_string();

    let read_key = |provider: &str| -> String { crate::commands::read_api_key(app, provider) };

    match settings.provider.as_str() {
        "claude" => Box::new(claude::ClaudeProvider::new(
            settings.model.clone(),
            read_key("claude"),
            prompt,
        )),
        "openai" => Box::new(openai::OpenAiProvider::new(
            settings.model.clone(),
            read_key("openai"),
            prompt,
        )),
        _ => Box::new(ollama::OllamaProvider::new(settings.model.clone(), prompt)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_is_non_empty() {
        assert!(!SYSTEM_PROMPT.is_empty());
    }

    #[test]
    fn system_prompt_instructs_no_translation() {
        assert!(SYSTEM_PROMPT.contains("Never translate"));
    }

    #[test]
    fn max_response_bytes_is_four_mebibytes() {
        assert_eq!(MAX_RESPONSE_BYTES, 4 * 1024 * 1024);
    }
}
