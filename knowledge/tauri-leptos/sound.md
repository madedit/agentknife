# Music and sound effects

For any app that plays sound: a game, a timer that rings, a button that clicks.
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
web-sys = { version = "0.3.106", features = ["AudioBuffer", "AudioBufferSourceNode", "AudioContext", "AudioDestinationNode", "AudioNode", "AudioParam", "AudioScheduledSourceNode", "GainNode", "HtmlAudioElement", "HtmlMediaElement", "MediaElementAudioSourceNode", "Response", "Storage", "Window"] }
```
(if web-sys is there already, add these features to its list). Then
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
