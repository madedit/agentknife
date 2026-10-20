//! No Node.js anywhere in the repo (AGENTS.md §2).

use std::path::Path;

use crate::Result;

const FORBIDDEN: &[&str] = &[
    "package.json",
    "package-lock.json",
    "node_modules",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lockb",
];
const SKIP_DIRS: &[&str] = &["target", ".git"];

pub fn check(root: &Path) -> Result<()> {
    let mut found = Vec::new();
    scan(root, root, &mut found)?;
    if found.is_empty() {
        Ok(())
    } else {
        Err(format!("Node.js artifacts are not allowed: {}", found.join(", ")).into())
    }
}

fn scan(root: &Path, dir: &Path, found: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if FORBIDDEN.contains(&name.as_str()) {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            found.push(rel.display().to_string());
        } else if path.is_dir() && !SKIP_DIRS.contains(&name.as_str()) {
            scan(root, &path, found)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_tree_passes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        // Anything under target/ is ignored.
        std::fs::create_dir_all(dir.path().join("target/node_modules")).unwrap();
        assert!(check(dir.path()).is_ok());
    }

    #[test]
    fn nested_package_json_fails() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("crates/x")).unwrap();
        std::fs::write(dir.path().join("crates/x/package.json"), "{}").unwrap();
        assert!(check(dir.path()).is_err());
    }

    #[test]
    fn node_modules_dir_fails() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules")).unwrap();
        assert!(check(dir.path()).is_err());
    }
}
