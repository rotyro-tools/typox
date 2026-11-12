//! API keys, kept in the OS keychain. Earlier versions stored them in plain text in the
//! settings file; `migrate_legacy` moves them over at startup.

use std::sync::atomic::Ordering;

use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

use crate::state::AppState;

const KEYRING_SERVICE: &str = "com.rotyrotools.typox";
pub const PROVIDERS: [&str; 2] = ["claude", "openai"];

pub fn validate_provider(provider: &str) -> Result<(), String> {
    if PROVIDERS.contains(&provider) {
        Ok(())
    } else {
        Err(format!("unknown provider: {provider}"))
    }
}

fn keychain_entry(provider: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, &format!("{provider}-api-key"))
}

/// Settings-file key of a plain-text API key saved by an earlier version.
fn legacy_store_key(provider: &str) -> String {
    format!("api_key_{provider}")
}

fn legacy_key(app: &AppHandle, provider: &str) -> Option<String> {
    app.store(crate::settings::STORE_FILE)
        .ok()?
        .get(legacy_store_key(provider))
        .and_then(|v| v.as_str().map(str::to_string))
        .filter(|k| !k.is_empty())
}

fn cache(app: &AppHandle, provider: &str, value: Option<String>) {
    if let Ok(mut keys) = app.state::<AppState>().api_keys.lock() {
        keys.insert(provider.to_string(), value);
    }
}

/// The saved key for `provider`, reading the keychain only on first use.
pub fn get(app: &AppHandle, provider: &str) -> Option<String> {
    let state = app.state::<AppState>();
    if let Some(cached) = state.api_keys.lock().ok()?.get(provider) {
        return cached.clone();
    }
    let key = match keychain_entry(provider).and_then(|e| e.get_password()) {
        Ok(key) => Some(key),
        Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            eprintln!("[typox] keychain read failed: {e}");
            state.keychain_ok.store(false, Ordering::SeqCst);
            // Still usable if the key couldn't be moved out of the settings file yet.
            legacy_key(app, provider)
        }
    };
    cache(app, provider, key.clone());
    key
}

pub fn save(app: &AppHandle, provider: &str, key: &str) -> Result<(), String> {
    validate_provider(provider)?;
    let key = key.trim();
    if key.is_empty() {
        return Err("The API key is empty.".to_string());
    }
    let state = app.state::<AppState>();
    match keychain_entry(provider).and_then(|e| e.set_password(key)) {
        Ok(()) => {
            state.keychain_ok.store(true, Ordering::SeqCst);
            cache(app, provider, Some(key.to_string()));
            remove_legacy(app, provider);
            Ok(())
        }
        Err(e) => {
            // Never fall back to writing the key in plain text.
            state.keychain_ok.store(false, Ordering::SeqCst);
            Err(format!("Couldn't save the key to the keychain: {e}"))
        }
    }
}

pub fn delete(app: &AppHandle, provider: &str) -> Result<(), String> {
    validate_provider(provider)?;
    match keychain_entry(provider).and_then(|e| e.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(e) => return Err(format!("Couldn't remove the key from the keychain: {e}")),
    }
    cache(app, provider, None);
    remove_legacy(app, provider);
    Ok(())
}

pub fn keychain_ok(app: &AppHandle) -> bool {
    app.state::<AppState>().keychain_ok.load(Ordering::SeqCst)
}

fn remove_legacy(app: &AppHandle, provider: &str) {
    if let Ok(store) = app.store(crate::settings::STORE_FILE) {
        if store.delete(legacy_store_key(provider)) {
            if let Err(e) = store.save() {
                eprintln!("[typox] failed to save settings after removing a plain-text key: {e}");
            }
        }
    }
}

/// Moves plain-text keys saved by earlier versions into the keychain. A key stays in the
/// settings file only if the keychain refuses it, so nothing is lost.
pub fn migrate_legacy(app: &AppHandle) {
    for provider in PROVIDERS {
        if let Some(key) = legacy_key(app, provider) {
            if let Err(e) = save(app, provider, &key) {
                eprintln!("[typox] keeping the {provider} key in the settings file: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_providers_are_accepted() {
        assert!(validate_provider("claude").is_ok());
        assert!(validate_provider("openai").is_ok());
        assert!(validate_provider("ollama").is_err());
        assert!(validate_provider("../etc").is_err());
    }

    #[test]
    fn legacy_store_keys_match_earlier_versions() {
        assert_eq!(legacy_store_key("claude"), "api_key_claude");
        assert_eq!(legacy_store_key("openai"), "api_key_openai");
    }
}
