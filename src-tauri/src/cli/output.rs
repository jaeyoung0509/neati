//! Terminal output for the CLI: results on stdout, progress on stderr.
//!
//! Progress is drawn only when stderr is a terminal and `--quiet` is absent,
//! so an agent that reads stdout gets one clean JSON object and nothing else.

use std::io::{IsTerminal, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

/// The JSON contract name every `--json` object carries.
pub const SCHEMA: &str = "neati.cli/1";

/// One JSON result: `{"schema": …, "command": …, …payload}`.
#[derive(Serialize)]
pub struct Envelope<'a, T: Serialize> {
    pub schema: &'static str,
    pub command: &'a str,
    #[serde(flatten)]
    pub payload: T,
}

pub fn print_json<T: Serialize>(command: &str, payload: T) -> Result<(), String> {
    let envelope = Envelope {
        schema: SCHEMA,
        command,
        payload,
    };
    let text = serde_json::to_string_pretty(&envelope).map_err(|error| error.to_string())?;
    println!("{text}");
    Ok(())
}

#[derive(Serialize)]
struct ErrorPayload<'a> {
    error: &'a str,
}

/// Errors go to stdout as JSON in `--json` mode so an agent can parse them,
/// and to stderr otherwise. Paths stay readable (this is the person's own
/// terminal); anything shaped like a secret is still redacted.
pub fn print_error(command: &str, json: bool, message: &str) {
    let message = crate::privacy::secrets::redact(message);
    if json {
        let _ = print_json(command, ErrorPayload { error: &message });
    } else {
        eprintln!("neati: {message}");
    }
}

/// Bytes the way the app shows them (1024-based, one decimal).
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let rounded = (value * 10.0).round() / 10.0;
    if rounded.fract() == 0.0 {
        format!("{rounded:.0} {}", UNITS[unit])
    } else {
        format!("{rounded:.1} {}", UNITS[unit])
    }
}

/// Replaces the home prefix with `~` for display.
pub fn display_path(path: &str, home: Option<&std::path::Path>) -> String {
    if let Some(home) = home.and_then(|home| home.to_str()) {
        if let Some(rest) = path.strip_prefix(home) {
            if rest.is_empty() || rest.starts_with('/') {
                return format!("~{rest}");
            }
        }
    }
    path.to_string()
}

/// One status line on stderr, redrawn in place and throttled.
pub struct Progress {
    enabled: bool,
    last: Mutex<Option<Instant>>,
}

impl Progress {
    const INTERVAL: Duration = Duration::from_millis(80);
    const WIDTH: usize = 76;

    pub fn new(quiet: bool) -> Self {
        Self {
            enabled: !quiet && std::io::stderr().is_terminal(),
            last: Mutex::new(None),
        }
    }

    pub fn update(&self, message: &str) {
        if !self.enabled {
            return;
        }
        let mut last = self
            .last
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        if last.is_some_and(|previous| now.duration_since(previous) < Self::INTERVAL) {
            return;
        }
        *last = Some(now);
        let line: String = message.chars().take(Self::WIDTH).collect();
        let mut stderr = std::io::stderr();
        let _ = write!(stderr, "\r\x1b[2K{line}");
        let _ = stderr.flush();
    }

    pub fn clear(&self) {
        if self.enabled {
            let mut stderr = std::io::stderr();
            let _ = write!(stderr, "\r\x1b[2K");
            let _ = stderr.flush();
        }
    }
}

/// The terminal rendering of the use cases' activity.
impl crate::services::ports::ActivityPort for Progress {
    fn report(&self, activity: crate::services::ports::Activity) {
        use crate::services::ports::Activity;
        let line = match activity {
            Activity::Scanning { location } => format!("Scanning {location}…"),
            Activity::Measuring { project, kind } => format!("Measuring {project} · {kind}…"),
            Activity::Removing { name, index, total } => {
                format!("Removing {name} ({}/{total})…", index + 1)
            }
            Activity::Checking { source } => format!("Checked {source}…"),
        };
        self.update(&line);
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_match_the_app_formatter() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(78 * 1024 * 1024 * 1024), "78 GB");
        assert_eq!(format_bytes(u64::MAX), "16384 PB");
    }

    #[test]
    fn home_is_shortened_only_on_a_component_boundary() {
        let home = std::path::Path::new("/Users/me");
        assert_eq!(display_path("/Users/me/code", Some(home)), "~/code");
        assert_eq!(display_path("/Users/me", Some(home)), "~");
        assert_eq!(
            display_path("/Users/meta/code", Some(home)),
            "/Users/meta/code"
        );
        assert_eq!(display_path("/opt/x", None), "/opt/x");
    }
}
