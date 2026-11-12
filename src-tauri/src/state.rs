use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use tauri_plugin_global_shortcut::Shortcut;

pub struct CorrectionSession {
    /// Changes on every capture and retry, so a late AI reply for an older request is dropped.
    pub id: u64,
    pub original: String,
    pub corrected: String,
    /// The app that had focus at capture time, as `platform::frontmost_app` returns it.
    pub source_app: String,
    /// Text that was on the clipboard before Typox used it, put back afterwards.
    pub saved_clipboard: Option<String>,
}

pub struct AppState {
    pub pending: Mutex<Option<CorrectionSession>>,
    /// Set while a capture is running, so a second shortcut press is ignored.
    busy: AtomicBool,
    next_id: AtomicU64,
    /// The global shortcut currently registered with the OS.
    pub shortcut: Mutex<Option<Shortcut>>,
    /// API keys already read this run (`None` = no key saved), so the keychain is asked once.
    pub api_keys: Mutex<HashMap<String, Option<String>>>,
    /// False when the OS keychain could not be used.
    pub keychain_ok: AtomicBool,
}

/// Clears the busy flag when dropped, so a failed capture never leaves Typox stuck.
pub struct BusyGuard<'a> {
    flag: &'a AtomicBool,
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(None),
            busy: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            shortcut: Mutex::new(None),
            api_keys: Mutex::new(HashMap::new()),
            keychain_ok: AtomicBool::new(true),
        }
    }

    /// Marks a capture as running, or returns `None` if one already is.
    pub fn try_start(&self) -> Option<BusyGuard<'_>> {
        self.busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| BusyGuard { flag: &self.busy })
    }

    pub fn next_session_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: u64) -> CorrectionSession {
        CorrectionSession {
            id,
            original: "original".to_string(),
            corrected: "corrected".to_string(),
            source_app: "123".to_string(),
            saved_clipboard: Some("clipboard".to_string()),
        }
    }

    #[test]
    fn new_state_has_no_pending_session() {
        let state = AppState::default();
        assert!(state.pending.lock().unwrap().is_none());
        assert!(state.keychain_ok.load(Ordering::SeqCst));
    }

    #[test]
    fn pending_can_be_set_and_taken() {
        let state = AppState::new();
        *state.pending.lock().unwrap() = Some(session(7));
        let taken = state.pending.lock().unwrap().take().unwrap();
        assert_eq!(taken.original, "original");
        assert_eq!(taken.saved_clipboard.as_deref(), Some("clipboard"));
        assert!(state.pending.lock().unwrap().is_none());
    }

    #[test]
    fn busy_guard_blocks_a_second_capture_until_dropped() {
        let state = AppState::new();
        let guard = state.try_start();
        assert!(guard.is_some());
        assert!(state.try_start().is_none());
        drop(guard);
        assert!(state.try_start().is_some());
    }

    #[test]
    fn session_ids_are_unique_and_increasing() {
        let state = AppState::new();
        let a = state.next_session_id();
        let b = state.next_session_id();
        assert!(b > a);
    }
}
