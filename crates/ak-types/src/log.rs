//! The open project's log: everything its programs printed, for the Log tab.

use serde::{Deserialize, Serialize};

/// Which program printed a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogSource {
    /// The preview's screens (trunk).
    Screens,
    /// The preview's app backend (`cargo run`).
    Backend,
    /// "Run in a real window" (`cargo tauri dev`).
    Window,
    /// Packaging (`cargo tauri build`).
    Bundle,
    /// The AI's steps: what ran, then its output.
    Ai,
}

impl LogSource {
    pub const ALL: [LogSource; 5] = [
        LogSource::Screens,
        LogSource::Backend,
        LogSource::Window,
        LogSource::Bundle,
        LogSource::Ai,
    ];

    /// `screens`, `backend`, …: fluent `log-source-<key>`.
    pub fn key(self) -> &'static str {
        match self {
            LogSource::Screens => "screens",
            LogSource::Backend => "backend",
            LogSource::Window => "window",
            LogSource::Bundle => "bundle",
            LogSource::Ai => "ai",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    /// Counts up from 1 per open project.
    pub seq: u64,
    /// Milliseconds since the Unix epoch.
    pub at_ms: u64,
    pub source: LogSource,
    /// One line, without colors.
    pub text: String,
}

impl LogLine {
    /// Looks like an error (shown in red).
    pub fn is_error(&self) -> bool {
        let t = self.text.trim_start();
        t.starts_with("error") || t.starts_with("thread '") || t.contains("panicked at")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> LogLine {
        LogLine {
            seq: 1,
            at_ms: 0,
            source: LogSource::Backend,
            text: text.into(),
        }
    }

    #[test]
    fn errors_are_compiler_errors_and_panics() {
        assert!(line("error[E0308]: mismatched types").is_error());
        assert!(line("error: could not compile `app`").is_error());
        assert!(line("thread 'main' (3864) panicked at src/main.rs:3:5:").is_error());
        assert!(!line("   Compiling app v0.1.0").is_error());
        assert!(!line("warning: unused variable").is_error());
    }

    #[test]
    fn sources_travel_as_snake_case_keys() {
        for s in LogSource::ALL {
            assert_eq!(
                serde_json::to_value(s).unwrap_or_default(),
                serde_json::Value::String(s.key().into())
            );
        }
    }
}
