use std::sync::Mutex;

pub struct CorrectionSession {
    pub original: String,
    pub corrected: String,
    pub source_app: String, // frontmost app at capture time; re-activated before paste
}

pub struct AppState {
    pub pending: Mutex<Option<CorrectionSession>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(None),
        }
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

    #[test]
    fn new_state_has_no_pending_session() {
        let state = AppState::new();
        assert!(state.pending.lock().unwrap().is_none());
    }

    #[test]
    fn default_state_has_no_pending_session() {
        let state = AppState::default();
        assert!(state.pending.lock().unwrap().is_none());
    }

    #[test]
    fn pending_can_be_set_and_taken() {
        let state = AppState::new();
        {
            let mut pending = state.pending.lock().unwrap();
            *pending = Some(CorrectionSession {
                original: "original".to_string(),
                corrected: "corrected".to_string(),
                source_app: "Safari".to_string(),
            });
        }
        let taken = state.pending.lock().unwrap().take();
        assert!(taken.is_some());
        let session = taken.unwrap();
        assert_eq!(session.original, "original");
        assert_eq!(session.corrected, "corrected");
        assert_eq!(session.source_app, "Safari");
        assert!(state.pending.lock().unwrap().is_none());
    }
}
