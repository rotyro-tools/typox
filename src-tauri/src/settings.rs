use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

pub const PROVIDERS: [&str; 3] = ["ollama", "claude", "openai"];

/// Every field has a default, so a settings file missing one (written by an older version,
/// say) still loads instead of resetting everything.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub launch_at_login: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_prompt: Option<String>,
    #[serde(default = "default_shortcut")]
    pub shortcut: String,
}

fn default_provider() -> String {
    "ollama".to_string()
}

fn default_shortcut() -> String {
    crate::shortcut::DEFAULT.to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: String::new(),
            launch_at_login: false,
            custom_prompt: None,
            shortcut: default_shortcut(),
        }
    }
}

impl Settings {
    /// Checks settings sent by the Settings window and tidies them up: an empty or default
    /// custom prompt is dropped, so the built-in prompt (and its future improvements) applies.
    pub fn validated(mut self) -> Result<Self, String> {
        if !PROVIDERS.contains(&self.provider.as_str()) {
            return Err(format!("unknown provider: {}", self.provider));
        }
        self.model = self.model.trim().to_string();
        self.shortcut = self.shortcut.trim().to_string();
        self.custom_prompt = self
            .custom_prompt
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty() && p != crate::ai::SYSTEM_PROMPT);
        if let Some(prompt) = &self.custom_prompt {
            if prompt.chars().count() > crate::ai::MAX_PROMPT_CHARS {
                return Err(format!(
                    "The system prompt is too long (more than {} characters).",
                    crate::ai::MAX_PROMPT_CHARS
                ));
            }
        }
        Ok(self)
    }
}

pub const STORE_FILE: &str = "typox.json";
const SETTINGS_KEY: &str = "settings";

pub fn load(app: &AppHandle) -> Settings {
    app.store(STORE_FILE)
        .ok()
        .and_then(|store| {
            store
                .get(SETTINGS_KEY)
                .and_then(|v| serde_json::from_value(v).ok())
        })
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = std::fs::create_dir_all(dir);
    }
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(
        SETTINGS_KEY,
        serde_json::to_value(settings).map_err(|e| e.to_string())?,
    );
    store.save().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_ollama() {
        let s = Settings::default();
        assert_eq!(s.provider, "ollama");
        assert!(s.model.is_empty());
        assert!(!s.launch_at_login);
        assert_eq!(s.shortcut, "Ctrl+Shift+Space");
    }

    #[test]
    fn settings_serde_round_trip() {
        let original = Settings {
            provider: "claude".to_string(),
            model: "claude-haiku-5-5".to_string(),
            launch_at_login: true,
            custom_prompt: Some("Custom prompt".to_string()),
            shortcut: "Ctrl+Alt+A".to_string(),
        };
        let json = serde_json::to_value(&original).unwrap();
        assert_eq!(json["launchAtLogin"], true);
        let restored: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(restored.provider, original.provider);
        assert_eq!(restored.model, original.model);
        assert_eq!(restored.custom_prompt, original.custom_prompt);
        assert_eq!(restored.shortcut, original.shortcut);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let restored: Settings =
            serde_json::from_value(serde_json::json!({ "shortcut": "Ctrl+Alt+K" })).unwrap();
        assert_eq!(restored.provider, "ollama");
        assert_eq!(
            restored.shortcut, "Ctrl+Alt+K",
            "the saved shortcut survives"
        );
    }

    #[test]
    fn custom_prompt_none_omitted_from_json() {
        let json = serde_json::to_value(Settings::default()).unwrap();
        assert!(json.get("customPrompt").is_none());
    }

    #[test]
    fn validated_drops_empty_and_default_prompts() {
        let mut s = Settings {
            custom_prompt: Some("   ".to_string()),
            ..Settings::default()
        };
        assert_eq!(s.clone().validated().unwrap().custom_prompt, None);
        s.custom_prompt = Some(crate::ai::SYSTEM_PROMPT.to_string());
        assert_eq!(s.clone().validated().unwrap().custom_prompt, None);
        s.custom_prompt = Some("  Fix typos only. ".to_string());
        assert_eq!(
            s.validated().unwrap().custom_prompt.as_deref(),
            Some("Fix typos only.")
        );
    }

    #[test]
    fn validated_rejects_unknown_providers_and_huge_prompts() {
        let s = Settings {
            provider: "evil".to_string(),
            ..Settings::default()
        };
        assert!(s.validated().is_err());
        let s = Settings {
            custom_prompt: Some("x".repeat(crate::ai::MAX_PROMPT_CHARS + 1)),
            ..Settings::default()
        };
        assert!(s.validated().is_err());
    }
}
