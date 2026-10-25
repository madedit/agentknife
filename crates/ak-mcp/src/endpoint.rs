//! `<data_dir>/AgentKnife/mcp.json`: where the running AgentKnife listens. It
//! exists while AgentKnife runs; without the setting on it has no port.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    /// "Let other AI tools operate AgentKnife" is on.
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// 128 random bits, hex. Never shown to a model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

impl Endpoint {
    /// The file in the usual place (`None` without a data folder).
    pub fn default_path() -> Option<PathBuf> {
        Some(dirs::data_dir()?.join("AgentKnife").join("mcp.json"))
    }

    pub fn read(path: &Path) -> Option<Endpoint> {
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }

    /// Writes it readable by this user only (a temporary file, then renamed).
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, path)
    }
}

/// 128 random bits, hex.
pub fn random_token() -> std::io::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_off_has_no_port() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("AgentKnife").join("mcp.json");
        let on = Endpoint {
            enabled: true,
            port: Some(4321),
            token: Some(random_token().unwrap_or_default()),
        };
        on.write(&path).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(Endpoint::read(&path), Some(on));
        let off = Endpoint {
            enabled: false,
            port: None,
            token: None,
        };
        off.write(&path).unwrap_or_else(|e| panic!("{e}"));
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(!text.contains("port") && !text.contains("token"), "{text}");
        assert_eq!(random_token().map(|t| t.len()).ok(), Some(32));
    }
}
