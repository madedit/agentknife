//! Reading a dev server's output: which state it is in, and the compiler output
//! of the current build (shown to the agent when the build fails).

use std::collections::VecDeque;

use ak_types::preview::ServerState;
use ak_types::stack::LogMarkers;

/// Lines kept per build (compiler errors come before the failure line).
const MAX_LINES: usize = 400;
/// Characters of output handed out with an error.
const MAX_CHARS: usize = 20_000;

/// Removes ANSI escape sequences (colors) and trailing `\r`.
pub fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                // Parameters until the final byte (a letter).
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out.trim_end_matches('\r').to_owned()
}

/// The last lines, joined, at most [`MAX_CHARS`] (the end is kept: errors summarise
/// at the bottom).
pub fn tail(lines: &VecDeque<String>) -> String {
    let joined = lines.iter().cloned().collect::<Vec<_>>().join("\n");
    let count = joined.chars().count();
    if count <= MAX_CHARS {
        return joined;
    }
    joined.chars().skip(count - MAX_CHARS).collect()
}

fn push(lines: &mut VecDeque<String>, line: String) {
    if lines.len() == MAX_LINES {
        lines.pop_front();
    }
    lines.push_back(line);
}

/// State of a dev server that rebuilds on its own (frontend).
pub struct BuildTracker {
    markers: LogMarkers,
    state: ServerState,
    lines: VecDeque<String>,
}

impl BuildTracker {
    pub fn new(markers: LogMarkers) -> Self {
        Self {
            markers,
            state: ServerState::Building,
            lines: VecDeque::new(),
        }
    }

    pub fn state(&self) -> &ServerState {
        &self.state
    }

    pub fn output(&self) -> String {
        tail(&self.lines)
    }

    /// Reads one output line; returns the new state if it changed (or, in an
    /// error, if the error output grew).
    pub fn feed(&mut self, raw: &str) -> Option<ServerState> {
        let line = strip_ansi(raw);
        let has = |list: &[&str]| list.iter().any(|m| line.contains(m));
        if has(self.markers.building) {
            self.lines.clear();
            push(&mut self.lines, line);
            return self.set(ServerState::Building);
        }
        if line.trim().is_empty() && !self.state.is_error() {
            return None;
        }
        push(&mut self.lines, line.clone());
        if has(self.markers.error) {
            return self.set(ServerState::Error {
                output: self.output(),
            });
        }
        if has(self.markers.success) {
            return self.set(ServerState::Ready);
        }
        // Details printed after the failure line (e.g. "Caused by: …").
        if self.state.is_error() {
            return self.set(ServerState::Error {
                output: self.output(),
            });
        }
        None
    }

    fn set(&mut self, state: ServerState) -> Option<ServerState> {
        if state == self.state {
            return None;
        }
        self.state = state.clone();
        Some(state)
    }
}

/// The output of a process that builds once and then runs (backend): the last
/// lines, for when it fails.
#[derive(Default)]
pub struct Tail {
    lines: VecDeque<String>,
}

impl Tail {
    pub fn push(&mut self, raw: &str) -> String {
        let line = strip_ansi(raw);
        push(&mut self.lines, line.clone());
        line
    }

    pub fn text(&self) -> String {
        tail(&self.lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRUNK: LogMarkers = LogMarkers {
        building: &["INFO starting build"],
        success: &["INFO success"],
        error: &["ERROR error"],
    };

    #[test]
    fn ansi_is_stripped() {
        assert_eq!(strip_ansi("Done in \u{1b}[31m1s\u{1b}[39m\r"), "Done in 1s");
        assert_eq!(
            strip_ansi("\u{1b}[3m\u{1b}[1mtailwind\u{1b}[0m"),
            "tailwind"
        );
    }

    /// Real trunk 0.21.14 output: build, spurious rebuild, error, recovery.
    #[test]
    fn follows_trunk_through_an_error_and_back() {
        let mut t = BuildTracker::new(TRUNK);
        let mut states = Vec::new();
        for line in [
            "2026-10-20T11:42:07.1Z  INFO starting build",
            "   Compiling app-ui v0.1.0",
            "2026-10-20T11:42:09.6Z  INFO applying new distribution",
            "2026-10-20T11:42:09.6Z  INFO success",
            "2026-10-20T11:42:09.6Z  INFO starting build",
            "2026-10-20T11:42:12.0Z  INFO success",
            "2026-10-20T11:46:12.0Z  INFO starting build",
            "error[E0308]: mismatched types",
            "   --> src\\main.rs:171:22",
            "error: could not compile `app-ui` (bin \"app-ui\") due to 1 previous error",
            "2026-10-20T11:46:14.3Z ERROR error",
            "error from build pipeline",
            "",
            "Caused by:",
            "    0: HTML build pipeline failed (1 errors), showing first",
        ] {
            if let Some(s) = t.feed(line) {
                states.push(s);
            }
        }
        let kinds: Vec<&str> = states
            .iter()
            .map(|s| match s {
                ServerState::Building => "building",
                ServerState::Ready => "ready",
                ServerState::Error { .. } => "error",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds[..5],
            ["ready", "building", "ready", "building", "error"]
        );
        let ServerState::Error { output } = t.state() else {
            panic!("expected an error");
        };
        assert!(
            output.contains("error[E0308]: mismatched types"),
            "{output}"
        );
        assert!(output.contains("src\\main.rs:171:22"));
        assert!(
            output.contains("Caused by:"),
            "details after the failure line"
        );
        assert!(
            !output.contains("Compiling app-ui"),
            "only this build's output"
        );

        assert_eq!(
            t.feed("2026-10-20T11:48:20Z  INFO starting build"),
            Some(ServerState::Building)
        );
        assert_eq!(
            t.feed("2026-10-20T11:48:23Z  INFO success"),
            Some(ServerState::Ready)
        );
    }

    #[test]
    fn output_is_bounded_and_keeps_the_end() {
        let mut t = Tail::default();
        for i in 0..1000 {
            t.push(&format!("line {i} {}", "x".repeat(50)));
        }
        let text = t.text();
        assert!(text.chars().count() <= MAX_CHARS);
        assert!(text.ends_with(&format!("line 999 {}", "x".repeat(50))));
    }
}
