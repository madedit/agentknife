//! Reading a dev server's output: which state it is in, and the compiler output
//! of the current build (shown to the agent when the build fails). And the
//! project's log of everything printed ([`LogBook`]).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use ak_types::log::{BuildProgress, LogLine, LogSource};
use ak_types::preview::ServerState;
use ak_types::stack::{CrashMarkers, LogMarkers};
use tokio::sync::mpsc;

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

/// Lines a project's log keeps (the oldest go first).
const BOOK_LINES: usize = 5_000;

/// The open project's log: the output of all its programs, shared by whoever
/// prints (preview, real window, packaging, the AI's steps) and read by the
/// Log tab. Cloning shares it.
#[derive(Clone, Default)]
pub struct LogBook {
    inner: Arc<Mutex<Book>>,
}

#[derive(Default)]
struct Book {
    lines: VecDeque<LogLine>,
    next_seq: u64,
    listener: Option<mpsc::UnboundedSender<LogLine>>,
}

impl LogBook {
    /// Adds what a program printed, one entry per line. One read may hold
    /// several `\r`-separated redraws. Colors are removed and blank lines
    /// skipped. cargo's progress bar is not kept: the listener gets it as a
    /// progress report (`LogLine::progress`) for the build window.
    pub fn push(&self, source: LogSource, raw: &str) {
        let Ok(mut book) = self.inner.lock() else {
            return;
        };
        let at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        for part in raw.split(['\r', '\n']) {
            let text = strip_ansi(part);
            if text.trim().is_empty() {
                continue;
            }
            if let Some((progress, what)) = BuildProgress::parse(&text) {
                let report = LogLine {
                    seq: book.next_seq,
                    at_ms,
                    source,
                    text: what,
                    progress: Some(progress),
                };
                if let Some(tx) = &book.listener
                    && tx.send(report).is_err()
                {
                    book.listener = None;
                }
                continue;
            }
            book.next_seq += 1;
            let line = LogLine {
                seq: book.next_seq,
                at_ms,
                source,
                text,
                progress: None,
            };
            if let Some(tx) = &book.listener
                && tx.send(line.clone()).is_err()
            {
                book.listener = None;
            }
            book.lines.push_back(line);
            if book.lines.len() > BOOK_LINES {
                book.lines.pop_front();
            }
        }
    }

    /// Every line kept, oldest first.
    pub fn lines(&self) -> Vec<LogLine> {
        self.inner
            .lock()
            .map(|b| b.lines.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn clear(&self) {
        if let Ok(mut b) = self.inner.lock() {
            b.lines.clear();
        }
    }

    /// New lines from now on (one listener: a new one replaces the old).
    pub fn subscribe(&self) -> mpsc::UnboundedReceiver<LogLine> {
        let (tx, rx) = mpsc::unbounded_channel();
        if let Ok(mut b) = self.inner.lock() {
            b.listener = Some(tx);
        }
        rx
    }
}

/// Crash reports kept (the newest).
const MAX_CRASHES: usize = 5;
/// Message lines kept per report.
const CRASH_LINES: usize = 4;

/// Crashes in a running program's output (e.g. Rust panics in the backend),
/// read by the agent: the app's own errors never reach the page. Cloning shares
/// it.
#[derive(Clone, Default)]
pub struct CrashLog {
    inner: Arc<Mutex<Crashes>>,
}

#[derive(Default)]
struct Crashes {
    reports: Vec<String>,
    /// The report still reading its message, with its message line count.
    open: Option<(String, usize)>,
}

impl Crashes {
    fn close(&mut self) {
        if let Some((report, _)) = self.open.take() {
            if self.reports.len() == MAX_CRASHES {
                self.reports.remove(0);
            }
            self.reports.push(report);
        }
    }
}

impl CrashLog {
    /// Reads one line of output.
    pub fn feed(&self, markers: &CrashMarkers, line: &str) {
        let Ok(mut c) = self.inner.lock() else {
            return;
        };
        let text = line.trim_end();
        if text.contains(markers.start) {
            c.close();
            c.open = Some((text.trim_start().to_owned(), 0));
        } else if let Some((report, lines)) = c.open.as_mut() {
            if text.trim_start().starts_with(markers.end) || *lines == CRASH_LINES {
                c.close();
            } else if !text.trim().is_empty() {
                report.push('\n');
                report.push_str(text);
                *lines += 1;
            }
        }
    }

    /// The reports, oldest first.
    pub fn reports(&self) -> Vec<String> {
        self.inner
            .lock()
            .map(|c| {
                let mut all = c.reports.clone();
                all.extend(c.open.as_ref().map(|(r, _)| r.clone()));
                let older = all.len().saturating_sub(MAX_CRASHES);
                all.split_off(older)
            })
            .unwrap_or_default()
    }

    /// Forgets them (the program started again).
    pub fn clear(&self) {
        if let Ok(mut c) = self.inner.lock() {
            *c = Crashes::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_reports_keep_the_panic_and_its_message() {
        let markers = CrashMarkers {
            start: " panicked at ",
            end: "note: ",
        };
        let log = CrashLog::default();
        for line in [
            "Serving the app",
            "thread 'tokio-runtime-worker' panicked at src\\main.rs:12:5:",
            "called `Option::unwrap()` on a `None` value",
            "note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace",
            "still running",
            "thread 'main' panicked at src\\main.rs:30:9:",
        ] {
            log.feed(&markers, line);
        }
        assert_eq!(
            log.reports(),
            [
                "thread 'tokio-runtime-worker' panicked at src\\main.rs:12:5:\n\
                 called `Option::unwrap()` on a `None` value",
                "thread 'main' panicked at src\\main.rs:30:9:",
            ]
        );
        for n in 0..10 {
            log.feed(&markers, &format!("thread 'x' panicked at a.rs:{n}:1:"));
        }
        assert_eq!(log.reports().len(), MAX_CRASHES, "only the newest are kept");
        log.clear();
        assert!(log.reports().is_empty());
    }

    #[test]
    fn the_book_keeps_clean_lines_and_tells_the_listener() {
        let book = LogBook::default();
        let mut rx = book.subscribe();
        book.push(
            LogSource::Window,
            "    \u{1b}[96mBuilding\u{1b}[0m [==> ] 1/9: tauri\r   \u{1b}[92mCompiling\u{1b}[0m app v0.1.0\r",
        );
        book.push(LogSource::Ai, "");
        book.push(LogSource::Backend, "Hello");
        let lines = book.lines();
        let texts: Vec<_> = lines
            .iter()
            .map(|l| (l.seq, l.source, l.text.as_str()))
            .collect();
        assert_eq!(
            texts,
            [
                (1, LogSource::Window, "   Compiling app v0.1.0"),
                (2, LogSource::Backend, "Hello")
            ]
        );
        // The progress bar reaches the listener as a report, not a line.
        let report = rx.try_recv().unwrap();
        assert_eq!(report.progress, Some(BuildProgress { done: 1, total: 9 }));
        assert_eq!((report.seq, report.text.as_str()), (0, "tauri"));
        assert_eq!(rx.try_recv().map(|l| l.seq).ok(), Some(1));
        assert_eq!(rx.try_recv().map(|l| l.seq).ok(), Some(2));
        book.clear();
        assert!(book.lines().is_empty());
        book.push(LogSource::Ai, "next");
        assert_eq!(
            book.lines()[0].seq,
            3,
            "numbers keep counting after a clear"
        );
    }

    #[test]
    fn the_book_drops_the_oldest_lines() {
        let book = LogBook::default();
        for i in 0..BOOK_LINES + 2 {
            book.push(LogSource::Screens, &format!("line {i}"));
        }
        let lines = book.lines();
        assert_eq!(lines.len(), BOOK_LINES);
        assert_eq!(lines[0].text, "line 2");
    }

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
