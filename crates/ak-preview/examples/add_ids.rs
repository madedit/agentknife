//! Adds missing `data-ak-id`s to Leptos files, in place, and lists them.
//!
//! ```text
//! cargo run -p ak-preview --example add_ids -- <file.rs>…
//! ```

use std::error::Error;

use ak_preview::ids::add_missing_ids;

fn main() -> Result<(), Box<dyn Error>> {
    for path in std::env::args().skip(1) {
        let source = std::fs::read_to_string(&path)?;
        let stem = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (out, added) = add_missing_ids(&source, &stem)?;
        for a in &added {
            println!("{path}:{} <{}> data-ak-id=\"{}\"", a.line, a.tag, a.id);
        }
        if !added.is_empty() {
            std::fs::write(&path, out)?;
        }
        println!("{path}: {} added", added.len());
    }
    Ok(())
}
