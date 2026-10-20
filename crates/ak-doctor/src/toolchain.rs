//! Where AgentKnife keeps its own tools, and the pinned versions it wants.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The repo's `versions.toml`, built in: the single source of pinned versions.
const VERSIONS_TOML: &str = include_str!("../../../versions.toml");

/// Versions the doctor installs and checks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pinned {
    pub rust: String,
    pub trunk: String,
    pub wasm_bindgen: String,
    pub tauri_cli: String,
    pub tailwind: String,
}

#[derive(Deserialize)]
struct VersionsFile {
    rust: RustSection,
    crates: CratesSection,
    tools: ToolsSection,
}

#[derive(Deserialize)]
struct RustSection {
    toolchain: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct CratesSection {
    wasm_bindgen: String,
    tauri_cli: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct ToolsSection {
    trunk: String,
    tailwind_cli: String,
}

impl Pinned {
    /// The versions this build of AgentKnife was made with.
    pub fn builtin() -> Result<Pinned, String> {
        let v: VersionsFile = toml::from_str(VERSIONS_TOML).map_err(|e| e.to_string())?;
        Ok(Pinned {
            rust: v.rust.toolchain,
            trunk: v.tools.trunk,
            wasm_bindgen: v.crates.wasm_bindgen,
            tauri_cli: v.crates.tauri_cli,
            tailwind: v.tools.tailwind_cli,
        })
    }
}

/// AgentKnife's private toolchain folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toolchain {
    root: PathBuf,
    rust: String,
}

impl Toolchain {
    pub fn new(root: impl Into<PathBuf>, pinned: &Pinned) -> Self {
        Self {
            root: root.into(),
            rust: pinned.rust.clone(),
        }
    }

    /// `<data_dir>/AgentKnife/toolchain`.
    pub fn default_root() -> PathBuf {
        dirs::data_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("AgentKnife")
            .join("toolchain")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn rustup_home(&self) -> PathBuf {
        self.root.join("rustup")
    }

    pub fn cargo_home(&self) -> PathBuf {
        self.root.join("cargo")
    }

    /// Where cargo-installed tools and the rustup proxies live.
    pub fn cargo_bin(&self) -> PathBuf {
        self.cargo_home().join("bin")
    }

    /// Standalone binaries (Tailwind CLI).
    pub fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    /// Full path of an executable in the toolchain.
    pub fn exe(&self, dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
    }

    /// Folders to put first on PATH for commands that use the toolchain.
    pub fn path_prefix(&self) -> Vec<PathBuf> {
        vec![self.cargo_bin(), self.bin()]
    }

    /// Environment for anything that runs rustup or cargo. Auto-install is OFF:
    /// rustup otherwise installs a missing toolchain as a side effect.
    pub fn env(&self) -> Vec<(String, String)> {
        vec![
            (
                "RUSTUP_HOME".into(),
                self.rustup_home().to_string_lossy().into_owned(),
            ),
            (
                "CARGO_HOME".into(),
                self.cargo_home().to_string_lossy().into_owned(),
            ),
            ("RUSTUP_TOOLCHAIN".into(), self.rust.clone()),
            ("RUSTUP_AUTO_INSTALL".into(), "0".into()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_versions_match_versions_toml() {
        let p = Pinned::builtin().unwrap();
        assert!(p.rust.starts_with("1."));
        assert!(!p.trunk.is_empty() && !p.tailwind.is_empty());
        let t = Toolchain::new("/ak/toolchain", &p);
        let env = t.env();
        assert!(env.contains(&("RUSTUP_AUTO_INSTALL".into(), "0".into())));
        assert!(env.contains(&("RUSTUP_TOOLCHAIN".into(), p.rust.clone())));
        assert_eq!(
            t.path_prefix()[0],
            PathBuf::from("/ak/toolchain").join("cargo").join("bin")
        );
    }
}
