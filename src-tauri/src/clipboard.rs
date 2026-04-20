use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tokio::time::sleep;

use crate::ai;
use crate::settings;
use crate::state::{AppState, CorrectionSession};

async fn read_clipboard_with_retries(attempts: usize) -> String {
    for _ in 0..attempts {
        let text = tokio::task::spawn_blocking(|| {
            use arboard::Clipboard;
            Clipboard::new()
                .and_then(|mut c| c.get_text())
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();

        if !text.is_empty() {
            return text;
        }
        sleep(Duration::from_millis(100)).await;
    }
    String::new()
}

#[cfg(target_os = "windows")]
async fn send_windows_copy(use_ctrl_insert: bool) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};
        let mut enigo = Enigo::new(&EnigoSettings::default()).map_err(|e| e.to_string())?;

        enigo
            .key(Key::Control, Direction::Press)
            .map_err(|e| e.to_string())?;

        if use_ctrl_insert {
            enigo
                .key(Key::Insert, Direction::Click)
                .map_err(|e| e.to_string())?;
        } else {
            enigo
                .key(Key::C, Direction::Click)
                .map_err(|e| e.to_string())?;
        }

        enigo
            .key(Key::Control, Direction::Release)
            .map_err(|e| e.to_string())?;

        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())?
}

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

    // ── 2. Simulate copy + capture source app (macOS: single osascript call) ──
    sleep(Duration::from_millis(100)).await;

    #[cfg(target_os = "macos")]
    let source_app = {
        let out = std::process::Command::new("osascript")
            .args([
                "-e",
                "tell application \"System Events\"\n\
                 set appName to name of first process whose frontmost is true\n\
                 keystroke \"c\" using command down\n\
                 return appName\n\
                 end tell",
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!(
                "osascript failed ({}): {}",
                out.status.code().unwrap_or(-1),
                stderr.trim()
            ));
        }
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    #[cfg(target_os = "windows")]
    {
        // Prefer Ctrl+Insert first; some Windows apps treat Ctrl+C input simulation
        // as a control character instead of a copy shortcut.
        send_windows_copy(true).await?;
    }

    #[cfg(target_os = "linux")]
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
    #[allow(unused_mut)]
    let mut selected = read_clipboard_with_retries(3).await;

    #[cfg(target_os = "windows")]
    if selected.is_empty() {
        send_windows_copy(false).await?;
        sleep(Duration::from_millis(120)).await;
        selected = read_clipboard_with_retries(3).await;
    }

    if selected.is_empty() {
        return Err("No text selected — select some text before triggering Typox.".to_string());
    }

    // ── 4. Capture source app BEFORE showing widget (non-macOS) ──────────────
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

    const WIDGET_W: i32 = 640;
    const WIDGET_H: i32 = 160;

    let monitors = app.available_monitors().unwrap_or_default();
    let monitor = monitors
        .into_iter()
        .find(|m| {
            let pos = m.position();
            let sz = m.size();
            cursor_x >= pos.x
                && cursor_x < pos.x + sz.width as i32
                && cursor_y >= pos.y
                && cursor_y < pos.y + sz.height as i32
        })
        .or_else(|| app.primary_monitor().ok().flatten());

    // Center widget on the active monitor, accounting for scale factor
    let (pos_x, pos_y) = if let Some(m) = monitor {
        let pos = m.position();
        let sz = m.size();
        let scale = m.scale_factor();
        let widget_w_phys = (WIDGET_W as f64 * scale) as i32;
        let widget_h_phys = (WIDGET_H as f64 * scale) as i32;
        (
            pos.x + (sz.width as i32 - widget_w_phys) / 2,
            pos.y + (sz.height as i32 - widget_h_phys) / 2,
        )
    } else {
        (cursor_x - WIDGET_W / 2, cursor_y - WIDGET_H / 2)
    };
    widget.show().map_err(|e| e.to_string())?;
    widget
        .set_position(tauri::PhysicalPosition::new(pos_x, pos_y))
        .map_err(|e| e.to_string())?;
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
