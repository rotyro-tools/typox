//! OS integration: synthetic copy/paste keystrokes, which app has focus, and giving it back.
//!
//! The source app is identified by its process id on macOS, its X11 window id on Linux and its
//! process name on Windows. It's always sanitized before going into a script.

use std::time::Duration;

use tokio::time::sleep;

#[cfg(target_os = "macos")]
mod mac {
    const ACCESSIBILITY_HINT: &str = "Typox needs the Accessibility permission to copy and paste \
        for you. Turn it on in System Settings → Privacy & Security → Accessibility, then try again.";

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceFlagsState(state_id: i32) -> u64;
    }

    const HID_SYSTEM_STATE: i32 = 1;
    // kCGEventFlagMaskShift | Control | Alternate | Command
    const MODIFIER_MASK: u64 = 0x0002_0000 | 0x0004_0000 | 0x0008_0000 | 0x0010_0000;

    /// Whether Shift, Control, Option or Command is physically held down.
    pub fn modifiers_held() -> bool {
        // SAFETY: a pure query of the global keyboard state, callable from any thread.
        unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) & MODIFIER_MASK != 0 }
    }

    pub async fn osascript(script: &str) -> Result<String, String> {
        let out = tokio::process::Command::new("osascript")
            .args(["-e", script])
            .output()
            .await
            .map_err(|e| format!("osascript failed to start: {e}"))?;
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
        }
        Err(explain(&String::from_utf8_lossy(&out.stderr)))
    }

    /// Turns osascript's error into something the user can act on.
    pub fn explain(stderr: &str) -> String {
        let lower = stderr.to_lowercase();
        let denied = ["not allowed", "assistive", "(1002)", "(-1719)", "(-25211)"]
            .iter()
            .any(|marker| lower.contains(marker));
        if denied {
            ACCESSIBILITY_HINT.to_string()
        } else {
            format!("osascript failed: {}", stderr.trim())
        }
    }
}

/// Keeps only what a source app identifier can contain on this platform.
pub fn sanitize_source(source: &str) -> String {
    if cfg!(target_os = "windows") {
        // A process name, used inside a PowerShell string.
        source
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_'))
            .collect()
    } else {
        // A process id (macOS) or window id (Linux).
        source.chars().filter(char::is_ascii_digit).collect()
    }
}

/// Waits until the shortcut's modifier keys are released (up to ~0.6 s), so the synthetic
/// copy isn't received as Cmd+Ctrl+Shift+C by the target app.
pub async fn wait_for_modifier_release() {
    #[cfg(target_os = "macos")]
    {
        for _ in 0..60 {
            if !mac::modifiers_held() {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
        sleep(Duration::from_millis(30)).await;
    }
    #[cfg(not(target_os = "macos"))]
    sleep(Duration::from_millis(100)).await;
}

/// Presses Ctrl together with `key` (Windows and Linux).
#[cfg(not(target_os = "macos"))]
async fn send_ctrl_with(key: enigo::Key) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        use enigo::{Direction, Enigo, Keyboard, Settings};
        let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
        enigo
            .key(enigo::Key::Control, Direction::Press)
            .map_err(|e| e.to_string())?;
        let result = enigo.key(key, Direction::Click).map_err(|e| e.to_string());
        // Always release Ctrl, even if the key press failed.
        enigo
            .key(enigo::Key::Control, Direction::Release)
            .map_err(|e| e.to_string())?;
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Copies the selection in the frontmost app and returns that app's identifier.
pub async fn copy_selection() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let pid = mac::osascript(
            "tell application \"System Events\"\n\
             set frontApp to first process whose frontmost is true\n\
             set appPid to unix id of frontApp\n\
             keystroke \"c\" using command down\n\
             return appPid\n\
             end tell",
        )
        .await?;
        Ok(sanitize_source(&pid))
    }

    #[cfg(target_os = "windows")]
    {
        // Ctrl+Insert first: some apps treat a simulated Ctrl+C as a control character.
        send_ctrl_with(enigo::Key::Insert).await?;
        Ok(frontmost_windows_app().await)
    }

    #[cfg(target_os = "linux")]
    {
        let window = tokio::process::Command::new("xdotool")
            .arg("getactivewindow")
            .output()
            .await
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
            .unwrap_or_default();
        send_ctrl_with(enigo::Key::Unicode('c')).await?;
        Ok(sanitize_source(&window))
    }
}

/// A second way to copy, for apps that ignored Ctrl+Insert: Ctrl+C.
/// Returns whether anything was sent.
#[cfg(target_os = "windows")]
pub async fn copy_selection_fallback() -> Result<bool, String> {
    send_ctrl_with(enigo::Key::C).await?;
    Ok(true)
}

/// Only Windows needs a second way to copy.
#[cfg(not(target_os = "windows"))]
pub async fn copy_selection_fallback() -> Result<bool, String> {
    Ok(false)
}

#[cfg(target_os = "windows")]
async fn frontmost_windows_app() -> String {
    let out = tokio::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
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
        .await;
    out.map(|o| sanitize_source(String::from_utf8_lossy(&o.stdout).trim()))
        .unwrap_or_default()
}

/// Gives focus back to the source app.
pub async fn activate(source: &str) {
    let source = sanitize_source(source);
    if source.is_empty() {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "tell application \"System Events\" to set frontmost of first process whose unix id is {source} to true"
        );
        if let Err(e) = mac::osascript(&script).await {
            eprintln!("[typox] failed to re-focus process {source}: {e}");
        }
    }
    #[cfg(target_os = "windows")]
    {
        let script = format!("(New-Object -ComObject WScript.Shell).AppActivate('{source}')");
        let _ = tokio::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .status()
            .await;
    }
    #[cfg(target_os = "linux")]
    {
        let _ = tokio::process::Command::new("xdotool")
            .args(["windowactivate", "--sync", &source])
            .status()
            .await;
    }
}

/// Gives focus back to the source app and pastes the clipboard into it.
pub async fn activate_and_paste(source: &str) -> Result<(), String> {
    let source = sanitize_source(source);
    #[cfg(target_os = "macos")]
    {
        if source.is_empty() {
            return Err("Typox couldn't tell which app to paste into.".to_string());
        }
        // Wait until the app is actually frontmost before pasting into it.
        let script = format!(
            "tell application \"System Events\"\n\
             set p to first process whose unix id is {source}\n\
             set frontmost of p to true\n\
             repeat 25 times\n\
             if frontmost of p then exit repeat\n\
             delay 0.02\n\
             end repeat\n\
             keystroke \"v\" using command down\n\
             end tell"
        );
        mac::osascript(&script).await.map(|_| ()).map_err(|e| {
            if e.contains("Can’t get") || e.contains("Can't get") {
                "The app you were editing is no longer open.".to_string()
            } else {
                e
            }
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        activate(&source).await;
        sleep(Duration::from_millis(50)).await;
        #[cfg(target_os = "windows")]
        let paste_key = enigo::Key::V;
        #[cfg(not(target_os = "windows"))]
        let paste_key = enigo::Key::Unicode('v');
        send_ctrl_with(paste_key).await
    }
}

/// Brings Typox's own window to the front. Needed on macOS, where a menu-bar app showing a
/// window does not take focus from the active app by itself.
pub async fn focus_self() {
    #[cfg(target_os = "macos")]
    {
        let pid = std::process::id();
        let script = format!(
            "tell application \"System Events\" to set frontmost of first process whose unix id is {pid} to true"
        );
        let _ = mac::osascript(&script).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_source_keeps_only_safe_characters() {
        if cfg!(target_os = "windows") {
            assert_eq!(sanitize_source("Code"), "Code");
            assert_eq!(sanitize_source("app'); rm -rf /"), "app rm -rf ");
        } else {
            assert_eq!(sanitize_source("12345\n"), "12345");
            assert_eq!(sanitize_source("1 to true\ndo shell script \"x\""), "1");
            assert_eq!(sanitize_source(""), "");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn accessibility_errors_explain_how_to_fix_them() {
        let stderr = "execution error: System Events got an error: osascript is not allowed to send keystrokes. (1002)";
        assert!(mac::explain(stderr).contains("Accessibility"));
        assert!(mac::explain("syntax error").starts_with("osascript failed"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reading_the_modifier_state_does_not_crash() {
        let _ = mac::modifiers_held();
    }
}
