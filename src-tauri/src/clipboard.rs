use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tokio::time::sleep;

use crate::ai;
use crate::settings;
use crate::state::{AppState, CorrectionSession};

pub async fn capture_and_correct(app: AppHandle) {
    if let Err(e) = do_capture(&app).await {
        eprintln!("[typox] error: {e}");
        if let Some(widget) = app.get_webview_window("widget") {
            let _ = widget.show();
            let _ = widget.set_focus();
            let _ = app.emit_to(
                "widget",
                "correction-error",
                serde_json::json!({ "message": e }),
            );
        }
    }
}

async fn do_capture(app: &AppHandle) -> Result<(), String> {
    // ── 1. Capture cursor position + clear clipboard ──────────────────────────
    let (cursor_x, cursor_y) = tokio::task::spawn_blocking(|| {
        use arboard::Clipboard;
        use enigo::{Enigo, Mouse, Settings as EnigoSettings};

        let enigo = Enigo::new(&EnigoSettings::default()).map_err(|e| e.to_string())?;
        let (x, y) = enigo.location().unwrap_or((0, 0));
        let _ = Clipboard::new().and_then(|mut c| c.set_text(""));
        Ok::<(i32, i32), String>((x, y))
    })
    .await
    .map_err(|e| e.to_string())??;

    // ── 2. Simulate copy ──────────────────────────────────────────────────────
    sleep(Duration::from_millis(100)).await;

    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("osascript")
            .args([
                "-e",
                "tell application \"System Events\" to keystroke \"c\" using command down",
            ])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!(
                "osascript failed ({})",
                status.code().unwrap_or(-1)
            ));
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        tokio::task::spawn_blocking(|| {
            use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};
            let mut enigo = Enigo::new(&EnigoSettings::default()).map_err(|e| e.to_string())?;
            enigo
                .key(Key::Control, Direction::Press)
                .map_err(|e| e.to_string())?;
            enigo
                .key(Key::Unicode('c'), Direction::Click)
                .map_err(|e| e.to_string())?;
            enigo
                .key(Key::Control, Direction::Release)
                .map_err(|e| e.to_string())?;
            Ok::<(), String>(())
        })
        .await
        .map_err(|e| e.to_string())??;
    }

    // ── 3. Read clipboard, retry up to 3× until non-empty ────────────────────
    sleep(Duration::from_millis(150)).await;
    let mut selected = String::new();
    for _ in 0..3 {
        let text = tokio::task::spawn_blocking(|| {
            use arboard::Clipboard;
            Clipboard::new()
                .and_then(|mut c| c.get_text())
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();

        if !text.is_empty() {
            selected = text;
            break;
        }
        sleep(Duration::from_millis(100)).await;
    }

    if selected.is_empty() {
        return Ok(());
    }

    // ── 4. Capture source app BEFORE showing widget ───────────────────────────
    #[cfg(target_os = "macos")]
    let source_app = {
        let out = std::process::Command::new("osascript")
            .args(["-e", "tell application \"System Events\" to get name of first process whose frontmost is true"])
            .output()
            .map_err(|e| e.to_string())?;
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    #[cfg(target_os = "windows")]
    let source_app = std::process::Command::new("powershell")
        .args([
            "-NoProfile", "-Command",
            "Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; \
             public class W { \
               [DllImport(\"user32\")] public static extern IntPtr GetForegroundWindow(); \
               [DllImport(\"user32\")] public static extern int GetWindowThreadProcessId(IntPtr h, out int p); \
             }'; \
             $h=[W]::GetForegroundWindow(); $p=0; \
             [W]::GetWindowThreadProcessId($h,[ref]$p); \
             (Get-Process -Id $p).Name",
        ])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default();
    #[cfg(target_os = "linux")]
    let source_app = std::process::Command::new("xdotool")
        .arg("getactivewindow")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default();

    // ── 5. Show widget immediately (skeleton) ─────────────────────────────────
    let widget = app
        .get_webview_window("widget")
        .ok_or_else(|| "widget window not found".to_string())?;

    // Clamp widget position so it never appears off-screen
    let (pos_x, pos_y) = if let Ok(Some(monitor)) = app.primary_monitor() {
        let sz = monitor.size();
        let max_x = (sz.width as i32 - 640).max(0);
        let max_y = (sz.height as i32 - 160).max(0);
        (
            (cursor_x + 12).clamp(0, max_x),
            (cursor_y + 12).clamp(0, max_y),
        )
    } else {
        (cursor_x + 12, cursor_y + 12)
    };
    widget
        .set_position(tauri::PhysicalPosition::new(pos_x, pos_y))
        .map_err(|e| e.to_string())?;
    widget.show().map_err(|e| e.to_string())?;
    widget.set_focus().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    {
        let pid = std::process::id();
        let script = format!(
            "tell application \"System Events\" to set frontmost of first process whose unix id is {pid} to true"
        );
        let _ = std::process::Command::new("osascript")
            .args(["-e", &script])
            .status();
    }
    app.emit_to(
        "widget",
        "correction-started",
        serde_json::json!({ "original": selected }),
    )
    .map_err(|e| e.to_string())?;

    // ── 6. Store session ──────────────────────────────────────────────────────
    {
        let state = app.state::<AppState>();
        let mut pending = state.pending.lock().map_err(|e| e.to_string())?;
        *pending = Some(CorrectionSession {
            original: selected.clone(),
            corrected: String::new(),
            source_app,
        });
    }

    // ── 7. Call AI provider ───────────────────────────────────────────────────
    let settings = settings::load(app);
    let provider = ai::make_provider(app, &settings);
    let corrected = provider.correct_text(&selected).await?.trim().to_string();
    let no_changes = corrected == selected.trim();

    // ── 8. Update state with result ───────────────────────────────────────────
    {
        let state = app.state::<AppState>();
        let mut pending = state.pending.lock().map_err(|e| e.to_string())?;
        if let Some(session) = pending.as_mut() {
            session.corrected = corrected.clone();
        }
    }

    // ── 9. Emit correction-ready ──────────────────────────────────────────────
    app.emit_to(
        "widget",
        "correction-ready",
        serde_json::json!({ "original": selected, "corrected": corrected, "noChanges": no_changes }),
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}
