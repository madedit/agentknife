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
ak/ak-bridge, ak/ak-ui  AgentKnife's libraries (do not edit; AgentKnife updates them;
                      read their source here to check a component's props)
```

## How the preview works
- AgentKnife runs the screens with `trunk serve --features dev` and the backend with
  `cargo run --features dev`. The preview reloads by itself after each change.
- The screens reach the backend only through `ak_bridge::invoke` (topic `ak-bridge`).
  It works in the preview and in the installed app.
- A backend change restarts the backend; a build error shows in the preview. Fix
  build errors before anything else.
- **Only in the real window** (the user's "run in a window"), not in the preview
  and not with `interact`: the system's open/save/folder dialogs, files dragged in
  from the computer, tray icons, notifications. Give such features a fallback the
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
- Use ak-ui components (topic `ak-ui`) before writing your own.
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
- Do not change `[features]`, the `ak/` folder, `index.html`'s trunk links (adding
  what topic `three-d` says is fine) or
  `tauri.conf.json`'s `build` section: the preview and the installer depend on them.
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

## The window
`tauri.conf.json` → `app.windows[0]`: `title`, `width`, `height`, `minWidth`,
`minHeight`, `resizable`. Keep `"label": "main"` and `"visible": false` (the app shows
the window itself when it starts).
