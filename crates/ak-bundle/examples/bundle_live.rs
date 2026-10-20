//! Live check of bundling with the real toolchain: scaffolds a new project (or
//! uses the folder given), builds it for this computer and prints the result.
//! Manual only (the first release build takes minutes).
//!
//! ```text
//! cargo run -p ak-bundle --example bundle_live            # new project, kept
//! cargo run -p ak-bundle --example bundle_live -- <dir>   # existing project
//! ```

use std::error::Error;
use std::path::PathBuf;
use std::time::Instant;

use ak_bundle::{BundleEvent, Bundler};
use ak_doctor::{Pinned, Toolchain};
use ak_preview::{Tools, profiles};
use ak_project::Registry;
use ak_types::i18n::{I18n, Locale};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let profile = profiles::default_profile();
    let root: PathBuf = match std::env::args().nth(1) {
        Some(dir) => dir.into(),
        None => {
            // Kept, so the result can be unpacked and run afterwards.
            let home = tempfile::tempdir()?.keep();
            let mut registry = Registry::load(&home.join("state"), &home)?;
            let mut project = registry.create("Bundle Check", None, Locale::En)?;
            project.scaffold(profile)?;
            project.root().to_path_buf()
        }
    };
    println!("project: {}", root.display());
    let tc = Toolchain::new(Toolchain::default_root(), &Pinned::builtin()?);
    let bundler = Bundler::new(&root, profile.bundle(), Tools::from_toolchain(&tc));
    let i18n = I18n::new(Locale::ZhTw)?;

    let (tx, mut rx) = mpsc::unbounded_channel();
    let printer = tokio::spawn(async move {
        let mut lines = 0usize;
        while let Some(e) = rx.recv().await {
            match e {
                BundleEvent::Stage(s) => println!("== {}", i18n.t(s.message_id())),
                BundleEvent::Output(line) => {
                    lines += 1;
                    if line.contains("error")
                        || line.contains("Finished")
                        || lines.is_multiple_of(50)
                    {
                        println!("   {line}");
                    }
                }
            }
        }
    });
    let start = Instant::now();
    let result = bundler.build(&tx, &CancellationToken::new()).await;
    drop(tx);
    printer.await?;
    match result {
        Ok(b) => {
            let size = std::fs::metadata(&b.path)?.len();
            println!(
                "done in {:.0?}: {} ({:.1} MB), checkpoint {:?}",
                start.elapsed(),
                b.path.display(),
                size as f64 / 1e6,
                b.checkpoint
            );
        }
        Err(e) => {
            if let ak_bundle::BundleError::BuildFailed { output } = &e {
                println!("{output}");
            }
            return Err(e.into());
        }
    }
    Ok(())
}
