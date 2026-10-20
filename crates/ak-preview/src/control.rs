//! The agent's side of the bridge's control channel: send an action to the open
//! preview page, get its answer.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ak_types::preview::ControlAction;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub struct Controller {
    url: String,
    conn: tokio::sync::Mutex<Option<Ws>>,
    next_id: AtomicU64,
}

impl Controller {
    /// `url`: `Preview::controller_url()` (token and controller role included).
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            conn: tokio::sync::Mutex::new(None),
            next_id: AtomicU64::new(1),
        }
    }

    /// Runs `action` in the newest open preview page. Reconnects if the backend
    /// restarted; never sends an action twice.
    pub async fn act(&self, action: &ControlAction, timeout: Duration) -> Result<Value, String> {
        let mut conn = self.conn.lock().await;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let text = json!({ "type": "control", "id": id, "action": action }).to_string();
        // A dead connection is only found when sending: reconnect once then.
        let mut sent = false;
        for _ in 0..2 {
            if conn.is_none() {
                let (ws, _) = tokio_tungstenite::connect_async(self.url.as_str())
                    .await
                    .map_err(|_| "The app's backend is not running (the preview is starting or failed to build).".to_owned())?;
                *conn = Some(ws);
            }
            let Some(ws) = conn.as_mut() else { continue };
            if ws.send(Message::text(text.clone())).await.is_ok() {
                sent = true;
                break;
            }
            *conn = None;
        }
        if !sent {
            return Err("Could not reach the preview.".into());
        }
        let Some(ws) = conn.as_mut() else {
            return Err("Could not reach the preview.".into());
        };
        let answer = tokio::time::timeout(timeout, async {
            while let Some(msg) = ws.next().await {
                let Ok(Message::Text(text)) = msg else {
                    if msg.is_err() {
                        break;
                    }
                    continue;
                };
                let Ok(v) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                if v["type"] == "control_result" && v["id"] == id {
                    let value = v["value"].clone();
                    return Some(if v["ok"] == true {
                        Ok(value)
                    } else {
                        Err(value
                            .as_str()
                            .map_or_else(|| value.to_string(), str::to_owned))
                    });
                }
            }
            None
        })
        .await;
        match answer {
            Ok(Some(result)) => result,
            Ok(None) => {
                *conn = None;
                Err("The preview disconnected before answering (the app restarted?).".into())
            }
            Err(_) => Err("The preview page did not answer in time.".into()),
        }
    }
}
