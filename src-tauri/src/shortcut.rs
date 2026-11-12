//! The global shortcut that starts a correction.

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::state::AppState;

pub const DEFAULT: &str = "Ctrl+Shift+Space";

/// Parses a shortcut as the Settings recorder writes it ("Ctrl+Shift+Space", "Meta+Alt+K",
/// "Ctrl+Shift+1"…). At least one modifier is required, so typing is never swallowed.
pub fn parse(value: &str) -> Option<Shortcut> {
    let tokens: Vec<&str> = value
        .split('+')
        .map(|token| {
            let token = token.trim();
            // The recorder says Meta; the parser calls that key Super.
            if token.eq_ignore_ascii_case("meta") {
                "Super"
            } else {
                token
            }
        })
        .collect();
    if tokens.len() < 2 || tokens.iter().any(|t| t.is_empty()) {
        return None;
    }
    tokens.join("+").parse::<Shortcut>().ok()
}

fn is_focused(app: &AppHandle, label: &str) -> bool {
    app.get_webview_window(label)
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false)
}

fn on_pressed(app: &AppHandle) {
    if is_focused(app, "settings") {
        // The OS swallows the current shortcut's keys, so the Settings recorder hears about
        // them this way.
        let current = crate::settings::load(app).shortcut;
        let _ = app.emit_to("settings", "shortcut-triggered", current);
        return;
    }
    if is_focused(app, crate::widget::LABEL) {
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        crate::capture::capture_and_correct(handle).await;
    });
}

/// Makes `value` the global shortcut. The current one stays active if `value` can't be used.
pub fn apply(app: &AppHandle, value: &str) -> Result<(), String> {
    let new = parse(value).ok_or_else(|| format!("\"{value}\" isn't a valid shortcut."))?;
    let state = app.state::<AppState>();
    let mut current = state.shortcut.lock().map_err(|e| e.to_string())?;
    if current.as_ref() == Some(&new) {
        return Ok(());
    }
    app.global_shortcut()
        .on_shortcut(new, |app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                on_pressed(app);
            }
        })
        .map_err(|e| format!("Couldn't use {value} ({e}). Another app may already use it."))?;
    if let Some(old) = current.take() {
        if let Err(e) = app.global_shortcut().unregister(old) {
            eprintln!("[typox] failed to unregister the previous shortcut: {e}");
        }
    }
    *current = Some(new);
    Ok(())
}

/// Registers the saved shortcut at startup, falling back to the default one.
pub fn register_saved(app: &AppHandle) -> Result<(), String> {
    let saved = crate::settings::load(app).shortcut;
    apply(app, &saved).or_else(|e| {
        eprintln!("[typox] saved shortcut unusable, using the default: {e}");
        apply(app, DEFAULT)
    })
}

#[cfg(test)]
mod tests {
    use tauri_plugin_global_shortcut::{Code, Modifiers};

    use super::*;

    #[test]
    fn parses_the_default() {
        assert_eq!(
            parse(DEFAULT),
            Some(Shortcut::new(
                Some(Modifiers::CONTROL | Modifiers::SHIFT),
                Code::Space
            ))
        );
    }

    #[test]
    fn parses_what_the_recorder_writes() {
        assert_eq!(
            parse("Meta+Alt+K"),
            Some(Shortcut::new(
                Some(Modifiers::SUPER | Modifiers::ALT),
                Code::KeyK
            ))
        );
        assert_eq!(
            parse("Ctrl+Shift+1"),
            Some(Shortcut::new(
                Some(Modifiers::CONTROL | Modifiers::SHIFT),
                Code::Digit1
            ))
        );
        assert_eq!(
            parse("Ctrl+Up"),
            Some(Shortcut::new(Some(Modifiers::CONTROL), Code::ArrowUp))
        );
        assert_eq!(
            parse("Alt+/"),
            Some(Shortcut::new(Some(Modifiers::ALT), Code::Slash))
        );
        assert_eq!(
            parse("Ctrl+F5"),
            Some(Shortcut::new(Some(Modifiers::CONTROL), Code::F5))
        );
    }

    #[test]
    fn rejects_invalid_shortcuts() {
        for value in [
            "",
            "A",
            "Space",
            "Ctrl+",
            "Ctrl++A",
            "Ctrl+Nope",
            "Ctrl+A+B",
        ] {
            assert_eq!(parse(value), None, "{value:?}");
        }
    }
}
