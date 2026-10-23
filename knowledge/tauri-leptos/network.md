# Data from the internet (weather, exchange rates, maps…)

## Try the service first, with curl
Before writing code, call the service with `run_command` (Windows has `curl`):
`curl -s "https://api.open-meteo.com/v1/forecast?latitude=25.03&longitude=121.56&current=temperature_2m&timezone=auto"`.
Look at the real answer: field names, units, what a search for the user's place
and language returns (searches in some languages are poor; a language code may
not give the script the user expects; results may sort by population). Then
write the Rust types from that answer. Prefer free services that need no key.

## Fetching from the screens
The screens can ask the internet directly with the browser's `fetch`, in the
preview and in the installed app. In the screens' `Cargo.toml`:
```toml
serde_json = "1.0.154"
wasm-bindgen-futures = "0.4.79"
web-sys = { version = "0.3.106", features = ["Window", "Request", "RequestInit", "Response", "AbortController", "AbortSignal"] }
```
```rust
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// GETs `url` and returns the answer as text; gives up after `timeout_ms`.
async fn fetch_text(url: &str, timeout_ms: u64) -> Result<String, String> {
    let window = web_sys::window().ok_or("no window")?;
    let abort = web_sys::AbortController::new().map_err(|e| format!("{e:?}"))?;
    let init = web_sys::RequestInit::new();
    init.set_signal(Some(&abort.signal()));
    // Cancels the request when the time is up (harmless if it already finished).
    set_timeout(
        move || abort.abort(),
        std::time::Duration::from_millis(timeout_ms),
    );
    let answer = JsFuture::from(window.fetch_with_str_and_init(url, &init))
        .await
        .map_err(|_| "Could not reach the server (no connection, or it took too long).".to_string())?;
    let response: web_sys::Response = answer.dyn_into().map_err(|_| "No answer.".to_string())?;
    if !response.ok() {
        return Err(format!("The server answered with error {}.", response.status()));
    }
    let text = response.text().map_err(|e| format!("{e:?}"))?;
    JsFuture::from(text)
        .await
        .map_err(|e| format!("{e:?}"))?
        .as_string()
        .ok_or_else(|| "The answer was not text.".to_string())
}

#[derive(serde::Deserialize, Clone)]
struct Current {
    temperature_2m: f64,
    time: String, // the place's own clock, "2026-10-23T14:00"
}

#[derive(serde::Deserialize, Clone)]
struct Forecast {
    current: Current,
}

#[component]
fn Weather() -> impl IntoView {
    let toaster = use_toaster();
    let now = RwSignal::new(None::<Current>);
    let load = move || {
        leptos::task::spawn_local(async move {
            let url = "https://api.open-meteo.com/v1/forecast?latitude=25.03&longitude=121.56&current=temperature_2m&timezone=auto";
            match fetch_text(url, 10_000).await.and_then(|t| {
                serde_json::from_str::<Forecast>(&t).map_err(|e| format!("Unexpected answer: {e}"))
            }) {
                Ok(f) => now.set(Some(f.current)),
                Err(e) => toaster.error(e),
            }
        });
    };
    load();
    view! {
        <p data-ak-id="weather-now">
            {move || now.get().map(|c| format!("{}  {:.0}°C", &c.time[11..16], c.temperature_2m))}
        </p>
    }
}
```
- Always give up after a while (`AbortController` above) and show the error: a
  request without a time limit can leave the screen waiting forever.
- The browser lets the screens read an answer only when the service allows it
  (CORS); public JSON services usually do. If every request fails although
  `curl` works, the service does not: fetch it in a backend command instead, with
  an HTTP crate (look up its version in the libraries folder; its first build is
  long).
- Put user text in a URL encoded: `String::from(js_sys::encode_uri_component(&city))`.
- Keep the last answer (topic `storage`) so the app shows something offline, and
  do not ask again on every render: load once, then on a button or a timer
  (topic `leptos-reactivity`).
- Times of other places: topic `date-time`.
