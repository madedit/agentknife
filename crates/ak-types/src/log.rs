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
    /// Set on a progress report (cargo's `Building [==> ] 12/274: tauri`): not
    /// a line to show, but how far the source's build is. `text` then names
    /// what is compiling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<BuildProgress>,
}

/// How many of a build's compile steps are done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildProgress {
    pub done: u32,
    pub total: u32,
}

impl BuildProgress {
    /// Reads cargo's progress bar (`    Building [=====>   ] 120/274: tauri, …`):
    /// the counts and what is compiling.
    pub fn parse(line: &str) -> Option<(BuildProgress, String)> {
        let rest = line.trim_start().strip_prefix("Building [")?;
        let (_, rest) = rest.split_once("] ")?;
        let (counts, what) = rest.split_once(':').unwrap_or((rest, ""));
        let (done, total) = counts.trim().split_once('/')?;
        let progress = BuildProgress {
            done: done.parse().ok()?,
            total: total.parse().ok()?,
        };
        (progress.total > 0).then(|| (progress, what.trim().to_owned()))
    }

    /// Whole percent done.
    pub fn percent(self) -> u32 {
        let total = u64::from(self.total.max(1));
        let done = u64::from(self.done.min(self.total));
        u32::try_from(done * 100 / total).unwrap_or(100)
    }
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
            progress: None,
        }
    }

    #[test]
    fn cargo_progress_bars_become_counts() {
        let (p, what) = BuildProgress::parse(
            "    Building [=====>                 ] 120/274: tauri, serde_core(build.rs), ve…",
        )
        .unwrap();
        assert_eq!(
            p,
            BuildProgress {
                done: 120,
                total: 274
            }
        );
        assert_eq!(what, "tauri, serde_core(build.rs), ve…");
        let (p, what) = BuildProgress::parse("    Building [ ] 0/3").unwrap();
        assert_eq!((p.done, p.total, what.as_str()), (0, 3, ""));
        assert!(BuildProgress::parse("   Compiling tauri v2.12.4").is_none());
        assert!(BuildProgress::parse("    Building [ ] 0/0: x").is_none());
        assert!(BuildProgress::parse("    Building [ ] a/b: x").is_none());
    }

    #[test]
    fn percent_is_whole_and_capped() {
        let p = |done, total| BuildProgress { done, total }.percent();
        assert_eq!(p(137, 274), 50);
        assert_eq!(p(273, 274), 99);
        assert_eq!(p(5, 3), 100);
        assert_eq!(p(0, 0), 0);
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
