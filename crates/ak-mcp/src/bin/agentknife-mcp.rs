//! `agentknife-mcp`: what an agent CLI starts to use AgentKnife as an MCP
//! server. It connects its stdin/stdout to the running AgentKnife (starting it
//! when it is not running) and passes the messages through unchanged.
//!
//! Messages for the user go to stderr: clients show it in their MCP log.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use ak_mcp::{Endpoint, HELLO};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// How long AgentKnife may take to start and listen.
const START_WAIT: Duration = Duration::from_secs(60);

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("agentknife-mcp: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let file = Endpoint::default_path().ok_or("this computer has no app-data folder")?;
    let stream = match connect(&file).await {
        Ok(s) => s,
        Err(Problem::Off) => return Err(OFF.into()),
        Err(Problem::NotRunning) => {
            start_agentknife()?;
            wait_for(&file).await?
        }
        Err(Problem::Refused(e)) => return Err(e),
    };
    pipe(stream).await
}

const OFF: &str = "AgentKnife is open, but other AI tools may not operate it yet. In \
                   AgentKnife, turn on \"Let other AI tools operate AgentKnife\" (Connect AI, \
                   or Settings), then try again.";

enum Problem {
    /// AgentKnife runs with the setting off.
    Off,
    NotRunning,
    Refused(String),
}

async fn connect(file: &Path) -> Result<TcpStream, Problem> {
    let Some(endpoint) = Endpoint::read(file) else {
        return Err(Problem::NotRunning);
    };
    let (Some(port), Some(token)) = (endpoint.port, endpoint.token) else {
        return Err(if endpoint.enabled {
            Problem::NotRunning
        } else {
            Problem::Off
        });
    };
    // A file left by an AgentKnife that did not close normally: nobody listens.
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)).await else {
        return Err(Problem::NotRunning);
    };
    let hello = format!("{HELLO} {token}\n");
    stream
        .write_all(hello.as_bytes())
        .await
        .map_err(|e| Problem::Refused(e.to_string()))?;
    let mut answer = String::new();
    let mut reader = BufReader::new(&mut stream);
    reader
        .read_line(&mut answer)
        .await
        .map_err(|e| Problem::Refused(e.to_string()))?;
    if answer.trim_end() != "OK" {
        return Err(Problem::Refused(
            "AgentKnife did not accept the connection; close and open AgentKnife, then try again."
                .into(),
        ));
    }
    Ok(stream)
}

/// AgentKnife sits next to this program (also in a development build).
fn agentknife_path() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_owned();
    let name = if cfg!(windows) {
        "agentknife.exe"
    } else {
        "agentknife"
    };
    let path = dir.join(name);
    path.is_file().then_some(path)
}

fn start_agentknife() -> Result<(), String> {
    let path = agentknife_path().ok_or(
        "AgentKnife is not running and could not be found next to agentknife-mcp: open it yourself, then try again.",
    )?;
    eprintln!("agentknife-mcp: starting AgentKnife…");
    std::process::Command::new(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not start AgentKnife ({}): {e}", path.display()))
}

async fn wait_for(file: &Path) -> Result<TcpStream, String> {
    let until = Instant::now() + START_WAIT;
    loop {
        match connect(file).await {
            Ok(s) => return Ok(s),
            Err(Problem::Off) => return Err(OFF.into()),
            Err(Problem::Refused(e)) => return Err(e),
            Err(Problem::NotRunning) if Instant::now() < until => {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(Problem::NotRunning) => {
                return Err(
                    "AgentKnife did not start in time; open it yourself, then try again.".into(),
                );
            }
        }
    }
}

/// stdin → AgentKnife, AgentKnife → stdout, until either side closes.
async fn pipe(stream: TcpStream) -> Result<(), String> {
    let (mut from_app, mut to_app) = stream.into_split();
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let up = async {
        let r = tokio::io::copy(&mut stdin, &mut to_app).await;
        let _ = to_app.shutdown().await;
        r
    };
    let down = async {
        let r = tokio::io::copy(&mut from_app, &mut stdout).await;
        let _ = stdout.flush().await;
        r
    };
    tokio::select! {
        r = up => r.map(drop).map_err(|e| e.to_string()),
        r = down => match r {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("the connection to AgentKnife broke: {e}")),
        },
    }
}
