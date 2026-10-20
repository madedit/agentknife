//! Stack-specific names stay inside their `StackProfile` implementation
//! (AGENTS.md §3): no other code under `crates/` may say `tauri-leptos`.

use std::path::Path;

use crate::Result;

const NAME: &str = "tauri-leptos";

/// Where the name may appear (paths relative to the repo, `/` separators).
const ALLOWED: &[&str] = &[
    "crates/ak-preview/src/profiles/tauri_leptos.rs",
    // Embeds templates/tauri-leptos.
    "crates/ak-preview/build.rs",
    // Tests of that profile.
    "crates/ak-preview/tests/",
];

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if name != "target" && name != "vendor" && name != "dist" {
                walk(&path, out)?;
            }
        } else if name.ends_with(".rs") || name.ends_with(".toml") {
            out.push(path);
        }
    }
    Ok(())
}

pub fn violations(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    walk(&root.join("crates"), &mut files)?;
    let mut out = Vec::new();
    for file in files {
        let rel = file
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if ALLOWED
            .iter()
            .any(|a| rel == *a || (a.ends_with('/') && rel.starts_with(a)))
        {
            continue;
        }
        let text = std::fs::read_to_string(&file)?;
        for (n, line) in text.lines().enumerate() {
            if line.contains(NAME) {
                out.push(format!(
                    "{rel}:{}: names the {NAME} stack; go through StackProfile instead",
                    n + 1
                ));
            }
        }
    }
    Ok(out)
}

pub fn check(root: &Path) -> Result<()> {
    let v = violations(root)?;
    if v.is_empty() {
        Ok(())
    } else {
        Err(v.join("\n").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_other_crates_but_not_the_profile() {
        let dir = tempfile::tempdir().unwrap();
        let write = |rel: &str, text: &str| {
            let p = dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        write(
            "crates/ak-preview/src/profiles/tauri_leptos.rs",
            "fn id() -> &str { \"tauri-leptos\" }",
        );
        write("crates/ak-preview/tests/x.rs", "// templates/tauri-leptos");
        write(
            "crates/ak-app/src/main.rs",
            "let p = by_id(\"tauri-leptos\");",
        );
        write(
            "crates/ak-preview/src/dev_server.rs",
            "// for tauri-leptos only",
        );
        let v = violations(dir.path()).unwrap();
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(v[0].starts_with("crates/ak-app/src/main.rs:1"));
        assert!(v[1].starts_with("crates/ak-preview/src/dev_server.rs:1"));
    }
}
