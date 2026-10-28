//! Pictures, sounds, fonts and 3D models shown in the window: a URL scheme
//! that serves the open project's files to AgentKnife's own page.
//!
//! Each URL starts with a token made when AgentKnife starts, which only the
//! page gets (from `read_file`). The preview's page runs in the same web view
//! but cannot guess it, so it cannot read the project's files this way.

use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use ak_agent::tools::path::resolve;
use ak_types::ipc::{AssetKind, Media};
use tauri::http::{Request, Response, StatusCode, header};
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::assets::kind_of;
use crate::state::AppState;

pub const SCHEME: &str = "akfile";
/// The most a range request gets at once (the page asks again for more).
const CHUNK: u64 = 4 * 1024 * 1024;

/// Where the scheme's URLs start (Windows and Android serve custom schemes
/// over http).
fn base() -> String {
    if cfg!(any(windows, target_os = "android")) {
        format!("http://{SCHEME}.localhost")
    } else {
        format!("{SCHEME}://localhost")
    }
}

fn encode(rel: &str) -> String {
    rel.split('/')
        .map(|part| {
            part.bytes()
                .map(|b| match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                        (b as char).to_string()
                    }
                    _ => format!("%{b:02X}"),
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The URL of a project file (`rel`, with `/`); `modified` makes a changed
/// file a new URL.
pub fn url(token: &str, rel: &str, modified: u64) -> String {
    format!("{}/{token}/{}?v={modified}", base(), encode(rel))
}

/// How the window shows `path` (`rel` in the project), or `None` for text
/// and files it cannot show.
pub fn describe(token: &str, rel: &str, path: &Path) -> Option<Media> {
    let kind = kind_of(rel);
    if kind == AssetKind::Other {
        return None;
    }
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64);
    Some(Media {
        kind,
        url: url(token, rel, modified),
        size: meta.len(),
    })
}

fn content_type(name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "gltf" => "model/gltf+json",
        "glb" => "model/gltf-binary",
        _ => "application/octet-stream",
    }
}

/// The bytes a `Range` header asks for (inclusive), at most [`CHUNK`];
/// `None` for the whole file (no header, or one this does not understand).
pub fn range(header: Option<&str>, len: u64) -> Option<(u64, u64)> {
    let spec = header?.trim().strip_prefix("bytes=")?;
    if spec.contains(',') || len == 0 {
        return None;
    }
    let (start, end) = spec.split_once('-')?;
    let (start, end) = match (start.trim(), end.trim()) {
        ("", n) => {
            let n: u64 = n.parse().ok()?;
            (len.saturating_sub(n), len - 1)
        }
        (s, "") => (s.parse().ok()?, len - 1),
        (s, e) => (s.parse().ok()?, e.parse::<u64>().ok()?.min(len - 1)),
    };
    if start > end {
        return None;
    }
    Some((start, end.min(start + CHUNK - 1)))
}

/// The project file a request names, if its token is right.
fn requested(root: &Path, token: &str, uri_path: &str) -> Option<PathBuf> {
    let rest = uri_path.strip_prefix('/')?.strip_prefix(token)?;
    let rel = decode(rest.strip_prefix('/')?)?;
    resolve(root, &rel).ok()
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    let mut r = Response::new(Vec::new());
    *r.status_mut() = code;
    r
}

async fn serve(
    root: Option<PathBuf>,
    token: String,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let Some(path) = root.and_then(|r| requested(&r, &token, request.uri().path())) else {
        return status(StatusCode::NOT_FOUND);
    };
    let Ok(mut file) = tokio::fs::File::open(&path).await else {
        return status(StatusCode::NOT_FOUND);
    };
    let Ok(len) = file.metadata().await.map(|m| m.len()) else {
        return status(StatusCode::NOT_FOUND);
    };
    let asked = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok());
    let builder = Response::builder()
        .header(header::CONTENT_TYPE, content_type(&path.to_string_lossy()))
        .header(header::ACCEPT_RANGES, "bytes")
        // Fonts load with CORS from the page's own origin.
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::CACHE_CONTROL, "no-cache");
    let result = match range(asked, len) {
        Some((start, end)) => {
            let mut body = vec![0; (end - start + 1) as usize];
            match file.seek(SeekFrom::Start(start)).await {
                Ok(_) => file.read_exact(&mut body).await.map(|_| {
                    builder
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
                        .body(body)
                }),
                Err(e) => Err(e),
            }
        }
        None => {
            let mut body = Vec::with_capacity(len as usize);
            file.read_to_end(&mut body)
                .await
                .map(|_| builder.body(body))
        }
    };
    match result {
        Ok(Ok(r)) => r,
        _ => status(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// The scheme's handler (`Builder::register_asynchronous_uri_scheme_protocol`).
pub fn handle<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let app = ctx.app_handle().clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let root = state
            .inner
            .lock()
            .await
            .open
            .as_ref()
            .map(|o| o.root.clone());
        let token = state.files_token.clone();
        responder.respond(serve(root, token, request).await);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_round_trip_names_with_spaces_and_other_scripts() {
        let u = url("t0k", "assets/images/貓 (2).png", 7);
        assert!(
            u.ends_with("/t0k/assets/images/%E8%B2%93%20%282%29.png?v=7"),
            "{u}"
        );
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = u
            .split("localhost")
            .nth(1)
            .and_then(|p| p.split('?').next());
        let found = requested(dir.path(), "t0k", path.unwrap_or_default());
        assert_eq!(found, Some(dir.path().join("assets/images/貓 (2).png")));
        // A wrong token, or a way out of the project, finds nothing.
        assert_eq!(
            requested(dir.path(), "other", path.unwrap_or_default()),
            None
        );
        assert_eq!(requested(dir.path(), "t0k", "/t0k/..%2Fsecret.txt"), None);
        assert_eq!(requested(dir.path(), "t0k", "/t0k"), None);
    }

    #[test]
    fn ranges() {
        assert_eq!(range(None, 100), None);
        assert_eq!(range(Some("bytes=0-"), 100), Some((0, 99)));
        assert_eq!(range(Some("bytes=10-19"), 100), Some((10, 19)));
        assert_eq!(range(Some("bytes=90-200"), 100), Some((90, 99)));
        assert_eq!(range(Some("bytes=-10"), 100), Some((90, 99)));
        assert_eq!(range(Some("bytes=0-"), 10 * CHUNK), Some((0, CHUNK - 1)));
        assert_eq!(range(Some("bytes=50-10"), 100), None);
        assert_eq!(range(Some("bytes=0-1,5-6"), 100), None);
        assert_eq!(range(Some("bytes=0-"), 0), None);
    }

    #[test]
    fn only_assets_the_window_can_show_are_media() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let png = dir.path().join("cat.png");
        std::fs::write(&png, [1, 2, 3]).unwrap_or_else(|e| panic!("{e}"));
        let m = describe("t", "cat.png", &png).unwrap_or_else(|| panic!("an image"));
        assert_eq!((m.kind, m.size), (AssetKind::Image, 3));
        assert!(m.url.contains("/t/cat.png?v="));
        let rs = dir.path().join("main.rs");
        std::fs::write(&rs, "fn main() {}").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(describe("t", "main.rs", &rs), None);
        assert_eq!(
            describe("t", "gone.png", &dir.path().join("gone.png")),
            None
        );
        assert_eq!(content_type("a/b.WOFF2"), "font/woff2");
    }
}
