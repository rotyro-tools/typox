//! Commands the two windows can call. Each one checks which window is calling.

use serde::Serialize;
use tauri::{AppHandle, Emitter, WebviewWindow};
use tauri_plugin_autostart::ManagerExt;

use crate::settings::Settings;
use crate::{ai, capture, secrets, widget};

/// Rejects the call if it did not originate from the expected window.
fn require_window(window: &WebviewWindow, label: &str) -> Result<(), String> {
    if window.label() != label {
        return Err("command not available from this window".to_string());
    }
    Ok(())
}

fn is_not_found_os_error(message: &str) -> bool {
    message.to_ascii_lowercase().contains("os error 2")
}

/// Reads an API key off the async runtime: the keychain may show a permission prompt.
async fn api_key(app: &AppHandle, provider: &str) -> Option<String> {
    let app = app.clone();
    let provider = provider.to_string();
    tauri::async_runtime::spawn_blocking(move || secrets::get(&app, &provider))
        .await
        .ok()
        .flatten()
}

// ── Settings window ──────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_settings(window: WebviewWindow, app: AppHandle) -> Result<Settings, String> {
    require_window(&window, "settings")?;
    Ok(crate::settings::load(&app))
}

#[tauri::command]
pub fn save_settings(
    window: WebviewWindow,
    app: AppHandle,
    settings: Settings,
) -> Result<(), String> {
    require_window(&window, "settings")?;
    let settings = settings.validated()?;
    // Before saving, so an unusable shortcut is reported and the working one kept.
    crate::shortcut::apply(&app, &settings.shortcut)?;
    crate::settings::save(&app, &settings)?;

    if settings.launch_at_login {
        app.autolaunch().enable().map_err(|e| e.to_string())?;
    } else if let Err(e) = app.autolaunch().disable() {
        let msg = e.to_string();
        if !is_not_found_os_error(&msg) {
            return Err(msg);
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    saved: bool,
    keychain_ok: bool,
}

/// Whether a key is saved for `provider`. The key itself never leaves the Rust side.
#[tauri::command]
pub async fn get_api_key_status(
    window: WebviewWindow,
    app: AppHandle,
    provider: String,
) -> Result<KeyStatus, String> {
    require_window(&window, "settings")?;
    secrets::validate_provider(&provider)?;
    let saved = api_key(&app, &provider).await.is_some();
    Ok(KeyStatus {
        saved,
        keychain_ok: secrets::keychain_ok(&app),
    })
}

#[tauri::command]
pub async fn save_api_key(
    window: WebviewWindow,
    app: AppHandle,
    provider: String,
    key: String,
) -> Result<(), String> {
    require_window(&window, "settings")?;
    tauri::async_runtime::spawn_blocking(move || secrets::save(&app, &provider, &key))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_api_key(
    window: WebviewWindow,
    app: AppHandle,
    provider: String,
) -> Result<(), String> {
    require_window(&window, "settings")?;
    tauri::async_runtime::spawn_blocking(move || secrets::delete(&app, &provider))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_ollama_models(window: WebviewWindow) -> Result<Vec<String>, String> {
    require_window(&window, "settings")?;
    ai::ollama::list_models().await
}

#[tauri::command]
pub async fn get_claude_models(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<Vec<String>, String> {
    require_window(&window, "settings")?;
    let key = api_key(&app, "claude").await.unwrap_or_default();
    Ok(ai::claude::list_models(&key).await)
}

#[tauri::command]
pub async fn get_openai_models(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<Vec<String>, String> {
    require_window(&window, "settings")?;
    let key = api_key(&app, "openai").await.unwrap_or_default();
    Ok(ai::openai::list_models(&key).await)
}

#[tauri::command]
pub fn get_default_prompt() -> String {
    ai::SYSTEM_PROMPT.to_string()
}

#[tauri::command]
pub fn get_app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

// ── Widget window ────────────────────────────────────────────────────────────

/// Pastes the correction into the source app. `override_text` is the user's edited version.
#[tauri::command]
pub async fn apply_correction(
    window: WebviewWindow,
    app: AppHandle,
    override_text: Option<String>,
) -> Result<(), String> {
    require_window(&window, widget::LABEL)?;
    capture::apply(&app, override_text).await
}

#[tauri::command]
pub async fn dismiss_correction(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    require_window(&window, widget::LABEL)?;
    capture::dismiss(&app).await;
    Ok(())
}

/// Asks the AI again; the outcome arrives as a correction-ready or correction-error event.
#[tauri::command]
pub async fn retry_correction(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    require_window(&window, widget::LABEL)?;
    capture::retry(&app).await
}

/// Resizes the widget to fit its content (in logical pixels) and keeps it centered.
#[tauri::command]
pub fn fit_widget(window: WebviewWindow, height: f64) -> Result<(), String> {
    require_window(&window, widget::LABEL)?;
    if !height.is_finite() {
        return Err("invalid height".to_string());
    }
    widget::fit(&window, height)
}

// ── Updates ──────────────────────────────────────────────────────────────────

/// Spawns a full update flow (check → download → install → restart) with live
/// status events emitted to the settings window for UI feedback.
#[tauri::command]
pub async fn check_update(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    require_window(&window, "settings")?;
    tauri::async_runtime::spawn(async move {
        run_update_install(app).await;
    });
    Ok(())
}

async fn run_update_install(app: AppHandle) {
    use tauri_plugin_notification::NotificationExt;
    use tauri_plugin_updater::UpdaterExt;

    let emit = |payload: serde_json::Value| {
        let _ = app.emit_to("settings", "update-status", payload);
    };

    emit(serde_json::json!({ "state": "checking" }));

    let update = match app.updater() {
        Ok(updater) => match updater.check().await {
            Ok(Some(update)) => update,
            Ok(None) => {
                emit(serde_json::json!({ "state": "up-to-date" }));
                return;
            }
            Err(e) => {
                emit(serde_json::json!({ "state": "error", "message": e.to_string() }));
                return;
            }
        },
        Err(e) => {
            emit(serde_json::json!({ "state": "error", "message": e.to_string() }));
            return;
        }
    };

    let version = update.version.clone();
    emit(serde_json::json!({ "state": "downloading", "version": version }));
    let _ = app
        .notification()
        .builder()
        .title("Typox update available")
        .body(format!("Downloading version {version}…"))
        .show();

    if let Err(e) = update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
    {
        emit(serde_json::json!({ "state": "error", "message": e.to_string() }));
        return;
    }

    emit(serde_json::json!({ "state": "installing" }));
    app.restart();
}

/// Silent startup update check — shows a system notification if an update is available.
pub async fn run_update_check(app: AppHandle) {
    use tauri_plugin_notification::NotificationExt;
    use tauri_plugin_updater::UpdaterExt;

    let Ok(updater) = app.updater() else {
        return;
    };
    if let Ok(Some(update)) = updater.check().await {
        let _ = app
            .notification()
            .builder()
            .title("Update available")
            .body(format!(
                "Typox v{} is available. Open Settings to install it.",
                update.version
            ))
            .show();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_os_error_detection_handles_windows_style_messages() {
        assert!(is_not_found_os_error(
            "The system cannot find the file specified. (os error 2)"
        ));
        assert!(!is_not_found_os_error("permission denied (os error 5)"));
    }
}
