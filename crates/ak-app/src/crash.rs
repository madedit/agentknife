//! Crash reports: written on this computer when AgentKnife panics, never sent
//! anywhere by AgentKnife. The settings page lists them; the user may copy one
//! to share it.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ak_types::ipc::{CrashReport, CrashReports};

/// Under AgentKnife's data folder.
pub const FOLDER: &str = "crash-reports";
/// Reports kept; older ones are removed when a new one is written.
const KEEP: usize = 20;

/// Writes a report for every panic (then the default hook prints as usual).
pub fn install(folder: PathBuf) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let place = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let _ = write(&folder, &report(&message, &place, &backtrace), now());
        default(info);
    }));
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// What a report says: no keys, no project content, only where AgentKnife stopped.
pub fn report(message: &str, place: &str, backtrace: &str) -> String {
    format!(
        "AgentKnife {} crash report\nsystem: {} {}\nmessage: {message}\nat: {place}\n\nbacktrace:\n{backtrace}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

/// Saves `text` as `<unix time>.txt` and keeps the newest [`KEEP`].
pub fn write(folder: &Path, text: &str, at: u64) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(folder)?;
    let mut path = folder.join(format!("{at}.txt"));
    let mut n = 1;
    while path.exists() {
        path = folder.join(format!("{at}-{n}.txt"));
        n += 1;
    }
    std::fs::write(&path, text)?;
    for old in list(folder).reports.into_iter().skip(KEEP) {
        let _ = std::fs::remove_file(old.path);
    }
    Ok(path)
}

/// The reports in `folder`, newest first.
pub fn list(folder: &Path) -> CrashReports {
    let mut reports: Vec<CrashReport> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .filter_map(|p| {
            let created_at = p
                .file_stem()?
                .to_string_lossy()
                .split('-')
                .next()?
                .parse()
                .ok()?;
            Some(CrashReport {
                text: std::fs::read_to_string(&p).ok()?,
                path: p.display().to_string(),
                created_at,
            })
        })
        .collect();
    reports.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.path.cmp(&a.path)));
    CrashReports {
        folder: folder.display().to_string(),
        reports,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_are_written_listed_and_capped() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let text = report(
            "index out of bounds",
            "crates/ak-app/src/turn.rs:42",
            "0: main",
        );
        assert!(text.contains("message: index out of bounds") && text.contains("turn.rs:42"));
        assert!(text.starts_with("AgentKnife "));
        for at in 0..25u64 {
            write(dir.path(), &text, 1_000 + at).unwrap_or_else(|e| panic!("{e}"));
        }
        // Same second twice: both kept.
        write(dir.path(), "second", 1_024).unwrap_or_else(|e| panic!("{e}"));
        let listed = list(dir.path());
        assert_eq!(listed.reports.len(), KEEP);
        assert_eq!(listed.reports[0].created_at, 1_024);
        assert!(listed.reports.iter().all(|r| r.created_at >= 1_006));
        assert_eq!(list(&dir.path().join("none")).reports.len(), 0);
    }
}
