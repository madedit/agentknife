//! Project-relative paths. Every tool resolves paths here, so nothing outside the
//! project can be read or written, whatever the permission policy says.

use std::path::{Component, Path, PathBuf};

const OUTSIDE: &str = "That path is outside the project. Use a path inside it, like `src/app.rs`.";

/// Resolves `rel` inside `root`. Rejects absolute paths, `..` that leave the
/// project, and symlinks whose target is outside it.
pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim();
    let p = Path::new(if rel.is_empty() { "." } else { rel });
    let mut parts = Vec::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err(OUTSIDE.into());
                }
            }
            Component::Normal(s) => parts.push(s),
            Component::RootDir | Component::Prefix(_) => return Err(OUTSIDE.into()),
        }
    }
    let mut full = root.to_path_buf();
    full.extend(parts);

    // Symlinks: the deepest existing ancestor must really live inside the project.
    let root_real = root
        .canonicalize()
        .map_err(|e| format!("The project folder is not accessible: {e}"))?;
    let mut probe = full.clone();
    while !probe.exists() {
        if !probe.pop() {
            break;
        }
    }
    let real = probe
        .canonicalize()
        .map_err(|e| format!("Cannot access that path: {e}"))?;
    if !real.starts_with(&root_real) {
        return Err(OUTSIDE.into());
    }
    Ok(full)
}

/// Project-relative display form with `/` separators.
pub fn display(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let s: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if s.is_empty() {
        ".".into()
    } else {
        s.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_paths_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(
            resolve(root, "src/app.rs").unwrap(),
            root.join("src").join("app.rs")
        );
        assert_eq!(
            resolve(root, "./src/../README.md").unwrap(),
            root.join("README.md")
        );
        assert_eq!(resolve(root, "").unwrap(), root.to_path_buf());
        assert_eq!(
            display(root, &root.join("src").join("app.rs")),
            "src/app.rs"
        );
        assert_eq!(display(root, root), ".");
    }

    #[test]
    fn escapes_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for bad in [
            "../secret.txt",
            "src/../../x",
            "/etc/passwd",
            "\\\\server\\share\\x",
        ] {
            assert!(resolve(root, bad).is_err(), "{bad}");
        }
        #[cfg(windows)]
        for bad in ["C:\\Windows\\win.ini", "C:relative.txt"] {
            assert!(resolve(root, bad).is_err(), "{bad}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_out_of_project_is_rejected() {
        let outside = tempfile::tempdir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        assert!(resolve(dir.path(), "link/file.txt").is_err());
    }
}
