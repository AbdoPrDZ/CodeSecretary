//! Fallback for operating systems without presence/focus support.

use std::time::Duration;

use super::{FocusedWindow, PlatformError};

pub struct Native;

impl Native {
    pub fn connect() -> Result<Self, PlatformError> {
        Err(PlatformError::Unsupported(format!(
            "presence and focus are not implemented for {}",
            std::env::consts::OS
        )))
    }

    pub fn idle_time(&mut self) -> Result<Duration, PlatformError> {
        unreachable!("connect always fails")
    }

    pub fn focused_window(&mut self) -> Result<Option<FocusedWindow>, PlatformError> {
        unreachable!("connect always fails")
    }

    pub fn titles_available(&self) -> bool {
        false
    }
}

pub fn session_notes() -> Vec<String> {
    Vec::new()
}
