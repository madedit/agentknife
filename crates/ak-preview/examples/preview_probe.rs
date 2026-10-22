//! Starts the preview of an existing project, prints the build's progress (as
//! the build window gets it) and then what the agent's `dom_summary` would
//! see, every few seconds, while you use the page in a browser (the URL is
//! printed). Manual only; changes no files.
//!
//! ```text
//! cargo run -p ak-preview --example preview_probe -- <dir> [seconds=120]
//! ```

use std::error::Error;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ak_doctor::{Pinned, Toolchain};
use ak_preview::{Preview, PreviewSession, Tools, profiles};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let root: PathBuf = std::env::args()
        .nth(1)
        .ok_or("usage: preview_probe <dir> [seconds]")?
        .into();
    let secs: u64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(120);
    let tc = Toolchain::new(Toolchain::default_root(), &Pinned::builtin()?);
    let preview = Preview::new(
        &root,
        profiles::default_profile().preview(),
        Tools::from_toolchain(&tc),
    )?;
    // Progress reports, at most one per 10% step and source.
    let mut log = preview.log().subscribe();
    tokio::spawn(async move {
        let mut shown = std::collections::HashMap::new();
        while let Some(line) = log.recv().await {
            let Some(p) = line.progress else {
                // What completes a part in the build window.
                if line.text.trim_start().starts_with("Finished ") {
                    println!("finished {:?}: {}", line.source, line.text.trim());
                }
                continue;
            };
            let step = p.percent() / 10;
            if shown.insert(line.source, step) != Some(step) {
                println!(
                    "progress {:?}: {}/{} {}",
                    line.source, p.done, p.total, line.text
                );
            }
        }
    });
    let session = PreviewSession::new(preview, None);
    session.start().await?;
    let st = session.settled(Duration::from_secs(600)).await;
    println!(
        "status: ready={} page: {}",
        st.ready(),
        session.page_url().await
    );

    // `AK_PROBE_ACTIONS`: a JSON list of control actions to run first, each
    // answer printed with the app's state after it.
    if let Ok(list) = std::env::var("AK_PROBE_ACTIONS") {
        let actions: Vec<ak_types::preview::ControlAction> = serde_json::from_str(&list)?;
        for a in actions {
            println!("---- {a:?}\n{:?}", session.act(&a).await);
            println!("state: {:?}", session.app_state().await.map(|s| s.app));
        }
    }

    let end = Instant::now() + Duration::from_secs(secs);
    let mut last = String::new();
    while Instant::now() < end {
        let now = match session.dom_summary().await {
            Ok(s) => s.render(),
            Err(e) => format!("(no page: {e})\n"),
        };
        if now != last {
            println!("---- dom_summary\n{now}");
            last = now;
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    session.stop().await;
    Ok(())
}
