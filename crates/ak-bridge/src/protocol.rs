//! What the preview frontend and the bridge server say to each other, and how
//! AgentKnife starts the backend. Shared by both sides; JSON text frames.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Port to bind on 127.0.0.1; `0` picks a free one.
pub const ENV_PORT: &str = "AK_BRIDGE_PORT";
/// The secret every connection must present as `?token=`.
pub const ENV_TOKEN: &str = "AK_BRIDGE_TOKEN";
/// When set, connections must come from this origin (the preview's dev server).
pub const ENV_ORIGIN: &str = "AK_BRIDGE_ORIGIN";
/// Printed on stdout once the server listens: `AK_BRIDGE_READY {"port":1234}`.
pub const READY_PREFIX: &str = "AK_BRIDGE_READY ";
/// Preview URL parameter with the full WebSocket URL (token included).
pub const URL_PARAM: &str = "ak_bridge";
/// Where the frontend keeps that URL for the rest of the session.
pub const STORAGE_KEY: &str = "ak_bridge";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Call a Tauri command, like `invoke(cmd, args)` in the real app.
    Invoke {
        id: u64,
        cmd: String,
        #[serde(default)]
        args: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// The answer to the `Invoke` with the same id: the command's return value,
    /// or (`ok: false`) its error value.
    Result { id: u64, ok: bool, value: Value },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ready {
    pub port: u16,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn wire_format() {
        let m: ClientMessage = serde_json::from_str(
            r#"{"type":"invoke","id":7,"cmd":"add_todo","args":{"text":"milk"}}"#,
        )
        .unwrap();
        assert_eq!(
            m,
            ClientMessage::Invoke {
                id: 7,
                cmd: "add_todo".into(),
                args: json!({ "text": "milk" })
            }
        );
        let no_args: ClientMessage =
            serde_json::from_str(r#"{"type":"invoke","id":1,"cmd":"list"}"#).unwrap();
        assert!(matches!(no_args, ClientMessage::Invoke { args, .. } if args.is_null()));
        assert_eq!(
            serde_json::to_value(ServerMessage::Result {
                id: 7,
                ok: false,
                value: json!("empty text")
            })
            .unwrap(),
            json!({ "type": "result", "id": 7, "ok": false, "value": "empty text" })
        );
    }
}
