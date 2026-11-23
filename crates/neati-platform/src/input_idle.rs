//! How long the person has not touched the keyboard, mouse or trackpad (#507).
//!
//! macOS answers through Core Graphics for the combined session state, which
//! is the same source the system uses for display sleep. Other platforms
//! report `None`: an unknown answer never counts as idle.

/// Seconds since the last input event in this login session, when known.
pub fn seconds_since_last_input() -> Option<u64> {
    platform::seconds_since_last_input()
}

#[cfg(target_os = "macos")]
mod platform {
    /// `kCGEventSourceStateCombinedSessionState`.
    const COMBINED_SESSION_STATE: i32 = 0;
    /// `kCGAnyInputEventType`.
    const ANY_INPUT_EVENT: u32 = u32::MAX;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
    }

    pub fn seconds_since_last_input() -> Option<u64> {
        // SAFETY: a pure query with value arguments; it reads no pointer and
        // has no preconditions beyond a running window server session, and it
        // returns a large value rather than failing when there is none.
        let seconds = unsafe {
            CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION_STATE, ANY_INPUT_EVENT)
        };
        (seconds.is_finite() && seconds >= 0.0).then_some(seconds as u64)
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn seconds_since_last_input() -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(not(target_os = "macos"))]
    fn other_platforms_never_claim_idle() {
        assert_eq!(super::seconds_since_last_input(), None);
    }
}
