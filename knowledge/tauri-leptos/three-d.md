# 3D games and scenes with three.js

For 3D (games, product viewers, 3D charts) use **three.js 0.186.1**, which ships with
AgentKnife. The 3D part is a small JavaScript module drawing on a `<canvas>`; menus,
scores and buttons stay in Leptos. No npm, no CDN links, no downloads: they do not
work here.

## 1. Add the library
Call `add_library` with `"three"`. It copies into `vendor/three/`:
`three.module.js`, `three.core.js` (three.js itself),
`addons/controls/OrbitControls.js` (drag to turn the view) and `LICENSE` (keep it).
Other three.js addons (model loaders, effects) are not available.

## 2. index.html
Add the import map and the `copy-dir` link **above** the `rel="rust"` link. Do not
change the other trunk links.

```html
<script type="importmap">
  {
    "imports": {
      "three": "./vendor/three/three.module.js",
      "three/addons/": "./vendor/three/addons/"
    }
  }
</script>
<link data-trunk rel="copy-dir" href="vendor" />
```

## 3. Cargo.toml (screens)
```toml
serde_json = "1.0.154"
web-sys = { version = "0.3.106", features = ["HtmlCanvasElement"] }
```

## 4. src/game.js: the 3D part
One class. The constructor takes the canvas and callbacks for what Rust must know
(score, game over); methods are what Rust can ask for. Give every object the player
can hit a `name`. Clean everything up in `dispose()`.

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

  // The name of the object at (x, y) in canvas pixels, or undefined.
  pick(x, y) {
    const ndc = new THREE.Vector2(
      (x / this.canvas.width) * 2 - 1,
      -(y / this.canvas.height) * 2 + 1,
    );
    this.raycaster.setFromCamera(ndc, this.camera);
    const hit = this.raycaster.intersectObjects(this.scene.children, true)[0];
    return hit?.object.name || undefined;
  }

  restart() {
    this.score = 0;
    this.onScore(0);
  }

  dispose() {
    this.renderer.setAnimationLoop(null);
    this.resize.disconnect();
    this.canvas.removeEventListener("pointerdown", this.onDown);
    this.canvas.removeEventListener("pointerup", this.onUp);
    this.controls.dispose();
    this.cube.geometry.dispose();
    this.cube.material.dispose();
    this.renderer.dispose();
  }
}
```

## 5. src/game.rs: the screen
`#[wasm_bindgen(module = "/src/game.js")]` declares what Rust may call (the path is
from the project folder). Keep the `Closure`s alive as long as the game (here in the
same `StoredValue`), or JS calls into freed memory.

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

In `src/main.rs`: `mod game;` and show `<game::GameScreen />` (or mount it:
`leptos::mount::mount_to_body(game::GameScreen)` after `ak_bridge::start()`).

## Growing the game
- **More data to Rust**: one callback per kind of news (`onScore`, `onGameOver`).
  Each is a `Closure` passed to the constructor and kept in the `StoredValue`.
  Numbers, strings and bools cross directly; for more, pass a JSON string and parse
  it with `serde_json` in Rust.
- **Rust to the game**: add a method (`setLevel(n)`, `pause()`), declare it in the
  `extern "C"` block, call it through `game.with_value`.
- **Keyboard**: `window.addEventListener("keydown", this.onKey)` in the constructor,
  `removeEventListener` in `dispose()`. Keep pressed keys in a `Set` and read it in
  `frame()`.
- **Saving** (high score, progress): send it to the backend from Rust (topics
  `ak-bridge`, `storage`), not `localStorage` alone.
- **Shapes and looks**: build from three.js geometries (`BoxGeometry`,
  `SphereGeometry`, `CylinderGeometry`, `PlaneGeometry`, `TorusGeometry`) and
  `MeshStandardMaterial({ color, roughness, metalness })`. Group parts with
  `THREE.Group`. Images for textures go in a folder copied like `vendor`
  (`<link data-trunk rel="copy-dir" href="assets" />`), loaded with
  `new THREE.TextureLoader().load("./assets/wood.png")`.
- **Speed**: reuse geometries and materials for many copies (or `InstancedMesh`);
  never create objects inside `frame()`; `dispose()` what you remove from the scene.

## Checking the preview
- Changing `src/game.js` rebuilds and reloads the preview like Rust code.
- JS errors do not stop the build. When the canvas stays empty, call `dom_summary`:
  it ends with the page's errors (e.g. a misspelled three.js name, with file and
  line). If the whole page is blank, check the import map and `vendor/three/`.
- `pick` + `set_hit_test` make "this one" in the user's message name the 3D object
  under the pointer.
