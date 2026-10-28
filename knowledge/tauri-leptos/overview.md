# Project layout and workflow

The app is all Rust: a **Leptos** user interface compiled to WebAssembly (shown in a
web view) and a **Tauri 2** backend that runs on the computer. Never add Node.js, npm
or JavaScript frameworks. The one exception is 3D: three.js, added with
`add_library` (topic `three-d`).

```text
Cargo.toml            the screens' package (app-ui) and the workspace
index.html            page shell; Trunk builds the screens from it
style/input.css       Tailwind entry (do not remove the @source lines)
src/main.rs           the screens (Leptos components)
src-tauri/
  Cargo.toml          the backend package (app)
  tauri.conf.json     app name, window size, installer settings
  capabilities/       what the window may use
  icons/              app icons
  src/main.rs         backend commands (#[tauri::command])
ak/ak-bridge, ak/ak-ui  AgentKnife's libraries (AgentKnife updates them; the tools
                      refuse changes there; read their source to check a
                      component's props)
assets/               the user's own files, once they add some: images/, models/,
                      sounds/, fonts/, files/
```

## Which topic to read
| The user asks for | Topic |
|---|---|
| buttons, fields, lists, dialogs, tabs | `ak-ui`, `leptos-components` |
| saving what they enter | `storage` (the whole app: `example-todo`) |
| colours, dark mode, a theme, fonts, Chinese text, the layout in a small window | `tailwind` |
| charts, printing, PDF | `reports` |
| importing from Excel, backups, another computer | `import-backup` |
| thousands of rows, search, sort, pages | `long-lists` |
| Ctrl+Z, saving by itself | `undo-autosave` (plan it before the editor) |
| dates, months, times | `date-time` |
| dragging cards or rows | `drag-drop` |
| diagrams, maps, a drawing board, a big grid | `canvas` |
| text with bold and colours, find and replace | `rich-text` |
| pictures, thumbnails, choosing a file or folder | `images-dialogs` |
| notifications, tray icon, starting with Windows, a log | `desktop` |
| data from the internet | `network` |
| music, sound effects, a music player | `sound` |
| a 3D game or scene, 3D models | `three-d` |
| a build error | `troubleshooting` |

Not sure which: `read_knowledge` with `search` (a word or two, e.g. `dark`)
lists every line in the pack that has them, with its topic.

## The user's pictures, models, sounds and fonts
- The user adds them with "Add files…" (or by dropping them on AgentKnife's
  window): AgentKnife copies them into `assets/<kind>/` and puts
  `<link data-trunk rel="copy-dir" href="assets" />` into `index.html`, so the app
  loads them as `./assets/images/cat.png`, in the preview and once installed. You
  hear of each added file with the user's next message.
- 3D models (`.glb`, `.gltf`, `.obj`, `.fbx`) in `assets/models/` are loaded with
  three.js's loaders: topic `three-d`, "Models from files".
- Music and sound effects in `assets/sounds/` (ask for MP3): topic `sound`.
- Never download pictures, sounds, fonts or 3D models from the internet, and do not
  rename or move the user's files. When the app needs one that is not there, ask
  the user to add it with "Add files…"; until then draw it (a colored box, an
  icon from ak-ui, a three.js shape) so the app works without it.

## How the preview works
- AgentKnife runs the screens with `trunk serve --features dev` and the backend with
  `cargo run --features dev`. The preview reloads by itself after each change.
- The screens reach the backend only through `ak_bridge::invoke` (topic `ak-bridge`).
  It works in the preview and in the installed app.
- A backend change restarts the backend; a build error shows in the preview. Fix
  build errors before anything else.
- **Only in the real window** (the user's "run in a window"), not in the preview
  and not with `interact`: the system's open/save/folder dialogs, files dragged in
  from the computer, tray icons, notifications, printing. Give such features a fallback the
  preview can use (a text field for a path, a list to pick from) and tell the user
  to try the rest in the real window. The fallback must do everything the real
  path does (save, add to the recent files…): make both call the same function,
  and check its effects in the preview.

## Rules for every change
- Read a file before changing it; change as little as needed.
- Build often: write a screen a part at a time and check `build_status` after
  each part. In a long `view!` one mistake hides the others.
- `run_command` on Windows is `cmd`: no `grep`/`head`; search output with
  `findstr /n /c:"text" file`.
- Use ak-ui components (topic `ak-ui`) before writing your own; `class` adds to
  one's look, a rule for its `ak-…` marker class in `style/input.css` changes
  them all. When one does not fit, write your own component in `src/`. Never
  change `ak/`, not with a command or a script either.
- Before a feature, read its topic ("Which topic to read" above); keyboard use
  and readable colours are in `tailwind`.
- Data from the internet: try the service with `curl` in `run_command` first,
  then fetch it (topic `network`).
- Every meaningful element gets a stable `data-ak-id` (topic `data-ak-id`).
- Text the user sees is in the user's language. Errors returned by commands are shown
  to the user, so write them plainly too.
  Every label in that language and its script (Traditional Chinese: 縮小, 導覽, not
  缩小, 导航), with no English or other placeholders left behind; read the labels
  over once when a screen is done.
- Keep data the user enters: save it (topic `storage`). Sample content shown on
  the first start is saved too, so the file matches what the user sees.
- Splitting code: put screens in `src/` modules (`mod todos;` in `src/main.rs` and
  `src/todos.rs`) and backend code in `src-tauri/src/` modules.
- Do not change `[features]`, the `ak/` folder, `index.html`'s trunk links or
  `tauri.conf.json`'s `build` section: the preview and the installer depend on them.
  Two additions to `index.html` are expected, never removed: the import map and
  the `vendor` copy-dir link for three.js (topic `three-d`, `add_library` says
  them), and the `assets` copy-dir link AgentKnife adds with the user's files.
- New crates: only when really needed, with a fixed version. Prefer the versions the
  project already has in `Cargo.lock` (already downloaded, known to fit):
  `serde_json = "1.0.154"`, `js-sys = "0.3.106"`, `web-sys = "0.3.106"` (add the
  features you use), `chrono = "0.4.45"` (backend), `uuid = "1.28.0"`,
  `regex = "1.13.1"`. For another crate, search `Cargo.lock` for its version first;
  a crate the project never used is not there. Then look in the libraries folder
  (its path is in your instructions): `list_files` on its `index.crates.io-…`
  folder, or `search_files` for `^name = "rfd"` in it. A folder such as
  `rfd-0.16.1` means that version is downloaded and builds without internet.
  Crates must work in WebAssembly if the screens use them.
- Crates people often reach for that are not here, and what to use instead:
  - `gloo-timers`, or `tokio` in the screens: `set_timeout` and
    `set_interval_with_handle` from `leptos::prelude` (topic `leptos-reactivity`).
  - `serde_wasm_bindgen`: send one JSON string and parse it with `serde_json`
    (topic `three-d`).
  - `rand` in the screens: `js_sys::Math::random()` (a number from 0 to 1).

## The window
`tauri.conf.json` → `app.windows[0]`: `title`, `width`, `height`, `minWidth`,
`minHeight`, `resizable`. Keep `"label": "main"` and `"visible": false` (the app shows
the window itself when it starts).
