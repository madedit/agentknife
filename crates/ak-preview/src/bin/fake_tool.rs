//! Test helper for tests/dev_server.rs: behaves like `trunk serve` (first
//! argument `serve`) or like `cargo run` of the app backend (`run`), driven by
//! FAKE_* environment variables. Not used by AgentKnife itself.

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::time::Duration;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

fn log(text: &str) {
    let path = env("FAKE_LOG");
    if path.is_empty() {
        return;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{text}");
    }
}

fn build(outcome: &str) {
    println!("2026-10-20T00:00:00Z  INFO starting build");
    println!("   Compiling app-ui v0.1.0");
    if outcome == "fail" {
        eprintln!("error[E0308]: mismatched types");
        eprintln!("   --> src/main.rs:3:5");
        println!("2026-10-20T00:00:01Z ERROR error");
        println!("error from build pipeline");
    } else {
        println!("2026-10-20T00:00:01Z  INFO applying new distribution");
        println!("2026-10-20T00:00:01Z  INFO success");
    }
}

fn serve(args: &[String]) {
    let port = args
        .windows(2)
        .find(|w| w[0] == "--port")
        .and_then(|w| w[1].parse::<u16>().ok())
        .unwrap_or(0);
    log(&format!("serve port={port} args={}", args.join(" ")));
    // FAKE_LISTEN_DELAY (ms): like trunk, report the build before listening.
    let delay: u64 = env("FAKE_LISTEN_DELAY").parse().unwrap_or(0);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(delay));
        let Ok(listener) = std::net::TcpListener::bind(("127.0.0.1", port)) else {
            return;
        };
        for mut stream in listener.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok");
        }
    });
    build(&env("FAKE_FRONT"));
    // Rebuild whenever the trigger file changes: "ok", "fail" or "exit".
    let trigger = env("FAKE_TRIGGER");
    let mut last = std::fs::read_to_string(&trigger).unwrap_or_default();
    loop {
        std::thread::sleep(Duration::from_millis(30));
        let now = std::fs::read_to_string(&trigger).unwrap_or_default();
        if now != last {
            last = now.clone();
            match now.trim() {
                "exit" => std::process::exit(3),
                outcome => build(outcome),
            }
        }
    }
}

fn run() {
    log(&format!(
        "run port={} token={} origin={}",
        env("AK_BRIDGE_PORT"),
        env("AK_BRIDGE_TOKEN"),
        env("AK_BRIDGE_ORIGIN")
    ));
    log(&format!("config {}", env("TAURI_CONFIG")));
    println!("   Compiling app v0.1.0");
    if env("FAKE_BACK") == "fail" {
        eprintln!("error[E0425]: cannot find value `x` in this scope");
        eprintln!("error: could not compile `app` (bin \"app\") due to 1 previous error");
        std::process::exit(101);
    }
    println!("AK_BRIDGE_READY {{\"port\":{}}}", env("AK_BRIDGE_PORT"));
    if env("FAKE_BACK") == "crash" {
        std::thread::sleep(Duration::from_millis(200));
        eprintln!("thread 'main' panicked at src/main.rs:9:5: boom");
        std::process::exit(101);
    }
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("serve") => serve(&args),
        Some("run") => run(),
        _ => std::process::exit(2),
    }
}
