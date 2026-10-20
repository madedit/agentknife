//! Test helper for tests/bundle.rs: behaves like `cargo tauri build` in the
//! current folder, driven by FAKE_BUILD (`ok`, `fail`, `leak`, `none`, `slow`).
//! Not used by AgentKnife itself.

#![forbid(unsafe_code)]

use std::time::Duration;

fn main() {
    let mode = std::env::var("FAKE_BUILD").unwrap_or_default();
    println!("   Compiling app v0.1.0");
    eprintln!("\u{1b}[32m    Building\u{1b}[0m [=====>   ] 5/9");
    match mode.as_str() {
        "fail" => {
            eprintln!("error[E0308]: mismatched types");
            eprintln!(" --> src-tauri/src/main.rs:9:5");
            std::process::exit(101);
        }
        "slow" => std::thread::sleep(Duration::from_secs(60)),
        _ => {}
    }
    if mode != "none" {
        let program = format!("target/release/app{}", std::env::consts::EXE_SUFFIX);
        std::fs::create_dir_all("target/release").expect("target dir");
        let body: &[u8] = if mode == "leak" {
            b"\x7fELF...AK_BRIDGE_READY {}..."
        } else {
            b"\x7fELF...the app..."
        };
        std::fs::write(program, body).expect("program");
        std::fs::create_dir_all("dist").expect("dist");
        std::fs::write("dist/index.html", "<html></html>").expect("dist file");
    }
    println!("    Finished `release` profile [optimized] target(s) in 1.00s");
}
