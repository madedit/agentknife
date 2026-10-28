# Music and sound effects

For any app that plays sound: a game, a timer that rings, a button that clicks,
a music player with the user's own songs and a spectrum ("A music player").
The sounds come from the user's files in `assets/sounds/`. For sounds made
without files (beeps, an engine), see topic `three-d`, "Growing the game", Sound.

## The files
- **Sounds come from the user**, never from the internet. They add them with
  "Add files…", into `assets/sounds/`; the app loads them as
  `./assets/sounds/coin.mp3`, in the preview and once installed. Until a file is
  there, the app must still work: the sound simply does not play, and the state
  says why (below). Ask the user for the sounds you need, by name ("a short coin
  sound and some background music"), and where they come from (a `CREDITS.txt`
  next to them, as for models).
- **MP3** plays everywhere: WebView2 (Windows), WKWebView (macOS) and WebKitGTK
  (Linux, with GStreamer's plugins, below). Ask for MP3. WAV plays too but is
  large: only for very short effects. Do not rely on OGG, Opus, WebM, FLAC, AAC
  or M4A: some systems do not play them. When the user adds one of those, ask
  for an MP3 version of it.
- Keep music small: about 1 MB per minute at 128 kbit/s. The preview reloads the
  whole page after each change and loads the files again.

## Rules browsers keep
- **No sound before the first click or key.** Until the user clicks or presses
  a key in the app, `play()` fails (`NotAllowedError`) and Web Audio stays
  silent. Start the sound from the first `pointerdown` or `keydown` anywhere in
  the window, not when the screen appears. A screen that should start with
  music says so in its state ("waiting for a click").
- **Music**: one `<audio>` (`HtmlAudioElement`) that loops. **Effects** played
  often or several at once (a coin, a jump, a click): decode each file once
  (`decode_audio_data`) and make a new `AudioBufferSourceNode` for every play
  (a source plays once; they are cheap).
- **Volume**: the music and the effects each go through a `GainNode`, so the
  music can fade and one volume sets both. Keep the volume and mute in the
  app's settings (topic `storage`, localStorage is enough) and show a control.
- **Stop everything when the screen goes away** (`on_cleanup`): pause the music
  and close the `AudioContext`, or the music plays on behind another screen.

## src/sound.rs
`Cargo.toml` (screens), `[dependencies]`:
```toml
serde_json = "1.0.154"
js-sys = "0.3.106"
wasm-bindgen-futures = "0.4.79"
web-sys = { version = "0.3.106", features = ["AnalyserNode", "AudioBuffer", "AudioBufferSourceNode", "AudioContext", "AudioDestinationNode", "AudioNode", "AudioParam", "AudioScheduledSourceNode", "CanvasRenderingContext2d", "Element", "GainNode", "HtmlAudioElement", "HtmlCanvasElement", "HtmlElement", "HtmlMediaElement", "MediaElementAudioSourceNode", "Response", "Storage", "Window"] }
```
(if web-sys is there already, add these features to its list). Copy the list
rather than writing it from memory: a missing one gives `no method named
create_media_element_source` (or another method) with no hint of which. Then
`mod sound;` in `src/main.rs`, and this file; change the file names in `MUSIC`
and `EFFECTS` to the user's files:
```rust
use std::collections::HashMap;
use std::time::Duration;

use leptos::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{AudioBuffer, AudioContext, GainNode, HtmlAudioElement};

const MUSIC: &str = "./assets/sounds/music.mp3";
/// Short effects by name: decoded once, played as often as needed.
const EFFECTS: &[(&str, &str)] = &[
    ("coin", "./assets/sounds/coin.mp3"),
    ("jump", "./assets/sounds/jump.mp3"),
];
/// The music's level next to the effects; the user's volume sets both.
const MUSIC_LEVEL: f32 = 0.5;

/// The web objects (not Send): kept in a `StoredValue::new_local`.
#[derive(Default)]
struct Parts {
    ctx: Option<AudioContext>,
    music: Option<HtmlAudioElement>,
    music_gain: Option<GainNode>,
    effects_gain: Option<GainNode>,
    effects: HashMap<&'static str, AudioBuffer>,
}

/// One screen's sound. It is Copy: use it in any handler.
#[derive(Clone, Copy)]
pub struct Sound {
    parts: StoredValue<Parts, LocalStorage>,
    /// "waiting for a click", "playing", "paused" or "failed: …".
    pub music: RwSignal<String>,
    loaded: RwSignal<Vec<String>>,
    failed: RwSignal<Vec<String>>,
    last_effect: RwSignal<String>,
    pub volume: RwSignal<f64>,
    pub muted: RwSignal<bool>,
}

impl Sound {
    /// Reads the saved volume; nothing plays before `start`.
    pub fn new() -> Self {
        let volume = load_setting("sound-volume").and_then(|v| v.parse().ok());
        Sound {
            parts: StoredValue::new_local(Parts::default()),
            music: RwSignal::new("waiting for a click".to_owned()),
            loaded: RwSignal::new(Vec::new()),
            failed: RwSignal::new(Vec::new()),
            last_effect: RwSignal::new(String::new()),
            volume: RwSignal::new(volume.unwrap_or(0.8)),
            muted: RwSignal::new(load_setting("sound-muted").as_deref() == Some("true")),
        }
    }

    /// The level the user chose, 0 when muted.
    fn level(self) -> f32 {
        if self.muted.get_untracked() {
            0.0
        } else {
            self.volume.get_untracked() as f32
        }
    }

    /// Call on the first click or key (browsers keep sound off until then):
    /// loads the effects and starts the music. Later calls do nothing.
    pub fn start(self) {
        if self.parts.with_value(|p| p.ctx.is_some()) {
            return;
        }
        let Ok(ctx) = AudioContext::new() else {
            self.music.set("failed: this computer has no sound output".into());
            return;
        };
        let effects_gain = gain(&ctx, self.level());
        // The music goes through its own gain, so it can fade in and out.
        let music_gain = gain(&ctx, 0.0);
        let music = HtmlAudioElement::new_with_src(MUSIC).ok();
        if let (Some(m), Some(g)) = (&music, &music_gain) {
            m.set_loop(true);
            if let Ok(source) = ctx.create_media_element_source(m) {
                let _ = source.connect_with_audio_node(g);
            }
        }
        for &(name, url) in EFFECTS {
            let ctx = ctx.clone();
            leptos::task::spawn_local(async move {
                match decode(&ctx, url).await {
                    Ok(buffer) => {
                        self.parts.update_value(|p| {
                            p.effects.insert(name, buffer);
                        });
                        self.loaded.update(|l| l.push(name.to_owned()));
                    }
                    Err(e) => self.failed.update(|f| f.push(format!("{name}: {e}"))),
                }
            });
        }
        self.parts.update_value(|p| {
            p.ctx = Some(ctx);
            p.music = music;
            p.music_gain = music_gain;
            p.effects_gain = effects_gain;
        });
        self.play_music();
    }

    /// Starts or resumes the music, fading in over a second.
    pub fn play_music(self) {
        let Some(music) = self.parts.with_value(|p| p.music.clone()) else {
            return;
        };
        let Ok(playing) = music.play() else {
            return;
        };
        leptos::task::spawn_local(async move {
            match JsFuture::from(playing).await {
                Ok(_) => {
                    self.music.set("playing".into());
                    self.fade_music(self.level() * MUSIC_LEVEL, 1.0);
                }
                // NotSupportedError: the file is missing or not a format this system plays.
                Err(e) => self.music.set(format!("failed: {}", error_text(&e))),
            }
        });
    }

    /// Fades the music out over half a second, then pauses it.
    pub fn pause_music(self) {
        self.music.set("paused".into());
        self.fade_music(0.0, 0.5);
        set_timeout(
            move || {
                // Unless it was started again meanwhile.
                if self.music.try_get_untracked().as_deref() == Some("paused")
                    && let Some(m) = self.parts.try_with_value(|p| p.music.clone()).flatten()
                {
                    let _ = m.pause();
                }
            },
            Duration::from_millis(500),
        );
    }

    /// Moves the music's level to `to` over `seconds`.
    fn fade_music(self, to: f32, seconds: f64) {
        self.parts.with_value(|p| {
            if let (Some(ctx), Some(g)) = (&p.ctx, &p.music_gain) {
                let now = ctx.current_time();
                let level = g.gain();
                let _ = level.cancel_scheduled_values(now);
                let _ = level.set_value_at_time(level.value(), now);
                let _ = level.linear_ramp_to_value_at_time(to, now + seconds);
            }
        });
    }

    /// Plays an effect; several can play at once. Nothing plays before `start`
    /// or while its file is still loading.
    pub fn play(self, name: &str) {
        let played = self.parts.with_value(|p| {
            let (Some(ctx), Some(gain), Some(buffer)) =
                (&p.ctx, &p.effects_gain, p.effects.get(name))
            else {
                return false;
            };
            // A source plays once: a new one each time.
            let Ok(source) = ctx.create_buffer_source() else {
                return false;
            };
            source.set_buffer(Some(buffer));
            source.connect_with_audio_node(gain).is_ok() && source.start().is_ok()
        });
        if played {
            self.last_effect.set(name.to_owned());
        }
    }

    /// Sets and saves the volume (0 to 1) and mute.
    pub fn set_volume(self, volume: f64, muted: bool) {
        self.volume.set(volume.clamp(0.0, 1.0));
        self.muted.set(muted);
        save_setting("sound-volume", &self.volume.get_untracked().to_string());
        save_setting("sound-muted", if muted { "true" } else { "false" });
        let level = self.level();
        self.parts.with_value(|p| {
            if let Some(g) = &p.effects_gain {
                g.gain().set_value(level);
            }
        });
        if self.music.get_untracked() == "playing" {
            self.fade_music(level * MUSIC_LEVEL, 0.2);
        }
    }

    /// Stops everything: call it in the screen's `on_cleanup`.
    pub fn stop(self) {
        if let Some(p) = self.parts.try_update_value(std::mem::take) {
            if let Some(m) = &p.music {
                let _ = m.pause();
            }
            if let Some(ctx) = &p.ctx {
                let _ = ctx.close();
            }
        }
    }

    /// What plays, for `set_state_reader`: no sound is heard while checking.
    pub fn state(self) -> serde_json::Value {
        serde_json::json!({
            "music": self.music.get_untracked(),
            "effects_loaded": self.loaded.get_untracked(),
            "effects_failed": self.failed.get_untracked(),
            "last_effect": self.last_effect.get_untracked(),
            "volume": self.volume.get_untracked(),
            "muted": self.muted.get_untracked(),
        })
    }
}

/// A gain (volume) node connected to the speakers.
fn gain(ctx: &AudioContext, level: f32) -> Option<GainNode> {
    let g = ctx.create_gain().ok()?;
    g.gain().set_value(level);
    g.connect_with_audio_node(&ctx.destination()).ok()?;
    Some(g)
}

/// Fetches and decodes a sound file.
async fn decode(ctx: &AudioContext, url: &str) -> Result<AudioBuffer, String> {
    let window = web_sys::window().ok_or("no window")?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| error_text(&e))?
        .unchecked_into();
    if !response.ok() {
        return Err(format!("{url} is missing ({})", response.status()));
    }
    let bytes = response.array_buffer().map_err(|e| error_text(&e))?;
    let bytes: js_sys::ArrayBuffer = JsFuture::from(bytes)
        .await
        .map_err(|e| error_text(&e))?
        .unchecked_into();
    let decoding = ctx.decode_audio_data(&bytes).map_err(|e| error_text(&e))?;
    let buffer = JsFuture::from(decoding).await.map_err(|e| error_text(&e))?;
    Ok(buffer.unchecked_into())
}

/// `NotSupportedError: …` from a failed promise.
fn error_text(e: &JsValue) -> String {
    match e.dyn_ref::<js_sys::Error>() {
        Some(err) => format!("{}: {}", String::from(err.name()), String::from(err.message())),
        None => format!("{e:?}"),
    }
}

fn load_setting(key: &str) -> Option<String> {
    let storage = web_sys::window()?.local_storage().ok()??;
    storage.get_item(key).ok()?
}

fn save_setting(key: &str, value: &str) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.set_item(key, value);
    }
}
```

## Using it on a screen
In the screen's component (in `src/sound.rs` here; elsewhere import it with
`use crate::sound::Sound;`):
```rust
/// Music, an effect and the volume controls.
#[component]
pub fn SoundControls() -> impl IntoView {
    let sound = Sound::new();
    // The first click or key anywhere starts the sound.
    let click = window_event_listener(leptos::ev::pointerdown, move |_| sound.start());
    let key = window_event_listener(leptos::ev::keydown, move |_| sound.start());
    on_cleanup(move || {
        click.remove();
        key.remove();
        sound.stop();
    });
    // Lets AgentKnife check what plays: dom_summary shows this state.
    ak_bridge::set_state_reader(move || serde_json::json!({ "sound": sound.state() }));
    let playing = move || sound.music.get() == "playing";
    view! {
        <div data-ak-id="sound-controls" class="flex flex-wrap items-center gap-3">
            <button
                data-ak-id="coin-button"
                class="rounded bg-amber-500 px-3 py-1 text-white"
                on:click=move |_| sound.play("coin")
            >
                "Coin"
            </button>
            <button
                data-ak-id="music-button"
                class="rounded border px-3 py-1"
                on:click=move |_| if playing() { sound.pause_music() } else { sound.play_music() }
            >
                {move || if playing() { "Pause music" } else { "Play music" }}
            </button>
            <label class="flex items-center gap-2">
                "Volume"
                <input
                    data-ak-id="volume"
                    type="range"
                    min="0"
                    max="1"
                    step="0.05"
                    prop:value=move || sound.volume.get().to_string()
                    on:input=move |e| {
                        let v = event_target_value(&e).parse().unwrap_or(0.8);
                        sound.set_volume(v, sound.muted.get_untracked());
                    }
                />
            </label>
            <button
                data-ak-id="mute-button"
                class="rounded border px-3 py-1"
                on:click=move |_| sound.set_volume(sound.volume.get_untracked(), !sound.muted.get_untracked())
            >
                {move || if sound.muted.get() { "Sound on" } else { "Mute" }}
            </button>
        </div>
    }
}
```
- An app with one `set_state_reader` already (a game): put `sound.state()`
  into that one, as a `"sound"` field. Calling it twice keeps only the last.
- A sound for an event (a timer done, a point scored): call `sound.play("…")`
  where the event happens, not in an `Effect`.
- Texts on the buttons are in the user's language, like every label.

## In a three.js game (game.js)
When the game's logic is in `game.js` (topic `three-d`), keep the sound there
too, with the same rules:
```js
const EFFECTS = { coin: "./assets/sounds/coin.mp3", crash: "./assets/sounds/crash.mp3" };

class GameSound {
  constructor() {
    this.ctx = null;
    this.buffers = new Map();
    this.state = { music: "waiting for a click", effects_loaded: [], effects_failed: [], last_effect: "" };
  }

  // From the first keydown / pointerdown handler; later calls do nothing.
  start(volume = 0.8) {
    if (this.ctx) return;
    this.ctx = new AudioContext();
    this.effects = new GainNode(this.ctx, { gain: volume });
    this.effects.connect(this.ctx.destination);
    this.musicGain = new GainNode(this.ctx, { gain: 0 });
    this.musicGain.connect(this.ctx.destination);
    this.music = new Audio("./assets/sounds/music.mp3");
    this.music.loop = true;
    this.ctx.createMediaElementSource(this.music).connect(this.musicGain);
    this.music.play().then(
      () => { this.state.music = "playing"; this.fade(volume * 0.5, 1); },
      (e) => { this.state.music = `failed: ${e.name}: ${e.message}`; },
    );
    for (const [name, url] of Object.entries(EFFECTS)) this.load(name, url);
  }

  async load(name, url) {
    try {
      const res = await fetch(url);
      if (!res.ok) throw new Error(`${url} is missing (${res.status})`);
      this.buffers.set(name, await this.ctx.decodeAudioData(await res.arrayBuffer()));
      this.state.effects_loaded.push(name);
    } catch (e) {
      this.state.effects_failed.push(`${name}: ${e.message}`);
    }
  }

  play(name) {
    const buffer = this.buffers.get(name);
    if (!buffer) return;
    const source = new AudioBufferSourceNode(this.ctx, { buffer }); // one per play
    source.connect(this.effects);
    source.start();
    this.state.last_effect = name;
  }

  fade(to, seconds) {
    const g = this.musicGain.gain;
    const t = this.ctx.currentTime;
    g.cancelScheduledValues(t);
    g.setValueAtTime(g.value, t);
    g.linearRampToValueAtTime(to, t + seconds);
  }

  dispose() {
    this.music?.pause();
    this.ctx?.close();
  }
}
```
In the game: `this.sound = new GameSound()` in the constructor,
`this.sound.start()` in the key and pointer handlers, `this.sound.play("coin")`
where a point is scored, `sound: this.sound.state` in `state_json()`, and
`this.sound.dispose()` in `dispose()`. A volume setting kept by the Rust side
goes in with a method (`setVolume(v)`: `this.effects.gain.value = v`).

## A music player (the user's own songs)
When the app *is* the player, the songs are not in `assets/`: the user picks them
from their computer while the app runs.
- **Picking**: a backend command opens the system's file dialog (topic
  `images-dialogs`). The preview has no dialogs: next to the button, a field
  where a path can be pasted, calling the same command.
- **Remember where the song is**: the list of songs keeps each song's name and
  its path on the computer, saved like any data (topic `storage`). Do not copy
  the files: the songs stay where the user keeps them, and take no space twice.
- **Playing**: the screens cannot read files, so the backend reads the song and
  returns it as a data URL for `<audio src>`. Keep only the current song's URL
  (a long song is a few MB of text); fetch the next one when the song changes.
- **A song that is gone** (the user moved, renamed or deleted it): `song_url`
  fails. Show the message on that song in the list (not only in the player),
  skip to the next song, and offer "Remove" and "Add it again" (a new path).
  Never remove it from the list by yourself.
- **Copy only when the user asks** for songs that keep playing after the
  originals are gone: then `add_song` copies the file into
  `app.path().app_data_dir()?.join("songs")` (`use tauri::Manager;`, a new
  name for each copy) and returns the copy's path, which the list remembers.
  Tell the user the songs then take space twice.
- Backend (`src-tauri/src/main.rs`; `base64 = "0.22.1"` in its
  `[dependencies]`, both commands in `generate_handler!`):
  ```rust
  /// Checks a song the user picked; returns its name for the list.
  #[tauri::command]
  fn add_song(path: String) -> Result<String, String> {
      let path = std::path::Path::new(&path);
      song_type(path)?;
      if !path.is_file() {
          return Err("There is no song at this path.".into());
      }
      let name = path.file_stem().and_then(|n| n.to_str()).unwrap_or("Song");
      Ok(name.to_owned())
  }

  /// One song as a data URL for `<audio src>`, read from where the user keeps it.
  #[tauri::command]
  fn song_url(path: String) -> Result<String, String> {
      use base64::Engine;
      let path = std::path::Path::new(&path);
      let mime = song_type(path)?;
      let bytes = std::fs::read(path)
          .map_err(|_| "The song is no longer there (moved or deleted). Add it again.".to_owned())?;
      let data = base64::engine::general_purpose::STANDARD.encode(bytes);
      Ok(format!("data:{mime};base64,{data}"))
  }

  /// Only sound files: the screens cannot ask for any other file.
  fn song_type(path: &std::path::Path) -> Result<&'static str, String> {
      let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
      match ext.to_ascii_lowercase().as_str() {
          "mp3" => Ok("audio/mpeg"),
          "wav" => Ok("audio/wav"),
          _ => Err("Choose an MP3 or WAV file.".into()),
      }
  }
  ```
- **The `<audio>` element goes in the `view!`**: `on:timeupdate`,
  `on:loadedmetadata`, `on:playing`, `on:pause`, `on:ended` and `on:error` work
  on it like `on:click` (no `Closure` to keep), and a `NodeRef` gives the
  element for `play()`, `pause()` and `set_current_time`.
- **Events can arrive late**: a `pause` event from an earlier `pause()` can come
  after the next `play()`. Do not set "paused" because a `pause` event came: in
  each of these events read the element itself (`paused()`, `ended()`).
- **A spectrum** (bars that move with the music): an `AnalyserNode` between the
  song and the speakers. `get_byte_frequency_data` takes a `&mut [u8]` (a `Vec`),
  not a `Uint8Array`. Draw it on a `<canvas>` every 50 ms (20 pictures a
  second is enough) and stop the timer in `on_cleanup`. Draw something when
  nothing plays (short bars), so the screen does not look broken.
- The player below goes in `src/sound.rs` too, after the code above (the
  features listed there cover it). `src` is the current song's data URL, empty
  for none:

```rust
/// Play/pause, a seek bar, the time and a spectrum for one song.
#[component]
pub fn Player(#[prop(into)] src: Signal<String>) -> impl IntoView {
    let audio = NodeRef::<leptos::html::Audio>::new();
    let canvas = NodeRef::<leptos::html::Canvas>::new();
    // "no song", "loading", "playing", "paused", "ended" or "failed: …".
    let state = RwSignal::new("no song".to_owned());
    let position = RwSignal::new(0.0f64);
    let duration = RwSignal::new(0.0f64);
    let analyser = StoredValue::new_local(None::<(AudioContext, web_sys::AnalyserNode)>);
    Effect::new(move |_| {
        let none = src.get().is_empty();
        state.set(if none { "no song" } else { "loading" }.to_owned());
    });
    // Events can come late: the element itself says whether it plays.
    let sync = move || {
        if let Some(a) = audio.get_untracked() {
            let now = if a.ended() {
                "ended"
            } else if a.paused() {
                "paused"
            } else {
                "playing"
            };
            state.set(now.to_owned());
        }
    };
    let toggle = move |_| {
        let Some(a) = audio.get_untracked() else {
            return;
        };
        if analyser.with_value(Option::is_none) {
            // After a click the browser lets sound start: song → analyser → speakers.
            if let Ok(ctx) = AudioContext::new()
                && let Ok(source) = ctx.create_media_element_source(&a)
                && let Ok(node) = ctx.create_analyser()
            {
                node.set_fft_size(64);
                let _ = source.connect_with_audio_node(&node);
                let _ = node.connect_with_audio_node(&ctx.destination());
                analyser.set_value(Some((ctx, node)));
            }
        }
        if !a.paused() {
            let _ = a.pause();
            return;
        }
        if let Ok(playing) = a.play() {
            leptos::task::spawn_local(async move {
                if let Err(e) = JsFuture::from(playing).await {
                    state.set(format!("failed: {}", error_text(&e)));
                }
            });
        }
    };
    // The timer's handle is a Result; `.ok()` keeps it for on_cleanup.
    let timer = set_interval_with_handle(
        move || draw_spectrum(canvas, analyser),
        Duration::from_millis(50),
    )
    .ok();
    on_cleanup(move || {
        if let Some(h) = timer {
            h.clear();
        }
        if let Some(a) = audio.get_untracked() {
            let _ = a.pause();
        }
        if let Some((ctx, _)) = analyser.try_update_value(Option::take).flatten() {
            let _ = ctx.close();
        }
    });
    // What plays, in whole seconds (an app with its own reader puts this in that one).
    ak_bridge::set_state_reader(move || {
        serde_json::json!({ "player": {
            "state": state.get_untracked(),
            "position": position.get_untracked().floor(),
            "duration": duration.get_untracked().floor(),
        } })
    });
    view! {
        <section data-ak-id="player" class="flex flex-col gap-2">
            <audio
                node_ref=audio
                src=move || Some(src.get()).filter(|s| !s.is_empty())
                on:loadedmetadata=move |_| {
                    if let Some(a) = audio.get_untracked() {
                        duration.set(a.duration());
                    }
                    sync();
                }
                on:timeupdate=move |_| {
                    if let Some(a) = audio.get_untracked() {
                        position.set(a.current_time());
                    }
                }
                on:playing=move |_| sync()
                on:pause=move |_| sync()
                on:ended=move |_| sync()
                on:error=move |_| state.set("failed: this song cannot be played".to_owned())
            ></audio>
            <canvas node_ref=canvas data-ak-id="spectrum" class="h-[clamp(40px,10vh,96px)] w-full"></canvas>
            <div class="flex items-center gap-3">
                <button
                    data-ak-id="play-button"
                    class="shrink-0 rounded bg-sky-600 px-3 py-1 text-white"
                    on:click=toggle
                >
                    {move || if state.get() == "playing" { "Pause" } else { "Play" }}
                </button>
                <input
                    data-ak-id="seek"
                    type="range"
                    min="0"
                    step="0.1"
                    class="min-w-0 flex-1"
                    prop:max=move || duration.get().max(0.0).to_string()
                    prop:value=move || position.get().to_string()
                    on:input=move |e| {
                        if let (Some(a), Ok(t)) = (audio.get_untracked(), event_target_value(&e).parse()) {
                            a.set_current_time(t);
                        }
                    }
                />
                <span class="shrink-0 whitespace-nowrap text-sm tabular-nums">
                    {move || format!("{} / {}", clock(position.get()), clock(duration.get()))}
                </span>
            </div>
        </section>
    }
}

/// 83.4 seconds → "1:23".
fn clock(seconds: f64) -> String {
    if !seconds.is_finite() {
        return "0:00".to_owned();
    }
    let s = seconds as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The spectrum's bars; short ones while nothing plays.
fn draw_spectrum(
    canvas: NodeRef<leptos::html::Canvas>,
    analyser: StoredValue<Option<(AudioContext, web_sys::AnalyserNode)>, LocalStorage>,
) {
    let Some(el) = canvas.get_untracked() else {
        return;
    };
    let Some(ctx) = el
        .get_context("2d")
        .ok()
        .flatten()
        .and_then(|c| c.dyn_into::<web_sys::CanvasRenderingContext2d>().ok())
    else {
        return;
    };
    // Sharp on high-resolution screens: one canvas pixel per screen pixel.
    // Setting the size clears the drawing state, so the transform comes after.
    let ratio = web_sys::window().map(|w| w.device_pixel_ratio()).unwrap_or(1.0);
    let (w, h) = (f64::from(el.client_width()), f64::from(el.client_height()));
    let (pw, ph) = ((w * ratio) as u32, (h * ratio) as u32);
    if el.width() != pw || el.height() != ph {
        el.set_width(pw);
        el.set_height(ph);
    }
    let _ = ctx.set_transform(ratio, 0.0, 0.0, ratio, 0.0, 0.0);
    ctx.clear_rect(0.0, 0.0, w, h);
    let mut data = vec![0u8; 32];
    analyser.with_value(|a| {
        if let Some((_, node)) = a {
            data.resize(node.frequency_bin_count() as usize, 0);
            node.get_byte_frequency_data(&mut data);
        }
    });
    ctx.set_fill_style_str("#38bdf8");
    let bar = w / data.len() as f64;
    for (i, v) in data.iter().enumerate() {
        let height = (f64::from(*v) / 255.0 * h).max(2.0);
        ctx.fill_rect(i as f64 * bar + 1.0, h - height, (bar - 2.0).max(1.0), height);
    }
}
```
- The position moves by itself: `interact` `read_state` with `samples: 3,
  wait_ms: 3000` shows whether the song plays.

## Checking it
No sound is heard while you check the preview. Check the state instead:
- Before any click: `"music": "waiting for a click"`, nothing loaded. After an
  `interact` click on the page: `"playing"`, `effects_loaded` lists the effects,
  and a click on a sound button sets `last_effect`.
- `"failed: NotSupportedError: …"` or `effects_failed` with "is missing": the file
  is not in `assets/sounds/` under that name, or it is not a format this system
  plays. Check the name with `list_files`; ask the user for the file (MP3).
- Then ask the user to listen: in the preview, and in the packaged app.

## Linux
WebKitGTK plays sound through GStreamer. Without its plugins (`playback`,
`autodetect`, `audioparsers`, `mpg123`) the page works but stays silent, or
`play()` fails with `NotSupportedError`. AgentKnife's setup checks them on this
computer (the command it shows installs them). On another Linux computer the
packaged app needs them too: the install steps AgentKnife gives name
`gstreamer1.0-plugins-good` (Ubuntu, Debian).
