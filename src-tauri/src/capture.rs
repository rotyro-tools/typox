//! The correction flow: copy the selection, show the widget, ask the AI, then paste the
//! correction back or dismiss it, and put the user's clipboard back either way.

use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::time::sleep;

use crate::state::{AppState, CorrectionSession};
use crate::{ai, platform, settings, widget};

/// How long the target app gets to read the pasted correction before the user's own
/// clipboard content is put back.
const CLIPBOARD_RESTORE_DELAY: Duration = Duration::from_millis(600);
/// How long to wait for the copied selection to reach the clipboard.
const CLIPBOARD_POLL: Duration = Duration::from_millis(50);
const CLIPBOARD_POLLS: usize = 12;

async fn clipboard_text() -> Option<String> {
    tokio::task::spawn_blocking(|| {
        arboard::Clipboard::new()
            .and_then(|mut c| c.get_text())
            .ok()
    })
    .await
    .ok()
    .flatten()
}

async fn set_clipboard_text(text: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        arboard::Clipboard::new()
            .and_then(|mut c| c.set_text(text))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn clear_clipboard() {
    let _ = tokio::task::spawn_blocking(|| {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.clear();
        }
    })
    .await;
}

/// Waits for the copied selection to show up on the (cleared) clipboard.
async fn read_copied_text() -> String {
    for _ in 0..CLIPBOARD_POLLS {
        if let Some(text) = clipboard_text().await.filter(|t| !t.is_empty()) {
            return text;
        }
        sleep(CLIPBOARD_POLL).await;
    }
    String::new()
}

fn take_session(app: &AppHandle) -> Option<CorrectionSession> {
    app.state::<AppState>().pending.lock().ok()?.take()
}

fn store_session(app: &AppHandle, session: CorrectionSession) {
    if let Ok(mut pending) = app.state::<AppState>().pending.lock() {
        *pending = Some(session);
    }
}

/// Puts the user's own clipboard content back now, if Typox still holds it.
async fn restore_saved_clipboard(app: &AppHandle) {
    let saved = app
        .state::<AppState>()
        .pending
        .lock()
        .ok()
        .and_then(|mut p| p.as_mut().and_then(|s| s.saved_clipboard.take()));
    if let Some(saved) = saved {
        let _ = set_clipboard_text(saved).await;
    }
}

/// Entry point of the global shortcut.
pub async fn capture_and_correct(app: AppHandle) {
    let state = app.state::<AppState>();
    let Some(_busy) = state.try_start() else {
        return;
    };
    let cursor = widget::cursor_position();
    // A result still open from earlier (the user clicked away from it): drop it.
    discard(&app).await;

    let id = state.next_session_id();
    let original = match capture(&app, id).await {
        Ok(text) => text,
        Err(message) => {
            show_error(&app, cursor, &message, false).await;
            return;
        }
    };

    if let Err(e) = widget::show_centered(&app, cursor) {
        eprintln!("[typox] failed to show the widget: {e}");
    }
    platform::focus_self().await;
    let _ = app.emit_to(widget::LABEL, "correction-started", ());
    run(&app, id, original).await;
}

/// Copies the selection and opens a session for it. Returns the selected text.
async fn capture(app: &AppHandle, id: u64) -> Result<String, String> {
    platform::wait_for_modifier_release().await;
    let saved_clipboard = clipboard_text().await;
    clear_clipboard().await;

    let copied = async {
        let source_app = platform::copy_selection().await?;
        let mut selected = read_copied_text().await;
        if selected.is_empty() && platform::copy_selection_fallback().await? {
            selected = read_copied_text().await;
        }
        Ok::<_, String>((source_app, selected))
    }
    .await;

    let (source_app, selected, error) = match copied {
        Ok((source, text)) if text.trim().is_empty() => (
            source,
            String::new(),
            Some("No text selected. Select some text, then press the shortcut again.".to_string()),
        ),
        Ok((source, text)) => (source, text, None),
        Err(e) => (String::new(), String::new(), Some(e)),
    };

    // Kept even on failure, so dismissing gives focus back to the app.
    store_session(
        app,
        CorrectionSession {
            id,
            original: selected.clone(),
            corrected: String::new(),
            source_app,
            saved_clipboard,
        },
    );
    match error {
        Some(message) => {
            restore_saved_clipboard(app).await;
            Err(message)
        }
        None => Ok(selected),
    }
}

/// Asks the AI to correct `original` for session `id` and sends the outcome to the widget.
/// The outcome is dropped if the session was dismissed, retried or replaced meanwhile.
async fn run(app: &AppHandle, id: u64, original: String) {
    let settings = settings::load(app);
    // Off the async runtime: reading the API key may show a keychain prompt.
    let provider = {
        let (app, settings) = (app.clone(), settings.clone());
        tauri::async_runtime::spawn_blocking(move || ai::make_provider(&app, &settings))
            .await
            .map_err(|e| e.to_string())
            .and_then(|provider| provider)
    };
    let result = match provider {
        Ok(provider) => {
            ai::correct(provider.as_ref(), ai::system_prompt(&settings), &original).await
        }
        Err(e) => Err(e),
    };

    {
        let state = app.state::<AppState>();
        let Ok(mut pending) = state.pending.lock() else {
            return;
        };
        match pending.as_mut() {
            Some(session) if session.id == id => {
                if let Ok(correction) = &result {
                    session.corrected = correction.text.clone();
                }
            }
            _ => return,
        }
    }

    let _ = match result {
        Ok(correction) => app.emit_to(
            widget::LABEL,
            "correction-ready",
            json!({
                "original": original,
                "noChanges": correction.text == original,
                "corrected": correction.text,
                "warning": correction.warning,
            }),
        ),
        Err(message) => app.emit_to(
            widget::LABEL,
            "correction-error",
            json!({ "message": message, "retryable": true }),
        ),
    };
}

async fn show_error(app: &AppHandle, cursor: Option<(f64, f64)>, message: &str, retryable: bool) {
    eprintln!("[typox] error: {message}");
    if let Err(e) = widget::show_centered(app, cursor) {
        eprintln!("[typox] failed to show the widget: {e}");
    }
    platform::focus_self().await;
    let _ = app.emit_to(
        widget::LABEL,
        "correction-error",
        json!({ "message": message, "retryable": retryable }),
    );
}

/// Closes a session nobody is looking at any more, giving back the user's clipboard.
async fn discard(app: &AppHandle) {
    if let Some(session) = take_session(app) {
        if let Some(saved) = session.saved_clipboard {
            let _ = set_clipboard_text(saved).await;
        }
    }
    widget::hide(app);
}

/// Pastes the correction (or the user's edit of it) into the source app.
pub async fn apply(app: &AppHandle, override_text: Option<String>) -> Result<(), String> {
    let session = take_session(app).ok_or_else(|| "No pending correction".to_string())?;
    let text = override_text
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| session.corrected.clone());
    if text.is_empty() {
        store_session(app, session);
        return Err("There's no correction to apply yet.".to_string());
    }
    if let Err(e) = set_clipboard_text(text.clone()).await {
        store_session(app, session);
        return Err(format!("Failed to write to the clipboard: {e}"));
    }

    widget::hide(app);
    sleep(Duration::from_millis(50)).await;

    if let Err(e) = platform::activate_and_paste(&session.source_app).await {
        // The widget is gone, so say it with a notification. The correction stays on the clipboard.
        let _ = app
            .notification()
            .builder()
            .title("Typox couldn't paste the correction")
            .body(format!("{e} The corrected text is on your clipboard."))
            .show();
        return Ok(());
    }

    if let Some(saved) = session.saved_clipboard {
        tauri::async_runtime::spawn(async move {
            sleep(CLIPBOARD_RESTORE_DELAY).await;
            // Only if nothing else was copied in the meantime.
            if clipboard_text().await.as_deref() == Some(text.as_str()) {
                let _ = set_clipboard_text(saved).await;
            }
        });
    }
    Ok(())
}

/// Closes the widget without applying anything and goes back to the source app.
pub async fn dismiss(app: &AppHandle) {
    let session = take_session(app);
    widget::hide(app);
    if let Some(session) = session {
        if let Some(saved) = session.saved_clipboard {
            let _ = set_clipboard_text(saved).await;
        }
        platform::activate(&session.source_app).await;
    }
}

/// Asks the AI again for the pending session's text.
pub async fn retry(app: &AppHandle) -> Result<(), String> {
    let (id, original) = {
        let state = app.state::<AppState>();
        let mut pending = state.pending.lock().map_err(|e| e.to_string())?;
        let session = pending
            .as_mut()
            .filter(|s| !s.original.trim().is_empty())
            .ok_or_else(|| {
                "Nothing to retry. Select text and press the shortcut again.".to_string()
            })?;
        // A new id, so a reply still on its way for the previous attempt is ignored.
        session.id = state.next_session_id();
        session.corrected.clear();
        (session.id, session.original.clone())
    };
    run(app, id, original).await;
    Ok(())
}
