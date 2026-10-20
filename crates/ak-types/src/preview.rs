//! The live preview's state, as the UI and the agent see it.

use serde::{Deserialize, Serialize};

/// One preview process (the screens' dev server, or the app's backend).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ServerState {
    /// Not running (or not used by this stack).
    #[default]
    Off,
    /// Started, nothing built yet.
    Starting,
    /// Compiling (the first build, or after a change).
    Building,
    Ready,
    /// The build failed or the process stopped. `output` is the end of what it
    /// printed (compiler errors), for the agent to fix.
    Error {
        output: String,
    },
}

impl ServerState {
    pub fn is_ready(&self) -> bool {
        *self == ServerState::Ready
    }

    pub fn is_error(&self) -> bool {
        matches!(self, ServerState::Error { .. })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewStatus {
    pub frontend: ServerState,
    pub backend: ServerState,
    /// The page to show (with the bridge address), once the screens are built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl PreviewStatus {
    /// The app can be used: screens built, backend (if any) running.
    pub fn ready(&self) -> bool {
        self.frontend.is_ready() && matches!(self.backend, ServerState::Ready | ServerState::Off)
    }

    /// The first build error to show or fix.
    pub fn error(&self) -> Option<&str> {
        [&self.frontend, &self.backend]
            .into_iter()
            .find_map(|s| match s {
                ServerState::Error { output } => Some(output.as_str()),
                _ => None,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_and_errors() {
        let mut s = PreviewStatus {
            frontend: ServerState::Ready,
            ..PreviewStatus::default()
        };
        assert!(s.ready(), "no backend needed");
        s.backend = ServerState::Building;
        assert!(!s.ready());
        s.backend = ServerState::Error {
            output: "error[E0425]".into(),
        };
        assert_eq!(s.error(), Some("error[E0425]"));
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["frontend"]["state"], "ready");
        assert_eq!(json["backend"]["state"], "error");
    }
}
