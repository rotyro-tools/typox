use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub provider: String,
    pub model: String,
    pub launch_at_login: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_prompt: Option<String>,
    #[serde(default = "default_shortcut")]
    pub shortcut: String,
}

fn default_shortcut() -> String {
    "Ctrl+Shift+Space".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: "ollama".to_string(),
            model: String::new(),
            launch_at_login: false,
            custom_prompt: None,
            shortcut: default_shortcut(),
        }
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
            model: "claude-haiku-4-5-20251001".to_string(),
            launch_at_login: true,
            custom_prompt: Some("Custom prompt".to_string()),
            shortcut: "Ctrl+Alt+A".to_string(),
        };
        let json = serde_json::to_value(&original).unwrap();
        let restored: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(restored.provider, original.provider);
        assert_eq!(restored.model, original.model);
        assert_eq!(restored.launch_at_login, original.launch_at_login);
        assert_eq!(restored.custom_prompt, original.custom_prompt);
        assert_eq!(restored.shortcut, original.shortcut);
    }

    #[test]
    fn custom_prompt_none_omitted_from_json() {
        let s = Settings::default();
        let json = serde_json::to_value(&s).unwrap();
        assert!(json.get("customPrompt").is_none());
    }
}
