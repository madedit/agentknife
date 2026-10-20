//! Small file helpers.

use std::path::Path;

use crate::error::{IoContext, ProjectError};

/// Write-then-rename, so a crash never leaves a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ProjectError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).at(dir)?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    std::fs::write(&tmp, bytes).at(&tmp)?;
    std::fs::rename(&tmp, path).at(path)
}

/// Writes `contents` unless the file already exists (templates never overwrite).
pub fn write_if_missing(path: &Path, contents: &str) -> Result<(), ProjectError> {
    if path.exists() {
        return Ok(());
    }
    write_atomic(path, contents.as_bytes())
}

/// Turns a project name into a safe folder name: keeps letters of any script,
/// replaces characters Windows forbids, avoids reserved device names.
pub fn folder_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches(|c: char| c == '.' || c == ' ' || c == '-');
    let cleaned: String = cleaned.chars().take(80).collect();
    const RESERVED: &[&str] = &[
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    let stem = cleaned.split('.').next().unwrap_or("").to_ascii_lowercase();
    if cleaned.is_empty() || RESERVED.contains(&stem.as_str()) {
        format!("project-{cleaned}")
            .trim_end_matches('-')
            .to_owned()
    } else {
        cleaned
    }
}

/// `parent/base`, or `parent/base 2`, `parent/base 3`… if taken.
pub fn unique_dir(parent: &Path, base: &str) -> std::path::PathBuf {
    let first = parent.join(base);
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| parent.join(format!("{base} {n}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names() {
        assert_eq!(folder_name("我的待辦清單"), "我的待辦清單");
        assert_eq!(folder_name("  Todo: v2 / final?  "), "Todo- v2 - final");
        assert_eq!(folder_name("CON"), "project-CON");
        assert_eq!(folder_name("nul.txt"), "project-nul.txt");
        assert_eq!(folder_name("..."), "project");
        assert_eq!(folder_name("trailing dot."), "trailing dot");
    }

    #[test]
    fn unique_dirs_and_atomic_writes() {
        let dir = tempfile::tempdir().unwrap();
        let a = unique_dir(dir.path(), "App");
        std::fs::create_dir(&a).unwrap();
        assert_eq!(unique_dir(dir.path(), "App"), dir.path().join("App 2"));

        let f = dir.path().join("x/conversation.json");
        write_atomic(&f, b"1").unwrap();
        write_atomic(&f, b"2").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "2");
        write_if_missing(&f, "3").unwrap();
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            "2",
            "never overwrites"
        );
        assert_eq!(
            std::fs::read_dir(dir.path().join("x")).unwrap().count(),
            1,
            "no tmp left"
        );
    }
}
