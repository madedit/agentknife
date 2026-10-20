//! Live check of the preview manager with the real toolchain: scaffolds a new
//! project (or uses the folder given), starts the preview, breaks and fixes the
//! screens, changes the backend, and prints every state change. Manual only.
//!
//! ```text
//! cargo run -p ak-preview --example preview_live            # new temp project
//! cargo run -p ak-preview --example preview_live -- <dir>   # existing project
//! # AK_PREVIEW_HOLD=60 keeps it running 60 s at the end (to open the page)
//! ```

use std::error::Error;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ak_doctor::{Pinned, Toolchain};
use ak_preview::{Preview, Tools, profiles};
use ak_project::Registry;
use ak_types::i18n::Locale;
use ak_types::preview::{PreviewStatus, ServerState};
use tokio::sync::watch;

fn short(s: &ServerState) -> String {
    match s {
        ServerState::Error { output } => {
            let first = output
                .lines()
                .find(|l| l.contains("error"))
                .unwrap_or_default();
            format!("error ({first})")
        }
        other => format!("{other:?}").to_lowercase(),
    }
}

async fn wait(
    rx: &mut watch::Receiver<PreviewStatus>,
    what: &str,
    ok: impl Fn(&PreviewStatus) -> bool,
) -> Result<PreviewStatus, Box<dyn Error>> {
    let start = Instant::now();
    loop {
        let st = rx.borrow_and_update().clone();
        if ok(&st) {
            println!("   ✓ {what} after {:.0?}", start.elapsed());
            return Ok(st);
        }
        if tokio::time::timeout(Duration::from_secs(600), rx.changed())
            .await
            .is_err()
        {
            return Err(format!("timed out waiting for {what}").into());
        }
        let st = rx.borrow().clone();
        println!(
            "   frontend={} backend={}",
            short(&st.frontend),
            short(&st.backend)
        );
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let profile = profiles::default_profile();
    let home = tempfile::tempdir()?;
    let root: PathBuf = match std::env::args().nth(1) {
        Some(dir) => dir.into(),
        None => {
            let mut registry = Registry::load(&home.path().join("state"), home.path())?;
            let mut project = registry.create("Preview Check", None, Locale::En)?;
            project.scaffold(profile)?;
            project.root().to_path_buf()
        }
    };
    println!("project: {}", root.display());
    let tc = Toolchain::new(Toolchain::default_root(), &Pinned::builtin()?);
    let mut preview = Preview::new(&root, profile.preview(), Tools::from_toolchain(&tc))?;
    let mut rx = preview.subscribe();

    println!("== start (first build compiles everything; may take minutes)");
    preview.start()?;
    let st = wait(&mut rx, "ready", PreviewStatus::ready).await?;
    println!("   page: {}", st.url.unwrap_or_default());

    println!("== break the screens");
    let main = root.join("src/main.rs");
    let original = std::fs::read_to_string(&main)?;
    std::fs::write(
        &main,
        format!("{original}\nfn broken() -> u32 {{ \"text\" }}\n"),
    )?;
    let st = wait(&mut rx, "frontend error", |s| s.frontend.is_error()).await?;
    if let ServerState::Error { output } = &st.frontend {
        let errors: Vec<&str> = output.lines().filter(|l| l.starts_with("error")).collect();
        println!("   compiler said: {errors:?}");
    }

    println!("== fix the screens");
    std::fs::write(&main, &original)?;
    wait(&mut rx, "ready again", PreviewStatus::ready).await?;

    println!("== change the backend");
    let backend = root.join("src-tauri/src/main.rs");
    let code = std::fs::read_to_string(&backend)?;
    std::fs::write(&backend, code.replace("Hello, {name}!", "Hi, {name}!"))?;
    let restarted = preview
        .files_changed(&["src-tauri/src/main.rs".into()])
        .await?;
    println!("   backend restarted: {restarted}");
    wait(&mut rx, "backend rebuilt and ready", PreviewStatus::ready).await?;

    if let Some(secs) = std::env::var("AK_PREVIEW_HOLD")
        .ok()
        .and_then(|s| s.parse().ok())
    {
        println!("== holding {secs} s: {}", preview.page_url());
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
    println!("== stop");
    preview.stop().await;
    println!("   {:?}", preview.status());
    Ok(())
}
