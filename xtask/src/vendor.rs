//! Vendored bundle integrity: every file under a vendor dir must match its SHA256SUMS.

use std::collections::BTreeMap;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::Result;

/// Vendor directories, relative to the workspace root.
const VENDOR_DIRS: &[&str] = &["crates/ak-shell/vendor/codemirror"];
const SUMS_FILE: &str = "SHA256SUMS";

pub fn check(root: &Path) -> Result<()> {
    let mut errors = Vec::new();
    for dir in VENDOR_DIRS {
        let dir = root.join(dir);
        // Absent until P1-CORE-004 vendors it.
        if dir.is_dir() {
            errors.extend(verify_dir(&dir)?);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n").into())
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Parses `<hex>  <relative/path>` lines.
fn parse_sums(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|l| l.split_once("  "))
        .map(|(hash, path)| (path.trim().to_owned(), hash.trim().to_owned()))
        .collect()
}

fn verify_dir(dir: &Path) -> Result<Vec<String>> {
    let label = dir.display();
    let Ok(text) = std::fs::read_to_string(dir.join(SUMS_FILE)) else {
        return Ok(vec![format!("{label}: missing {SUMS_FILE}")]);
    };
    let mut expected = parse_sums(&text);
    let mut errors = Vec::new();
    for rel in list_files(dir, dir)? {
        if rel == SUMS_FILE {
            continue;
        }
        let actual = sha256_hex(&std::fs::read(dir.join(&rel))?);
        match expected.remove(&rel) {
            Some(hash) if hash == actual => {}
            Some(_) => errors.push(format!("{label}/{rel}: hash mismatch")),
            None => errors.push(format!("{label}/{rel}: not listed in {SUMS_FILE}")),
        }
    }
    for rel in expected.keys() {
        errors.push(format!("{label}/{rel}: listed in {SUMS_FILE} but missing"));
    }
    Ok(errors)
}

/// Lists files under `dir` as `/`-separated paths relative to `base`.
fn list_files(base: &Path, dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            out.extend(list_files(base, &path)?);
        } else if let Ok(rel) = path.strip_prefix(base) {
            let parts: Vec<_> = rel.iter().map(|p| p.to_string_lossy()).collect();
            out.push(parts.join("/"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vendor_dir(files: &[(&str, &str)], sums: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        std::fs::write(dir.path().join(SUMS_FILE), sums).unwrap();
        dir
    }

    #[test]
    fn matching_hashes_pass() {
        let sums = format!(
            "{}  a.js\n{}  sub/b.js\n",
            sha256_hex(b"aaa"),
            sha256_hex(b"bbb")
        );
        let dir = vendor_dir(&[("a.js", "aaa"), ("sub/b.js", "bbb")], &sums);
        assert!(verify_dir(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn changed_byte_fails() {
        let sums = format!("{}  a.js\n", sha256_hex(b"aaa"));
        let dir = vendor_dir(&[("a.js", "aab")], &sums);
        assert_eq!(verify_dir(dir.path()).unwrap().len(), 1);
    }

    #[test]
    fn unlisted_and_missing_files_fail() {
        let sums = format!("{}  gone.js\n", sha256_hex(b"x"));
        let dir = vendor_dir(&[("extra.js", "y")], &sums);
        assert_eq!(verify_dir(dir.path()).unwrap().len(), 2);
    }
}
