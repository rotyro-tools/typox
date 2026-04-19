use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::settings::Settings;
use crate::state::AppState;

/// Strips characters that could break out of an AppleScript/PowerShell string literal.
/// Keeps only characters that legitimately appear in application names.
fn sanitize_app_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_'))
        .collect()
}

/// Rejects the call if it did not originate from the expected window.
fn require_window(window: &tauri::WebviewWindow, label: &str) -> Result<(), String> {
    if window.label() != label {
        return Err("command not available from this window".to_string());
    }
    Ok(())
}

fn is_not_found_os_error(message: &str) -> bool {
    message.to_ascii_lowercase().contains("os error 2")
}

#[tauri::command]
pub fn get_settings(window: tauri::WebviewWindow, app: AppHandle) -> Result<Settings, String> {
    require_window(&window, "settings")?;
    Ok(crate::settings::load(&app))
}

#[tauri::command]
pub fn save_settings(
    window: tauri::WebviewWindow,
    app: AppHandle,
    settings: Settings,
) -> Result<(), String> {
    require_window(&window, "settings")?;
    crate::settings::save(&app, &settings)?;

    if settings.launch_at_login {
        app.autolaunch().enable().map_err(|e| e.to_string())?;
    } else {
        if let Err(e) = app.autolaunch().disable() {
            let msg = e.to_string();
            if !is_not_found_os_error(&msg) {
                return Err(msg);
            }
        }
    }

    Ok(())
}

pub(crate) fn read_api_key(app: &AppHandle, provider: &str) -> String {
    use tauri_plugin_store::StoreExt;
    app.store(crate::settings::STORE_FILE)
        .ok()
        .and_then(|store| store.get(format!("api_key_{provider}")))
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default()
}

/// Returns the saved API key for the given provider from the store.
#[tauri::command]
pub fn get_api_key(
    window: tauri::WebviewWindow,
    app: AppHandle,
    provider: String,
) -> Result<String, String> {
    require_window(&window, "settings")?;
    if provider != "claude" && provider != "openai" {
        return Err(format!("unknown provider: {provider}"));
    }
    Ok(read_api_key(&app, &provider))
}

/// Saves an API key for the given provider to the store.
#[tauri::command]
pub fn save_api_key(
    window: tauri::WebviewWindow,
    app: AppHandle,
    key: String,
    provider: String,
) -> Result<(), String> {
    require_window(&window, "settings")?;
    if provider != "claude" && provider != "openai" {
        return Err(format!("unknown provider: {provider}"));
    }
    use tauri_plugin_store::StoreExt;
    let store = app
        .store(crate::settings::STORE_FILE)
        .map_err(|e| e.to_string())?;
    store.set(
        format!("api_key_{provider}"),
        serde_json::Value::String(key),
    );
    store.save().map_err(|e| e.to_string())
}

/// Returns model names available from the local Ollama instance.
#[tauri::command]
pub async fn get_ollama_models(window: tauri::WebviewWindow) -> Result<Vec<String>, String> {
    require_window(&window, "settings")?;

    #[derive(Deserialize)]
    struct Model {
        name: String,
    }
    #[derive(Deserialize)]
    struct TagsResponse {
        models: Vec<Model>,
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get("http://localhost:11434/api/tags")
        .send()
        .await
        .map_err(|_| "Ollama is not running".to_string())?;

    if !resp.status().is_success() {
        return Err(format!("Ollama returned {}", resp.status()));
    }
    let body = crate::ai::read_body_limited(resp).await?;
    let parsed: TagsResponse = serde_json::from_slice(&body).map_err(|e| e.to_string())?;

    Ok(parsed.models.into_iter().map(|m| m.name).collect())
}

/// Writes the corrected text to clipboard, hides the widget, then pastes.
/// `override_text` allows the frontend to supply edited text instead of the stored correction.
#[tauri::command]
pub async fn apply_correction(
    window: tauri::WebviewWindow,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    override_text: Option<String>,
) -> Result<(), String> {
    require_window(&window, "widget")?;
    let session = {
        let mut pending = state.pending.lock().map_err(|e| e.to_string())?;
        pending
            .take()
            .ok_or_else(|| "No pending correction".to_string())?
    };
    let corrected = override_text.unwrap_or(session.corrected);
    let source_app = sanitize_app_name(&session.source_app);

    if let Some(win) = app.get_webview_window("widget") {
        let _ = win.hide();
        let _ = win.set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: 640.0,
            height: 160.0,
        }));
    }

    tokio::task::spawn_blocking(move || {
        use arboard::Clipboard;
        Clipboard::new()
            .and_then(|mut c| c.set_text(corrected))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Paste — activate source app first, then Cmd+V
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "tell application \"{source_app}\" to activate\n\
             tell application \"System Events\" to keystroke \"v\" using command down"
        );
        let _ = std::process::Command::new("osascript")
            .args(["-e", &script])
            .status();
    }

    #[cfg(target_os = "windows")]
    {
        if !source_app.is_empty() {
            let activate =
                format!("(New-Object -ComObject WScript.Shell).AppActivate('{source_app}')");
            let _ = std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &activate])
                .status();
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        tokio::task::spawn_blocking(|| {
            use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};
            let mut enigo = Enigo::new(&EnigoSettings::default()).map_err(|e| e.to_string())?;
            enigo
                .key(Key::Control, Direction::Press)
                .map_err(|e| e.to_string())?;
            enigo
                .key(Key::Unicode('v'), Direction::Click)
                .map_err(|e| e.to_string())?;
            enigo
                .key(Key::Control, Direction::Release)
                .map_err(|e| e.to_string())?;
            Ok::<(), String>(())
        })
        .await
        .map_err(|e| e.to_string())??;
    }

    #[cfg(target_os = "linux")]
    {
        if !source_app.is_empty() {
            let _ = std::process::Command::new("xdotool")
                .args(["windowactivate", "--sync", &source_app])
                .status();
        }
        tokio::task::spawn_blocking(|| {
            use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};
            let mut enigo = Enigo::new(&EnigoSettings::default()).map_err(|e| e.to_string())?;
            enigo
                .key(Key::Control, Direction::Press)
                .map_err(|e| e.to_string())?;
            enigo
                .key(Key::Unicode('v'), Direction::Click)
                .map_err(|e| e.to_string())?;
            enigo
                .key(Key::Control, Direction::Release)
                .map_err(|e| e.to_string())?;
            Ok::<(), String>(())
        })
        .await
        .map_err(|e| e.to_string())??;
    }

    Ok(())
}

/// Hides the widget and discards the pending correction without applying it.
#[tauri::command]
pub async fn dismiss_correction(
    window: tauri::WebviewWindow,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    require_window(&window, "widget")?;
    let source_app = {
        let mut pending = state.pending.lock().map_err(|e| e.to_string())?;
        pending
            .take()
            .map(|s| sanitize_app_name(&s.source_app))
            .unwrap_or_default()
    };

    if let Some(win) = app.get_webview_window("widget") {
        let _ = win.hide();
        let _ = win.set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: 640.0,
            height: 160.0,
        }));
    }

    #[cfg(target_os = "macos")]
    if !source_app.is_empty() {
        let script = format!("tell application \"{source_app}\" to activate");
        let _ = std::process::Command::new("osascript")
            .args(["-e", &script])
            .status();
    }

    #[cfg(target_os = "windows")]
    if !source_app.is_empty() {
        let script = format!("(New-Object -ComObject WScript.Shell).AppActivate('{source_app}')");
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .status();
    }

    #[cfg(target_os = "linux")]
    if !source_app.is_empty() {
        let _ = std::process::Command::new("xdotool")
            .args(["windowactivate", &source_app])
            .status();
    }

    Ok(())
}

/// Re-runs the AI correction on the original text and emits correction-ready.
#[tauri::command]
pub async fn retry_correction(
    window: tauri::WebviewWindow,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    require_window(&window, "widget")?;
    let original = {
        let pending = state.pending.lock().map_err(|e| e.to_string())?;
        pending
            .as_ref()
            .map(|s| s.original.clone())
            .ok_or_else(|| "No pending session".to_string())?
    };

    let settings = crate::settings::load(&app);
    let provider = crate::ai::make_provider(&app, &settings);
    let corrected = provider.correct_text(&original).await?.trim().to_string();
    let no_changes = corrected == original.trim();

    {
        let mut pending = state.pending.lock().map_err(|e| e.to_string())?;
        if let Some(session) = pending.as_mut() {
            session.corrected = corrected.clone();
        }
    }

    app.emit_to(
        "widget",
        "correction-ready",
        serde_json::json!({ "original": original, "corrected": corrected, "noChanges": no_changes }),
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    Ok(())
}

/// Returns the default system prompt string.
#[tauri::command]
pub fn get_default_prompt() -> String {
    crate::ai::SYSTEM_PROMPT.to_string()
}

/// Returns the app version from Cargo metadata.
#[tauri::command]
pub fn get_app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Returns available Claude models, falling back to a hardcoded list if the API key is absent.
#[tauri::command]
pub async fn get_claude_models(
    window: tauri::WebviewWindow,
    app: AppHandle,
) -> Result<Vec<String>, String> {
    require_window(&window, "settings")?;
    #[derive(Deserialize)]
    struct Model {
        id: String,
    }
    #[derive(Deserialize)]
    struct ModelsResponse {
        data: Vec<Model>,
    }

    let key = read_api_key(&app, "claude");

    if key.is_empty() {
        return Ok(default_claude_models());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .map_err(|_| "Failed to fetch Claude models".to_string())?;

    if !resp.status().is_success() {
        return Ok(default_claude_models());
    }

    let body = crate::ai::read_body_limited(resp).await?;
    let parsed: ModelsResponse = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let models: Vec<String> = parsed.data.into_iter().map(|m| m.id).collect();
    if models.is_empty() {
        Ok(default_claude_models())
    } else {
        Ok(models)
    }
}

fn default_claude_models() -> Vec<String> {
    vec![
        "claude-haiku-4-5-20251001".to_string(),
        "claude-sonnet-4-5".to_string(),
        "claude-opus-4-7".to_string(),
        "claude-sonnet-4-6".to_string(),
        "claude-opus-4-6".to_string(),
    ]
}

/// Returns available OpenAI models, falling back to a hardcoded list if the API key is absent.
#[tauri::command]
pub async fn get_openai_models(
    window: tauri::WebviewWindow,
    app: AppHandle,
) -> Result<Vec<String>, String> {
    require_window(&window, "settings")?;
    #[derive(Deserialize)]
    struct Model {
        id: String,
    }
    #[derive(Deserialize)]
    struct ModelsResponse {
        data: Vec<Model>,
    }

    let key = read_api_key(&app, "openai");

    if key.is_empty() {
        return Ok(default_openai_models());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get("https://api.openai.com/v1/models")
        .header("Authorization", format!("Bearer {key}"))
        .send()
        .await
        .map_err(|_| "Failed to fetch OpenAI models".to_string())?;

    if !resp.status().is_success() {
        return Ok(default_openai_models());
    }

    let body = crate::ai::read_body_limited(resp).await?;
    let parsed: ModelsResponse = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let mut models: Vec<String> = parsed
        .data
        .into_iter()
        .map(|m| m.id)
        .filter(|id| id.starts_with("gpt-") || id.starts_with("o1") || id.starts_with("o3"))
        .collect();
    models.sort();
    if models.is_empty() {
        Ok(default_openai_models())
    } else {
        Ok(models)
    }
}

fn default_openai_models() -> Vec<String> {
    vec![
        "gpt-3.5-turbo".to_string(),
        "gpt-4".to_string(),
        "gpt-4-turbo".to_string(),
        "gpt-4o".to_string(),
        "gpt-4o-mini".to_string(),
        "o1".to_string(),
        "o1-mini".to_string(),
        "o3-mini".to_string(),
    ]
}

/// Parses a shortcut string like "Ctrl+Shift+Space" into a Tauri Shortcut.
pub fn parse_shortcut(s: &str) -> Option<tauri_plugin_global_shortcut::Shortcut> {
    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

    let mut modifiers = Modifiers::empty();
    let mut key_code: Option<Code> = None;

    for part in s.split('+') {
        match part {
            "Ctrl" => modifiers |= Modifiers::CONTROL,
            "Shift" => modifiers |= Modifiers::SHIFT,
            "Alt" => modifiers |= Modifiers::ALT,
            "Meta" | "Super" | "Cmd" => modifiers |= Modifiers::META,
            key => {
                key_code = match key {
                    "Space" => Some(Code::Space),
                    "A" => Some(Code::KeyA),
                    "B" => Some(Code::KeyB),
                    "C" => Some(Code::KeyC),
                    "D" => Some(Code::KeyD),
                    "E" => Some(Code::KeyE),
                    "F" => Some(Code::KeyF),
                    "G" => Some(Code::KeyG),
                    "H" => Some(Code::KeyH),
                    "I" => Some(Code::KeyI),
                    "J" => Some(Code::KeyJ),
                    "K" => Some(Code::KeyK),
                    "L" => Some(Code::KeyL),
                    "M" => Some(Code::KeyM),
                    "N" => Some(Code::KeyN),
                    "O" => Some(Code::KeyO),
                    "P" => Some(Code::KeyP),
                    "Q" => Some(Code::KeyQ),
                    "R" => Some(Code::KeyR),
                    "S" => Some(Code::KeyS),
                    "T" => Some(Code::KeyT),
                    "U" => Some(Code::KeyU),
                    "V" => Some(Code::KeyV),
                    "W" => Some(Code::KeyW),
                    "X" => Some(Code::KeyX),
                    "Y" => Some(Code::KeyY),
                    "Z" => Some(Code::KeyZ),
                    "F1" => Some(Code::F1),
                    "F2" => Some(Code::F2),
                    "F3" => Some(Code::F3),
                    "F4" => Some(Code::F4),
                    "F5" => Some(Code::F5),
                    "F6" => Some(Code::F6),
                    "F7" => Some(Code::F7),
                    "F8" => Some(Code::F8),
                    "F9" => Some(Code::F9),
                    "F10" => Some(Code::F10),
                    "F11" => Some(Code::F11),
                    "F12" => Some(Code::F12),
                    _ => None,
                };
            }
        }
    }

    key_code.map(|code| {
        Shortcut::new(
            if modifiers.is_empty() {
                None
            } else {
                Some(modifiers)
            },
            code,
        )
    })
}

/// Re-registers the global shortcut with the new value and persists it to settings.
#[tauri::command]
pub fn update_shortcut(
    window: tauri::WebviewWindow,
    app: AppHandle,
    shortcut: String,
) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

    require_window(&window, "settings")?;

    let new_shortcut =
        parse_shortcut(&shortcut).ok_or_else(|| format!("Invalid shortcut: {shortcut}"))?;

    let mut settings = crate::settings::load(&app);
    let old_shortcut = parse_shortcut(&settings.shortcut);

    // Unregister previous shortcut
    if let Some(old) = old_shortcut {
        let _ = app.global_shortcut().unregister(old);
    }

    // Register new shortcut
    let handle = app.clone();
    app.global_shortcut()
        .on_shortcut(new_shortcut, move |app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                let busy = app
                    .get_webview_window("widget")
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false);
                if busy {
                    return;
                }
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    crate::clipboard::capture_and_correct(h).await;
                });
            }
        })
        .map_err(|e| e.to_string())?;

    settings.shortcut = shortcut;
    crate::settings::save(&app, &settings)
}

/// Spawns a full update flow (check → download → install → restart) with live
/// status events emitted to the settings window for UI feedback.
#[tauri::command]
pub async fn check_update(window: tauri::WebviewWindow, app: AppHandle) -> Result<(), String> {
    require_window(&window, "settings")?;
    tauri::async_runtime::spawn(async move {
        run_update_install(app).await;
    });
    Ok(())
}

/// Full update flow triggered from the settings window.
/// Emits "update-status" events to the settings window at each stage.
async fn run_update_install(app: AppHandle) {
    use tauri_plugin_notification::NotificationExt;
    use tauri_plugin_updater::UpdaterExt;

    macro_rules! emit {
        ($payload:expr) => {
            let _ = app.emit_to("settings", "update-status", $payload);
        };
    }

    emit!(serde_json::json!({ "state": "checking" }));

    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            emit!(serde_json::json!({ "state": "error", "message": e.to_string() }));
            return;
        }
    };

    let update = match updater.check().await {
        Ok(Some(u)) => u,
        Ok(None) => {
            emit!(serde_json::json!({ "state": "up-to-date" }));
            return;
        }
        Err(e) => {
            emit!(serde_json::json!({ "state": "error", "message": e.to_string() }));
            return;
        }
    };

    let version = update.version.clone();
    emit!(serde_json::json!({ "state": "downloading", "version": version }));
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
        emit!(serde_json::json!({ "state": "error", "message": e.to_string() }));
        return;
    }

    emit!(serde_json::json!({ "state": "installing" }));
    app.restart();
}

/// Silent startup update check — shows a system notification if an update is available.
pub async fn run_update_check(app: AppHandle) {
    use tauri_plugin_notification::NotificationExt;
    use tauri_plugin_updater::UpdaterExt;

    let updater = match app.updater() {
        Ok(u) => u,
        Err(_) => return,
    };

    if let Ok(Some(update)) = updater.check().await {
        let _ = app
            .notification()
            .builder()
            .title("Update available")
            .body(format!("v{} is available.", update.version))
            .show();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_app_name_allows_alphanumeric_and_safe_punctuation() {
        assert_eq!(sanitize_app_name("Safari"), "Safari");
        assert_eq!(
            sanitize_app_name("Visual Studio Code"),
            "Visual Studio Code"
        );
        assert_eq!(sanitize_app_name("my-app_v2.0"), "my-app_v2.0");
    }

    #[test]
    fn sanitize_app_name_strips_injection_chars() {
        assert_eq!(sanitize_app_name("app\"; rm -rf /"), "app rm -rf ");
        assert_eq!(sanitize_app_name("app' & evil"), "app  evil");
        assert_eq!(sanitize_app_name("app\nmalicious"), "appmalicious");
        assert_eq!(sanitize_app_name("test$(cmd)"), "testcmd");
    }

    #[test]
    fn sanitize_app_name_empty_returns_empty() {
        assert_eq!(sanitize_app_name(""), "");
    }

    #[test]
    fn default_claude_models_is_non_empty_and_all_claude_prefixed() {
        let models = default_claude_models();
        assert!(!models.is_empty());
        assert!(models.iter().all(|m| m.starts_with("claude-")));
    }

    #[test]
    fn default_openai_models_is_non_empty_and_all_known_prefixed() {
        let models = default_openai_models();
        assert!(!models.is_empty());
        assert!(models
            .iter()
            .all(|m| m.starts_with("gpt-") || m.starts_with("o1") || m.starts_with("o3")));
    }

    #[test]
    fn not_found_os_error_detection_handles_windows_style_messages() {
        assert!(is_not_found_os_error(
            "The system cannot find the file specified. (os error 2)"
        ));
        assert!(!is_not_found_os_error("permission denied (os error 5)"));
    }
}
