//! The agent's control client against the real bridge server (mock-runtime app)
//! with a fake preview page; and the ak-types copies of the bridge protocol.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use ak_bridge::protocol as bridge;
use ak_preview::control::Controller;
use ak_types::preview as ours;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tokio_tungstenite::tungstenite::Message;

const TOKEN: &str = "0123456789abcdef0123";

fn same_json<A: serde::Serialize, B: serde::Serialize + serde::de::DeserializeOwned>(a: &A) -> B {
    let v = serde_json::to_value(a).unwrap();
    let b: B = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(&b).unwrap(),
        v,
        "round trip changed the JSON"
    );
    b
}

#[test]
fn protocol_copies_match_ak_bridge() {
    assert_eq!(ours::PAGE_NOT_OPEN, bridge::PAGE_NOT_OPEN);
    assert_eq!(ours::PAGE_CLOSED, bridge::PAGE_CLOSED);
    let actions = [
        bridge::ControlAction::Click { ak_id: "a".into() },
        bridge::ControlAction::Type {
            ak_id: "b".into(),
            text: "t".into(),
            enter: true,
        },
        bridge::ControlAction::DomSummary,
        bridge::ControlAction::AppState,
        bridge::ControlAction::PrepareScreenshot { ak_id: None },
    ];
    for a in &actions {
        let b: ours::ControlAction = same_json(a);
        let _: bridge::ControlAction = same_json(&b);
    }
    let summary = bridge::DomSummary {
        title: "T".into(),
        url: "u".into(),
        items: vec![bridge::DomItem {
            depth: 1,
            tag: "input".into(),
            ak_id: Some("x".into()),
            attrs: vec![("type".into(), "text".into())],
            text: Some("t".into()),
            value: Some("v".into()),
            checked: Some(true),
            disabled: true,
        }],
        omitted: 2,
    };
    let copy: ours::DomSummary = same_json(&summary);
    assert_eq!(copy.render(), bridge::render_dom_summary(&summary));
    let rect = bridge::Rect {
        x: 1.0,
        y: 2.0,
        width: 3.0,
        height: 4.0,
    };
    let size = bridge::Size {
        width: 5.0,
        height: 6.0,
    };
    let _: ours::ScreenshotArea = same_json(&bridge::ScreenshotArea {
        rect,
        viewport: size,
        device_pixel_ratio: 2.0,
    });
    let picked = bridge::Picked {
        ak_id: Some("p".into()),
        exact: false,
        path: "main > p".into(),
        tag: "p".into(),
        text: None,
        rect,
        viewport: size,
        canvas: Some(bridge::CanvasPoint {
            x: 1.0,
            y: 2.0,
            hit: Some(json!({ "shape": "a" })),
        }),
    };
    let _: ours::Picked = same_json(&picked);
    // Point-and-select messages, both ways.
    assert_eq!(ours::INSPECTOR_KEY, bridge::INSPECTOR_KEY);
    for (b, o) in [
        (
            bridge::InspectorCommand::Enable,
            ours::InspectorCommand::Enable,
        ),
        (
            bridge::InspectorCommand::Disable,
            ours::InspectorCommand::Disable,
        ),
    ] {
        assert_eq!(
            serde_json::to_value(&b).unwrap(),
            serde_json::to_value(o).unwrap()
        );
    }
    for event in [
        bridge::InspectorEvent::Ready,
        bridge::InspectorEvent::Hover {
            target: picked.clone(),
        },
        bridge::InspectorEvent::Select { target: picked },
        bridge::InspectorEvent::Cancel,
    ] {
        let _: ours::InspectorEvent = same_json(&event);
    }
    let mut storage = serde_json::Map::new();
    storage.insert("k".into(), json!([1]));
    let _: ours::AppState = same_json(&bridge::AppState {
        url: "u".into(),
        title: "t".into(),
        local_storage: storage,
        app: Some(json!({ "n": 1 })),
    });
}

/// A preview page: answers every control action with what it was asked.
async fn fake_page(port: u16, answer: fn(&Value) -> (bool, Value)) {
    let url = format!("ws://127.0.0.1:{port}/?token={TOKEN}");
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    while let Some(Ok(Message::Text(text))) = ws.next().await {
        let v: Value = serde_json::from_str(&text).unwrap();
        if v["type"] == "control" {
            let (ok, value) = answer(&v["action"]);
            let reply =
                json!({ "type": "control_result", "id": v["id"], "ok": ok, "value": value });
            ws.send(Message::text(reply.to_string())).await.unwrap();
        }
    }
}

fn answer(action: &Value) -> (bool, Value) {
    match action["action"].as_str() {
        Some("click") if action["ak_id"] == "missing" => {
            (false, json!("no element with data-ak-id `missing`"))
        }
        Some("click") => (true, json!({ "clicked": action["ak_id"], "tag": "button" })),
        Some("dom_summary") => (
            true,
            json!({ "title": "My App", "url": "http://x/", "items": [
                { "depth": 0, "tag": "button", "ak_id": "greet-button", "text": "Greet" }
            ]}),
        ),
        _ => (false, json!("unsupported")),
    }
}

#[test]
fn controller_drives_the_page_through_the_bridge() {
    let app = mock_builder().build(mock_context(noop_assets())).unwrap();
    tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let addr = ak_bridge::server::start(
        app.handle().clone(),
        ak_bridge::server::Config {
            port: 0,
            token: TOKEN.into(),
            origin: None,
        },
    )
    .unwrap();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let url = format!(
            "ws://127.0.0.1:{}/?token={TOKEN}&role=controller",
            addr.port()
        );
        let controller = Controller::new(url);
        let t = Duration::from_secs(5);

        // No page open yet: a clear error, no hang.
        let err = controller
            .act(&ours::ControlAction::DomSummary, t)
            .await
            .unwrap_err();
        assert!(err.contains("not open"), "{err}");

        tokio::spawn(fake_page(addr.port(), answer));
        tokio::time::sleep(Duration::from_millis(300)).await;
        let v = controller
            .act(
                &ours::ControlAction::Click {
                    ak_id: "greet-button".into(),
                },
                t,
            )
            .await
            .unwrap();
        assert_eq!(v["clicked"], "greet-button");
        let err = controller
            .act(
                &ours::ControlAction::Click {
                    ak_id: "missing".into(),
                },
                t,
            )
            .await
            .unwrap_err();
        assert!(err.contains("no element"), "{err}");
        let v = controller
            .act(&ours::ControlAction::DomSummary, t)
            .await
            .unwrap();
        let summary: ours::DomSummary = serde_json::from_value(v).unwrap();
        assert!(summary.render().contains("button #greet-button \"Greet\""));
    });
}

#[tokio::test]
async fn controller_reports_a_missing_backend() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let controller = Controller::new(format!("ws://127.0.0.1:{port}/?token=x&role=controller"));
    let err = controller
        .act(&ours::ControlAction::DomSummary, Duration::from_secs(2))
        .await
        .unwrap_err();
    assert!(err.contains("backend is not running"), "{err}");
}
