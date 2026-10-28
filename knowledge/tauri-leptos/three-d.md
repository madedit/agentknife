# 3D games and scenes with three.js

For 3D (games, product viewers, 3D charts) use **three.js 0.186.1**, which ships with
AgentKnife. The 3D part is a small JavaScript module drawing on a `<canvas>`; menus,
scores and buttons stay in Leptos. No npm, no CDN links, no downloads: they do not
work here. The Rust side is ordinary Leptos: read topics `leptos-components` and
`ak-ui` too before writing it.

## 1. Add the library
Call `add_library` with `"three"`. It copies into `vendor/three/`:
`three.module.js`, `three.core.js` (three.js itself),
`addons/controls/OrbitControls.js` (drag to turn the view), the model loaders in
`addons/loaders/` (with what they need in `addons/utils/`, `addons/libs/`,
`addons/curves/`) and `LICENSE` (keep it). Other three.js addons (effects,
post-processing) are not available. Build vehicles, buildings and props from
three.js geometries grouped with `THREE.Group` ("Shapes and looks" below), or use
a model the user added ("Models from files" below).

## 2. index.html
Add the import map and the `copy-dir` link right after `<title>`, before the
comment and the `rel="rust"` link. Do not change the other trunk links. The whole
`<head>` afterwards:

```html
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>My Game</title>
    <script type="importmap">
      {
        "imports": {
          "three": "./vendor/three/three.module.js",
          "three/addons/": "./vendor/three/addons/"
        }
      }
    </script>
    <link data-trunk rel="copy-dir" href="vendor" />
    <!-- wasm-opt is off: it is not part of AgentKnife's toolchain. -->
    <link data-trunk rel="rust" data-bin="app-ui" data-wasm-opt="0" />
    <link data-trunk rel="tailwind-css" href="style/input.css" />
  </head>
```

## 3. Cargo.toml (screens)
```toml
serde_json = "1.0.154"
web-sys = { version = "0.3.106", features = ["HtmlCanvasElement"] }
```

## 4. src/game.js: the 3D part
One class. The constructor takes the canvas and callbacks for what Rust must know
(score, game over); methods are what Rust can ask for. Name the methods in
snake_case like Rust (`state_json`, `set_move`), so the names match without
`js_name`. Give every object the player can hit a `name`. Clean everything up in
`dispose()`.

```js
// The 3D part of the app (three.js). The Rust screen creates a Game for a
// <canvas>, hears about the score through a callback and calls dispose() when
// the screen goes away.
import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";

export class Game {
  // onScore(points) is called with the new score after every hit.
  constructor(canvas, onScore) {
    this.canvas = canvas;
    this.onScore = onScore;
    this.score = 0;

    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
    this.renderer.setPixelRatio(window.devicePixelRatio);
    this.scene = new THREE.Scene();
    this.scene.background = new THREE.Color(0x111827);
    this.camera = new THREE.PerspectiveCamera(60, 1, 0.1, 100);
    this.camera.position.set(3, 2, 4);

    this.scene.add(new THREE.AmbientLight(0xffffff, 0.5));
    const sun = new THREE.DirectionalLight(0xffffff, 2);
    sun.position.set(5, 8, 3);
    this.scene.add(sun);

    // Every object the player can hit has a name (AgentKnife shows it when the
    // user points at it).
    this.cube = new THREE.Mesh(
      new THREE.BoxGeometry(1, 1, 1),
      new THREE.MeshStandardMaterial({ color: 0x3b82f6 }),
    );
    this.cube.name = "cube";
    this.scene.add(this.cube);

    // Drag to turn the view, wheel to zoom.
    this.controls = new OrbitControls(this.camera, canvas);
    this.controls.enableDamping = true;

    // A click (not a drag) on the cube scores a point.
    this.raycaster = new THREE.Raycaster();
    this.down = null;
    this.onDown = (e) => (this.down = { x: e.clientX, y: e.clientY });
    this.onUp = (e) => {
      const d = this.down;
      this.down = null;
      if (!d || Math.hypot(e.clientX - d.x, e.clientY - d.y) > 5) return;
      const r = canvas.getBoundingClientRect();
      const x = ((e.clientX - r.left) / r.width) * canvas.width;
      const y = ((e.clientY - r.top) / r.height) * canvas.height;
      if (this.pick(x, y) === "cube") {
        this.score += 1;
        this.cube.material.color.setHSL(Math.random(), 0.7, 0.55);
        this.onScore(this.score);
      }
    };
    canvas.addEventListener("pointerdown", this.onDown);
    canvas.addEventListener("pointerup", this.onUp);

    // Follow the canvas's size on the page.
    this.resize = new ResizeObserver(() => this.fit());
    this.resize.observe(canvas);
    this.fit();

    this.clock = new THREE.Clock();
    this.renderer.setAnimationLoop(() => this.frame());
  }

  fit() {
    const w = this.canvas.clientWidth;
    const h = this.canvas.clientHeight;
    if (w === 0 || h === 0) return;
    this.renderer.setSize(w, h, false);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
  }

  // Called every frame; move things by speed × dt so the speed does not
  // depend on the screen's refresh rate.
  frame() {
    const dt = this.clock.getDelta();
    this.cube.rotation.x += dt * 0.6;
    this.cube.rotation.y += dt * 0.9;
    this.controls.update();
    this.renderer.render(this.scene, this.camera);
  }

  // The name of the object at (x, y) in canvas pixels, or undefined. The ray
  // hits a part (a wolf's leg), so walk up to the nearest named group (the wolf).
  pick(x, y) {
    const ndc = new THREE.Vector2(
      (x / this.canvas.width) * 2 - 1,
      -(y / this.canvas.height) * 2 + 1,
    );
    this.raycaster.setFromCamera(ndc, this.camera);
    let obj = this.raycaster.intersectObjects(this.scene.children, true)[0]?.object;
    while (obj && !obj.name) obj = obj.parent;
    return obj?.name || undefined;
  }

  restart() {
    this.score = 0;
    this.onScore(0);
  }

  // What AgentKnife sees of the game (dom_summary cannot look inside the
  // canvas): everything needed to check the rules.
  state_json() {
    return JSON.stringify({
      score: this.score,
      cubeColor: "#" + this.cube.material.color.getHexString(),
    });
  }

  dispose() {
    this.renderer.setAnimationLoop(null);
    this.resize.disconnect();
    this.canvas.removeEventListener("pointerdown", this.onDown);
    this.canvas.removeEventListener("pointerup", this.onUp);
    this.controls.dispose();
    // Free every geometry, material and texture in the scene.
    this.scene.traverse((obj) => {
      obj.geometry?.dispose();
      for (const m of [obj.material].flat()) {
        for (const value of Object.values(m ?? {})) {
          if (value?.isTexture) value.dispose();
        }
        m?.dispose();
      }
    });
    this.renderer.dispose();
  }
}
```

## 5. src/game.rs: the screen
`#[wasm_bindgen(module = "/src/game.js")]` declares what Rust may call (the path is
from the project folder). Keep the `Closure`s alive as long as the game (here in the
same `StoredValue`), or JS calls into freed memory.

**Names must match exactly.** Write the JS methods in snake_case (`state_json`,
`set_move`) and the Rust declarations use the same names. A camelCase JS method
(`setMove`) needs `js_name`: `#[wasm_bindgen(method, js_name = setMove)] fn
set_move(this: &Game, dir: i32);`. A wrong name still builds; the page then fails
with `….set_move is not a function` (often followed by `closure invoked recursively
or after being dropped`, which is only a consequence). Call `dom_summary` to see
such errors.

```rust
//! The game screen: a <canvas> drawn by src/game.js (three.js), with the
//! score and buttons in Leptos.

use ak_ui::*;
use leptos::html::Canvas;
use leptos::prelude::*;
use wasm_bindgen::prelude::*;

// What src/game.js exports, as Rust can call it.
#[wasm_bindgen(module = "/src/game.js")]
extern "C" {
    type Game;

    #[wasm_bindgen(constructor)]
    fn new(canvas: &web_sys::HtmlCanvasElement, on_score: &Closure<dyn FnMut(u32)>) -> Game;

    #[wasm_bindgen(method)]
    fn pick(this: &Game, x: f64, y: f64) -> Option<String>;

    #[wasm_bindgen(method)]
    fn restart(this: &Game);

    #[wasm_bindgen(method)]
    fn state_json(this: &Game) -> String;

    #[wasm_bindgen(method)]
    fn dispose(this: &Game);
}

#[component]
pub fn GameScreen() -> impl IntoView {
    let score = RwSignal::new(0u32);
    let canvas = NodeRef::<Canvas>::new();
    // The game and its callback live as long as this screen. JS objects stay on
    // this thread, hence `new_local`.
    let game = StoredValue::new_local(None::<(Game, Closure<dyn FnMut(u32)>)>);

    canvas.on_load(move |el| {
        let on_score = Closure::new(move |points: u32| score.set(points));
        let g = Game::new(&el, &on_score);
        game.set_value(Some((g, on_score)));
        // Lets AgentKnife tell which 3D object the user points at.
        ak_bridge::set_hit_test(move |p| {
            game.with_value(|g| g.as_ref().and_then(|(g, _)| g.pick(p.x, p.y)))
                .map(|name| serde_json::json!({ "object": name }))
        });
        // Lets AgentKnife check the game: dom_summary shows this state.
        ak_bridge::set_state_reader(move || {
            game.with_value(|g| g.as_ref().map(|(g, _)| g.state_json()))
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_default()
        });
    });
    on_cleanup(move || {
        if let Some((g, _)) = game.try_update_value(Option::take).flatten() {
            g.dispose();
        }
    });
    let restart = move |_| {
        game.with_value(|g| {
            if let Some((g, _)) = g {
                g.restart();
            }
        })
    };

    view! {
        <main data-ak-id="game-screen" class="flex h-screen flex-col">
            <header data-ak-id="game-bar" class="flex items-center gap-4 p-3">
                <h1 data-ak-id="game-title" class="text-xl font-bold">"Cube Clicker"</h1>
                <span data-ak-id="score" class="text-lg">
                    {move || format!("Score: {}", score.get())}
                </span>
                <Button ak_id="restart-button" variant=ButtonVariant::Secondary on_click=restart>
                    "Restart"
                </Button>
            </header>
            <canvas node_ref=canvas data-ak-id="game-canvas" class="block min-h-0 w-full flex-1"></canvas>
        </main>
    }
}
```

The canvas gets its height from `h-screen` on `<main>`, through `flex-col` and
`min-h-0 flex-1`. Put `GameScreen` straight into `ToastHost` or the app (no box
around it), or give every element you add around it `flex min-h-0 flex-1
flex-col`: a canvas 0 pixels high is black, with no error (`dom_summary` shows the
canvas' `size`).

In `src/main.rs`: `mod game;` and show `<game::GameScreen />` (or mount it:
`leptos::mount::mount_to_body(game::GameScreen)` after `ak_bridge::start()`).

## Growing the game
- **Plan the states first.** Before coding a game, write a table in a comment at
  the top of `src/game.js`: each state (e.g. not started, playing, paused, over) ×
  each input (arrow keys, pause, restart, a click) → what happens. Decide the
  awkward cases up front (a key just after game over, turning back on oneself).
- **Report the state.** Put everything the rules depend on into `state_json()`
  (positions, direction, state, score, lives) and keep it current: it is the only
  way to check the game, since `dom_summary` cannot see inside the canvas. Add
  what a test needs to aim: what the player aims at now (`aim_on`) and the nearest
  target's distance and angle (`nearest`). For a game where the player flies or
  drives, something like:
  `{"state": "playing", "frame": 1840, "score": 3, "player": {"x": 0, "y": 20, "z": -100}, "speed": 36, "nearest": {"name": "ring-4", "dist": 45}, "last_event": "ring", "stunt": null}`
  (`frame` shows that it moves, `last_event` what just happened).
- **More data to Rust**: one callback per kind of news (`onScore`, `onGameOver`).
  Each is a `Closure` passed to the constructor and kept in the `StoredValue`.
  `Closure` cannot be copied and needs no `Rc`: create them all, pass references
  to the constructor, then move them into one struct kept with the game:
  ```rust
  struct Running {
      game: Game,
      // JS calls these as long as the game lives.
      _on_score: Closure<dyn FnMut(u32)>,
      _on_over: Closure<dyn FnMut()>,
  }
  let on_score = Closure::new(move |p: u32| score.set(p));
  let on_over = Closure::new(move || over.set(true));
  let game = Game::new(&el, &on_score, &on_over);
  running.set_value(Some(Running { game, _on_score: on_score, _on_over: on_over }));
  ```
  Numbers, strings and bools cross directly. **For several values at once**
  (speed, height, heading), JS sends one JSON string and Rust parses it with
  `serde_json` (already in Cargo.toml above). Not `JsValue` with
  `serde_wasm_bindgen`: that crate is not in the project.
  ```js
  // In frame(), a few times a second (not every frame):
  this.onTelemetry(JSON.stringify({ speed: Math.round(this.speed), altitude: Math.round(this.pos.y) }));
  ```
  ```rust
  #[derive(serde::Deserialize)]
  struct Telemetry {
      speed: u32,
      altitude: u32,
  }
  // Declared as `on_telemetry: &Closure<dyn FnMut(String)>` in the extern block.
  let on_telemetry = Closure::new(move |json: String| {
      if let Ok(t) = serde_json::from_str::<Telemetry>(&json) {
          speed.set(t.speed);
          altitude.set(t.altitude);
      }
  });
  ```
- **Rust to the game**: add a method (`set_level(n)`, `pause()`), declare it in the
  `extern "C"` block, call it through `game.with_value`.
- **Keyboard**: `window.addEventListener("keydown", this.onKey)` in the constructor,
  `removeEventListener` in `dispose()`. Keep pressed keys in a `Set` and read it in
  `frame()`. **Call `e.preventDefault()` for every key the game uses** (space,
  Enter, arrows): the button clicked last keeps the focus, so without it Space
  presses "Restart" again, and arrows scroll the page. `interact` `key` does the
  same (its answer's `clicked` names the button it pressed).
- **The view**: OrbitControls already turns (drag) and zooms (wheel): set
  `enableZoom`, `zoomSpeed`, `minDistance`/`maxDistance`, `maxPolarAngle` instead
  of adding a `wheel` listener (both would zoom). Buttons that turn or zoom the
  view change the camera once, when pressed, and `frame()` only calls
  `controls.update()`; setting `camera.position` every frame fights the controls:
  ```js
  // dTurn, dTilt in radians; zoom < 1 comes closer.
  turn_view(dTurn, dTilt, zoom) {
    const c = this.controls;
    const s = new THREE.Spherical().setFromVector3(this.camera.position.clone().sub(c.target));
    s.theta += dTurn;
    s.phi = Math.min(Math.max(s.phi + dTilt, 0.1), c.maxPolarAngle);
    s.radius = Math.min(Math.max(s.radius * zoom, c.minDistance), c.maxDistance);
    this.camera.position.setFromSpherical(s).add(c.target);
    c.update();
  }
  ```
  Read the view for `state_json()` with `controls.getAzimuthalAngle()`,
  `getPolarAngle()` and `getDistance()`.
- **Board games (match-3, cards, chess)**: the rules are turns, not frames.
  - One `state` (`"ready"`, `"busy"`, `"over"`) owned by the turn logic. Ignore
    clicks and keys while `"busy"` (pieces falling, a new shuffle). Only the
    function that runs the turn sets and clears it; helpers (`addGem`,
    `removeRow`) never set the state or start timers themselves.
  - A piece that starts moving or vanishing is tracked in exactly one list until
    it is done.
  - A new or shuffled board must be playable: no finished matches and at least
    one possible move (`has_move()`); deal again until both hold.
  - Share one geometry and a few materials between all pieces; free them once in
    `dispose()` (`scene.traverse`).
- **Buttons held down** (move while pressed, on screen or touch): Leptos
  `on:pointerdown` starts, `on:pointerup` and `on:pointerleave` stop; tell the game
  with a method (`set_move(-1)`, `set_move(0)`).
- **Saving** (high score, progress): send it to the backend from Rust (topics
  `ak-bridge`, `storage`), not `localStorage` alone.
- **Shapes and looks**: build from three.js geometries (`BoxGeometry`,
  `SphereGeometry`, `CylinderGeometry`, `PlaneGeometry`, `TorusGeometry`) and
  `MeshStandardMaterial({ color, roughness, metalness })`. Group parts with
  `THREE.Group`; parts that move (a propeller, a wheel) are kept to turn them in
  `frame()`. Build a vehicle with its nose toward -Z, the way a camera looks, and
  move it along its own -Z (`translateZ(-d)`). `lookAt` points an object's +Z at
  the target (only cameras turn their -Z), so after `plane.lookAt(p)` add
  `plane.rotateY(Math.PI)`:
  ```js
  const plane = new THREE.Group();
  plane.name = "plane";
  const paint = new THREE.MeshStandardMaterial({ color: 0xef4444 });
  const body = new THREE.Mesh(new THREE.CylinderGeometry(0.3, 0.2, 3, 12), paint);
  body.rotation.x = Math.PI / 2; // cylinders stand on Y: lay it along Z
  const wings = new THREE.Mesh(new THREE.BoxGeometry(4, 0.08, 0.8), paint);
  this.propeller = new THREE.Mesh(new THREE.BoxGeometry(1.2, 0.1, 0.05), paint);
  this.propeller.position.z = -1.55; // the nose is at -Z
  plane.add(body, wings, this.propeller);
  // In frame(): this.propeller.rotation.z += dt * 30;
  //             plane.translateZ(-speed * dt);
  ```
  Images for textures go in a folder copied like `vendor`
  (`<link data-trunk rel="copy-dir" href="assets" />`), loaded with
  `new THREE.TextureLoader().load("./assets/wood.png")`.
- **Which way a shape faces**: `TorusGeometry`, `RingGeometry`, `CircleGeometry`
  and `PlaneGeometry` lie in the XY plane, facing +Z (a plane is a wall, not a
  floor: `rotation.x = -Math.PI / 2` lays it down). To turn a ring so a path
  goes through its hole, from waypoint `a` to `b`:
  `ring.quaternion.setFromUnitVectors(new THREE.Vector3(0, 0, 1), b.clone().sub(a).normalize())`.
- **A HUD over the canvas** (gauges, score, buttons): the overlay lets the mouse
  through, and only the parts you click take it back. Otherwise an invisible
  overlay swallows every drag meant for the 3D view:
  ```html
  <main class="relative h-screen overflow-hidden">
    <canvas class="absolute inset-0 block h-full w-full touch-none"></canvas>
    <div class="pointer-events-none absolute inset-0 flex flex-col justify-between p-4">
      <header class="pointer-events-auto flex justify-between">…</header>
      <footer class="pointer-events-auto flex gap-2">…</footer>
    </div>
  </main>
  ```
- **Sound**: no sound files are needed: make sounds with the Web Audio API in
  `game.js`. Browsers keep sound off until the user presses a key or clicks, so
  create the `AudioContext` on the first input, not in the constructor:
  ```js
  // In the keydown / pointerdown handler: this.audio ??= new AudioContext();
  beep(freq, seconds, type = "sine") {
    if (!this.audio) return;
    const t = this.audio.currentTime;
    const osc = new OscillatorNode(this.audio, { type, frequency: freq });
    const gain = new GainNode(this.audio);
    gain.gain.setValueAtTime(0.2, t);
    gain.gain.exponentialRampToValueAtTime(0.001, t + seconds); // fade out, no click
    osc.connect(gain).connect(this.audio.destination);
    osc.start(t);
    osc.stop(t + seconds);
  }
  // A point: this.beep(880, 0.15); a crash: this.beep(80, 0.5, "sawtooth");
  ```
  A steady sound (an engine): one `OscillatorNode` started once, whose
  `frequency` follows the speed; stop it and `this.audio.close()` in `dispose()`.
- **Speed**: reuse geometries and materials for many copies (or `InstancedMesh`);
  never create objects inside `frame()`; `dispose()` what you remove from the scene.
- **Tuning**: put the numbers that set the difficulty at the top of `game.js` as
  constants (`MAX_ENEMIES`, `BITE_DAMAGE`, `BITE_COOLDOWN_S`, `ENEMY_SPEED`), so
  they are easy to find and change after a test.

## Models from files (glTF, OBJ, FBX)
`add_library("three")` also copies three.js's model loaders: `GLTFLoader` for
`.glb`/`.gltf` (the best choice: shapes, materials and animations in one file),
`OBJLoader` with `MTLLoader` for `.obj` + `.mtl`, and `FBXLoader` for `.fbx`.
- **Models come from the user**, never from the internet. They add them with "Add
  files…", into `assets/models/`. A `.gltf` with its `.bin` and pictures, or an
  `.obj` with its `.mtl` and pictures, needs all of them, side by side as they
  were exported; ask for the missing ones by name. Ask where a model comes from:
  someone else's model usually needs its author and license kept with the app;
  save the text the user gives you next to it as `assets/models/CREDITS.txt`.
- **Not supported**: models compressed with Draco or meshopt, and KTX2 textures.
  Loading then fails with an error naming `KHR_draco_mesh_compression`,
  `EXT_meshopt_compression` or `KHR_texture_basisu`. Ask the user for an export
  without compression (Blender: File → Export → glTF 2.0, Compression off).
- **Keep models small** (a few MB): the preview loads everything again after each
  change. A model of 50 MB makes every change slow; ask for a lighter one.
- **Load it in the background, and keep a stand-in.** The game runs at once with
  a simple shape in the model's place; the model replaces it when it arrives.
  Report the state in `state_json()` and on screen (a callback, like `onScore`):
  "Loading the model… 40%", or why it failed (the game goes on with the stand-in).
  Write the file name as the files pane shows it, through `encodeURI` (spaces and
  non-English names).

```js
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";

// In the constructor. onModel is a callback from Rust, like onScore.
this.modelState = "loading";
this.mixer = null;
new GLTFLoader().load(
  encodeURI("./assets/models/plane.glb"),
  (gltf) => {
    if (this.disposed) return; // the screen went away while loading
    const model = gltf.scene;
    fitTo(model, 3); // 3 units across, centered where the stand-in was
    model.name = "plane"; // its parts keep their own names (pick shows them)
    this.plane.remove(this.standIn); // the Group the game moves
    this.plane.add(model);
    if (gltf.animations.length > 0) {
      this.mixer = new THREE.AnimationMixer(model);
      this.mixer.clipAction(gltf.animations[0]).play();
    }
    this.modelState = "ready";
    this.onModel("ready");
  },
  (e) => {
    if (e.total) this.onModel(`loading ${Math.round((100 * e.loaded) / e.total)}`);
  },
  (err) => {
    this.modelState = "failed";
    this.onModel(`failed: ${err?.message ?? err}`);
  },
);
// In frame(dt): this.mixer?.update(dt);
// In state_json(): model: this.modelState,

// Scales `object` so its longest side is `size`, centered on its parent's origin.
function fitTo(object, size) {
  const box = new THREE.Box3().setFromObject(object);
  const longest = Math.max(...box.getSize(new THREE.Vector3()).toArray()) || 1;
  object.scale.multiplyScalar(size / longest);
  box.setFromObject(object);
  object.position.sub(box.getCenter(new THREE.Vector3()));
}
```
- **Which way it faces**: models are exported facing anywhere. Look at the
  screenshot (or `pick` its nose), then turn the model inside its group once
  (`model.rotation.y = Math.PI`), so the group still moves along -Z.
- **OBJ with MTL**: load the materials first; the result is a `Group` (not
  `.scene`):
  ```js
  import { MTLLoader } from "three/addons/loaders/MTLLoader.js";
  import { OBJLoader } from "three/addons/loaders/OBJLoader.js";

  const dir = encodeURI("./assets/models/");
  new MTLLoader().setPath(dir).load("chair.mtl", (materials) => {
    materials.preload();
    new OBJLoader().setMaterials(materials).setPath(dir).load("chair.obj", onLoad, onProgress, onError);
  }, undefined, onError);
  ```
- **FBX**: `new FBXLoader().load(url, (group) => …)`; its animations are in
  `group.animations`. FBX files are often in centimeters: `fitTo` takes care of it.
- **Animations**: `gltf.animations` lists the clips by `name` (walk, run, idle).
  Play one with `mixer.clipAction(clip).play()`; to switch, `fadeOut` the old
  action and `reset().fadeIn(0.3).play()` the new one.
- **Freeing it**: `dispose()` sets `this.disposed = true`, calls
  `this.mixer?.stopAllAction()` and frees every texture of every material (models
  bring normal and roughness maps, not only `map`), as in the `dispose()` above.

## Shooters and enemies
- **Crosshair**: a Leptos element over the canvas, not a 3D object: wrap the canvas
  in `<div class="relative …">` and add `<div data-ak-id="crosshair"
  class="pointer-events-none absolute left-1/2 top-1/2 -translate-x-1/2
  -translate-y-1/2 text-white">"+"</div>`.
- **Things a shot passes through** (bullets, effects, helpers): `obj.raycast = () =>
  {};` takes them out of `pick` and other rays.
- **Aiming** with a camera behind and above the player: a ray from the middle of
  the screen hits the ground in front of the player first. Aim from the player
  instead: the target is the nearest enemy whose direction is within a few degrees
  of where the player faces, measured on the ground plane:
  ```js
  aimed() {
    const facing = new THREE.Vector3(0, 0, -1).applyQuaternion(this.player.quaternion).setY(0);
    let best;
    for (const e of this.enemies) {
      const to = e.position.clone().sub(this.player.position).setY(0);
      if (facing.angleTo(to) < AIM_TOLERANCE && (!best || to.length() < best.dist))
        best = { enemy: e, dist: to.length() };
    }
    return best?.enemy;
  }
  ```
- **Fast bullets** move farther in one frame than a "close enough" distance, so
  they fly through. Each frame, steer toward the target's current position, and
  count a hit when this frame's step reaches it:
  ```js
  const to = target.position.clone().sub(bullet.position);
  const step = BULLET_SPEED * dt;
  if (step >= to.length()) hit(target);
  else bullet.position.add(to.setLength(step));
  ```
- **Enemies** must keep apart from the player and each other (push them back to a
  minimum distance each frame), or they pile up, hide the camera and block shots.
  Limit the damage too: one bite at a time, with a cooldown.

## Checking the preview
- Changing `src/game.js` rebuilds and reloads the preview like Rust code.
- JS errors do not stop the build. After every change call `dom_summary`: it shows
  the game state from `state_json()` and ends with the page's errors (e.g. a
  misspelled three.js name or a method name without `js_name`, with file and
  line). If the whole page is blank, check the import map and `vendor/three/`.
- `pick` + `set_hit_test` make "this one" in the user's message name the 3D object
  under the pointer.

## Testing a game that runs by itself
The game moves every frame; test it by acting and then watching its state.
- **Act**: `interact` `click` sends pointer down/up, mouse down/up and click, like a
  mouse. `hold` presses an element for `ms` (buttons that act while held). `key`
  presses a key on the page (`"ArrowLeft"`, `" "`, `"a"`), held for `ms`.
- **Watch**: `interact` `read_state` with `wait_ms` waits first; with `samples` it
  reads several times over `wait_ms` (e.g. `samples: 5, wait_ms: 4000` to see a
  ball travel and bounce). Do not sleep with `run_command`.
- Put what the rules depend on, and a frame or time counter, into `state_json()`
  so the readings show movement.
- 3D pieces have no `data-ak-id`, so `interact` cannot click them. Let the game
  find a move itself and say how to make it: a `hint()` that finds a valid move
  (on the visible layer first), and in `state_json()` the move and the keys that
  make it in the current view (`"hint": {"from": [2, 3], "keys": ["ArrowUp", " "]}`).
  Then press those keys and check the result (score, the board). This is far more
  reliable than working out positions from the camera.
- After a change the page reloads and the game starts over: an element can be
  missing (the start screen is back). Call `dom_summary` and do it again.
