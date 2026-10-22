# AGENTS.md — AgentKnife

Rules for any AI agent working on this repository. Read this file fully before every session.
Product requirements and user flows live in [`docs/SPEC.md`](docs/SPEC.md) (Traditional Chinese).

**Conflict rule:** if this file and SPEC disagree, **SPEC wins**. Implement per SPEC and add a `Blocked` entry in `plan/STATUS.md` saying "AGENTS.md needs update: <section>" for the coordinator to fix. There are two exceptions:
- A section of this file marked `Owner-approved override (YYYY-MM-DD)` wins over SPEC.
- A conflict with the fixed stack in §2 is an owner decision. Stop and ask.

## 1. What we are building

AgentKnife is a desktop IDE that lets **non-programmers** build apps by talking to an AI agent. The agent does almost all the work and guides the user in plain language. Core UI:

- **Center:** WYSIWYG live preview of the user's app (device frames: desktop / phone / tablet; code editor as a tab).
- **Right:** chat with the AI. Shows plain-language progress steps, collapsible thinking, and results.
- **Left:** file manager plus a plain-language checkpoint timeline.
- **Simple mode** (default) shows only the preview and the chat.

Core flow: connect an AI → AI greets the user and suggests next steps → `doctor` checks and installs the toolchain → the user asks for features and the preview updates → AgentKnife bundles an installer for the host platform and the AI explains how to install it.

## 2. Tech stack (fixed — do not change without owner approval)

| Area | Choice |
|---|---|
| AgentKnife shell | Tauri 2 (Rust) |
| AgentKnife UI | Leptos (CSR, compiled to WASM, built with Trunk) |
| Only JS allowed | CodeMirror 6, as a vendored pre-built ESM bundle (see below) |
| Async | `tokio` |
| Errors | `thiserror` in library crates, `anyhow` only in `ak-app` |
| Logging | `tracing` |
| Serialization | `serde` |
| i18n | `fluent` (zh-TW and en from day one) for fixed strings (see §5) |
| Git (embedded) | `gix` (preferred) or `git2` — users never install git |
| Secrets | `keyring` (OS keychain) |
| Updates | `tauri-plugin-updater` + GitHub Releases (ask the user, never force) |

The user's app stack is the MVP **stack profile** `tauri-leptos`: Tauri 2, Leptos CSR, Trunk, and the Tailwind standalone CLI.

**No Node.js anywhere.** That covers AgentKnife, generated apps, and CI. A `package.json` or `node_modules` must never appear in this repo. Plain JS files are fine where vendored (CodeMirror below; libraries for generated apps, see "JS libraries for generated apps").

### Single source of versions: `versions.toml`

`versions.toml` at the repo root is the only place versions are decided. It covers Tauri, Leptos, Trunk, wasm-bindgen, the Tailwind CLI, CodeMirror, and the knowledge-pack version.

`cargo xtask ci` fails if any of these drift from `versions.toml`:
- workspace `Cargo.toml` dependencies
- `templates/*`
- `knowledge/` metadata
- vendored bundle hashes

To upgrade, edit `versions.toml` and update everything that follows from it **in one change**.

### CodeMirror without Node

- `cargo xtask vendor-codemirror` fetches one esm.sh bundle per module listed in `xtask/src/vendor.rs` into `crates/ak-shell/vendor/codemirror/`. Versions come from `versions.toml` `[vendor.codemirror]`.
- Every other vendored module is passed as `external`. Shared packages (`@codemirror/state`, `@lezer/common`, …) therefore exist **once**. CM6 breaks with duplicate instances, so never bundle them separately.
- esm.sh helper modules (`/node/*.mjs` polyfills) are vendored byte-for-byte under `_esm/`.
- `importmap.json` maps every specifier to its file. `ak-shell` must load it as `<script type="importmap">`.
  - Browsers have no external import maps, so it is inline in `crates/ak-shell/index.html` between markers, with every file under `./codemirror/` (Trunk's `copy-dir` serves the folder there). `cargo xtask vendor-codemirror index` rewrites it; ci (step `vendor`) fails when it is stale.
  - `ak_shell::interop` imports the modules with wasm-bindgen `module = "<bare specifier>"` (resolved by the import map) and only takes plain values (`EditorView`, `keymap`, `Prec`, language functions); the rest goes through `Reflect`.
- The xtask also writes `manifest.toml` (vendored versions) and `SHA256SUMS`. Everything is **committed**, and `.gitattributes` marks the directory `-text`.
- CI never downloads anything. It checks four things:
  - the hashes
  - manifest against `versions.toml`
  - every import resolves through the import map
  - no stray imports
- To upgrade or add a language: edit `versions.toml` (and the module list in `vendor.rs`), then re-run the xtask.

### JS libraries for generated apps

- Apps get JS only through the knowledge pack's `[[library]]` entries (id, version, summary, `dir` under the pack, `target` in the project). Today: three.js (`three`) for 3D, with `addons/controls/OrbitControls.js`.
- `cargo xtask vendor-libs` takes each library's files from its npm tarball (checked against the registry's sha512 `integrity`) into `knowledge/<stack>/libraries/<id>/` and writes `SHA256SUMS`. Versions live in `versions.toml` `[vendor.libraries]`; the file list in `xtask/src/libs.rs`.
- CI (step `libs`) checks the hashes, a version mark inside the files, and that `pack.toml` agrees with `versions.toml`. It downloads nothing. `.gitattributes` keeps the files byte-for-byte.
- ak-preview build.rs embeds the files (`KnowledgePack::libraries`, without `SHA256SUMS`). The agent's `add_library` tool copies them into `<target>/` (unchanged files untouched, changed ones restored); the policy treats it like any file change. It is registered only when the pack has libraries.
- The topic (`three-d`) has the app add an import map and `copy-dir` to `index.html`, put its 3D code in `src/game.js` and call it from Rust with `#[wasm_bindgen(module = "/src/game.js")]`. check-knowledge compiles its `js` and `rust` blocks as `src/game.js` and `src/game.rs`.

Out of scope until Phase 3: wasmtime plugins and an egui profile. Keep the abstractions open, but do not implement them.

## 3. Repository layout

```
crates/
  ak-types/      Shared DTOs (IPC payloads, events), i18n API (fluent wrapper), StackProfile trait.
  ak-app/        Tauri binary: commands, windows, wiring. The only AgentKnife crate that depends on tauri
                 (ak-bridge may too, because it ships inside generated Tauri apps).
  ak-shell/      AgentKnife's Leptos UI (WASM). Talks to ak-app via Tauri IPC only.
    vendor/codemirror/  Vendored CM6 bundle + SHA256SUMS (generated by xtask, committed).
  ak-agent/      Agent loop, tool definitions, permission policy, context compaction.
    prompts/system.md  Versioned product prompt (`prompt-version: N` header; bump it on every change).
  ak-providers/  LLM providers: Anthropic, OpenAI, OpenAI-compatible (Ollama, LM Studio, OpenRouter…).
  ak-doctor/     Toolchain detection + isolated install. Deterministic; no LLM calls.
  ak-project/    Projects, .agentknife/ files, git checkpoints + GC, chat history.
  ak-preview/    Trunk dev-server management, bridge host, inspector protocol, screenshots.
  ak-bundle/     Host-platform bundling + install instructions.
  ak-ui/         Component kit used by generated apps (Leptos + Tailwind).
  ak-ui-gallery/ Every ak-ui component on one page (trunk serve; dev only).
  ak-bridge/     Crate embedded in generated apps: WebSocket invoke, inspector, control channel.
templates/
  agentknife/    Stack-independent .agentknife/ skeleton (PROJECT.<locale>.md, STATUS.md, tasks.md,
                 gitignore). Embedded by ak-project with include_str.
  tauri-leptos/  MVP stack-profile template.
knowledge/       Version-pinned docs/examples the agent reads on demand (Leptos, Tauri 2, ak-ui).
xtask/           `cargo xtask ci`, `vendor-codemirror`, and other dev tasks.
versions.toml    Single source of truth for pinned versions.
docs/SPEC.md     Product spec (zh-TW).
plan/            STATUS.md and execution-plan.md (format in §8).
```

### Layering rules (enforced by `cargo xtask ci`)

- `ak-agent`, `ak-providers`, `ak-doctor`, `ak-project`, `ak-preview`, and `ak-bundle` must **not** depend on `tauri`, `leptos`, or `ak-shell`. They expose plain Rust APIs, and `ak-app` adapts them to Tauri.
- `ak-types` is the bottom layer. It depends on no other `ak-*` crate and never on `tauri` or `leptos`. It must compile for both native and `wasm32-unknown-unknown`.
- `ak-shell` may depend on `ak-types` and no other `ak-*` crate. It never imports core crates directly.
- Core crates may depend on `ak-types`.
- `ak-ui` and `ak-bridge` are consumed by generated apps and must not depend on any other `ak-*` crate.
- Stack-specific logic goes behind a `StackProfile` trait, which covers template, knowledge pack, doctor requirements, preview strategy, and bundle strategy. Do not hard-code `tauri-leptos` outside its profile implementation.
  - The trait and its plain-data plans live in `ak_types::stack`: Template (embedded files, values to replace, renames, `instantiate`), KnowledgePack, doctor requirement ids (`DoctorReport::ready_for`), PreviewPlan and BundlePlan.
    - The plans hold CommandSpecs for toolchain tools, with `{port}`, `{bridge_port}`, `{bridge_token}` and `{bridge_origin}` placeholders and env to set or remove.
    - They also say how to tell readiness, which paths restart the backend, and, per host OS, how the release is built and what the user gets (an archive AgentKnife packs, or an installer the build makes).
  - Implementations live in `ak_preview::profiles` (`default_profile`, `by_id`); `tauri_leptos.rs` embeds `templates/tauri-leptos` via build.rs. Projects store the stack id in project.json; `Project::scaffold(&dyn StackProfile)` writes the starter app and never overwrites existing files.
  - `cargo xtask ci` (step `stack`) rejects the literal stack name anywhere under crates/ except that profile, its build.rs and its tests.

## 4. Key design contracts

- **Agent:** AgentKnife has its own agent loop. Providers sit behind one trait. On connect, check that the model supports tool calling, and warn in plain language if it does not (typical for small local models). API keys go only in the keychain, never in files or logs.
  - Keys: `ak_providers::credentials`. `Keychain` (service `AgentKnife`) is the only persistent store. The account is the provider id, or `<provider>@<base_url>` for a custom endpoint, so a local server never receives a cloud key. Never add a file or env fallback in the app; env vars are for dev examples only.
  - Keys travel as `ApiKey`, which has no `Serialize` or `Display`, a redacted `Debug`, and `expose()` only for the request header. Error messages from servers are scrubbed with `ApiKey::redact`. Keyring errors never show stored bytes.
  - OpenAI protocols: `OpenAiCompatConfig::api` (`WireApi`). OpenAI itself uses the Responses API, because its current models call tools only there; every other flavor defaults to Chat Completions.
    - Responses requests are stateless (`store: false`). The replay is the whole `output` array, echoed verbatim and in order, which keeps encrypted reasoning, item ids and `phase`. A Chat Completions replay is an object, so the two never mix.
    - OpenAI model capabilities come from the dated table in `openai::responses` (`MODEL_TABLE_DATE`). When OpenAI changes its models, update the table and its date in the same change.
- **AI reply language:** the agent replies in the language of the user's most recent message. If that cannot be determined, it uses the UI language. The rule lives in the system prompt (`ak_agent::prompt`), first and explicit. The system prompt is built once per conversation and never changes between turns, because providers bind reasoning signatures and the prompt cache to it. So the rule is static, with the UI language named as the fallback.
- **Permissions:** reads, writes, builds, and runs inside the project folder are automatic. Downloads or installs, access outside the project, and deletions need a plain-language confirmation.
  - Implemented by `ak_agent::policy::ProjectPolicy`, the agent's default gate. Use `AllowAll` only in tests.
  - Reference folders (`ProjectPolicy::with_reference_dirs`, `AgentConfig::reference_dirs`; ak-app passes the toolchain's `cargo/registry`, `cargo/git` and `rustup/toolchains`, never the whole toolchain folder, which may hold cargo credentials): the read tools take absolute paths there (`tools::path::resolve_read`), and commands that only read (`READERS`: type, cat, findstr, rg, Get-Content, …, without `>` redirection) may name them without asking. Writing, deleting and every other folder outside the project still ask; the write tools still use `resolve`.
  - Path escapes, writes into `.git/`, and destructive commands (format, `rm -rf /`, shutdown…) are refused outright.
  - Confirmation reasons are a structured `ConfirmReason { kind, detail }`, rendered by the UI through fluent (`ConfirmKind::message_id`). Never send English sentences to the UI.
  - When adding a tool, teach the policy about it; unknown tools ask.
- **Plan-before-big-change:** small edits are applied directly. Large features first get a 3–5 bullet plan with a time and cost estimate, and wait for the user to click "Start". This is configurable.
  - Implemented as the loop-owned `propose_plan` tool, which emits `AgentEvent::PlanProposed` and asks a `PlanReviewer` (Start / Revise / Cancel).
  - `PlanMode`: `Auto` (the AI decides), `AlwaysAsk` (change tools are refused before approval), `NeverAsk`.
  - With no UI attached, plans are cancelled.
- **Auto-fix:** on a build or run error, the agent retries up to 3 times (configurable). After that it stops and offers: try another approach, roll back to a checkpoint, or show technical details.
  - Counts consecutive failed `run_command` results; a success resets the count.
  - Exceeding `max_fix_attempts` ends the turn as `TurnOutcome::Stuck`, and the UI shows the three options via fluent `stuck-*`.
- **Progress display:** every step shows a plain-language label. Labels for deterministic steps (doctor, bundle, checkpoint) come from fluent, and labels for agent tool calls are AI-generated (see §5).
  - The agent loop adds a required `label` argument to every tool schema and strips it before the tool runs. Tools must never define their own `label` argument. Raw commands and output go under "Details". Thinking is collapsed by default.
- **Usage:** show token and cost estimates for cloud providers, with an optional daily cap and a warning before it is reached.
  - `ak_agent::usage`: cost comes only from the dated price table (`ANTHROPIC_PRICES_AS_OF`) for models on their official endpoints. Local endpoints are free; everything else is "unknown". Never guess a price.
  - Per-day totals live in `<data_dir>/AgentKnife/usage.json` (local calendar day, shared by all projects); session totals live in the Agent.
  - At 80% of the cap a `BudgetWarning` is sent once a day. At 100% the user is asked (`ConfirmKind::DailyCap`) once a day; declining ends the turn as `OverBudget`.
  - When prices change, update the table and its date in the same change.
- **Doctor:** detection and installation are deterministic Rust code, and the AI only narrates. Install Rust, targets, Trunk, wasm-bindgen, tauri-cli, and Tailwind CLI in AgentKnife's own directory (custom `RUSTUP_HOME`/`CARGO_HOME`). For system-level dependencies (MSVC Build Tools, WebView2, Xcode CLT, Linux webkit packages), guide the user step by step. Installs must be idempotent and resumable.
  - `ak_doctor::Toolchain` lives under `<data_dir>/AgentKnife/toolchain/`, with `rustup/`, `cargo/` and `bin/`. Tools are always called by full path. Pinned versions come from the embedded `versions.toml`.
  - Anything that runs rustup or cargo must use `Toolchain::env()`, which sets `RUSTUP_AUTO_INSTALL=0` and `RUSTUP_TOOLCHAIN`. Without it, rustup silently installs a missing toolchain. The one exception is installing Rust itself (rustup-init, `rustup toolchain install`), which uses the private homes without `RUSTUP_TOOLCHAIN`. `SystemProbe` drops inherited `RUSTUP_TOOLCHAIN`, `RUSTUP_HOME`, `CARGO_HOME`, `CARGO`, `RUSTC` and `RUSTDOC`, so the parent process (for example `cargo run`) cannot change which toolchain is used.
  - Detection goes through the `Probe` trait, so it can be tested for all OSes on any host with canned outputs.
  - `ak_doctor::Installer` installs only the isolated items that `check` did not find Ok, then checks again. It uses prebuilt downloads where `assets` has them, and falls back to `cargo install --locked` (Tailwind has no fallback). Each download must match its sha256 in `versions.toml` `[downloads]`; after changing a tool version, run `cargo xtask pin-downloads` (CI fails on missing or stale keys).
  - System dependencies: `ak_doctor::system::guides` turns the missing items into `SystemGuide`s, which contain plain-language fluent steps (`doctor-guide-*`) and end with "Check again".
    - On Windows, official installers run through winget, which verifies Microsoft's installer hash; without winget, the guide opens the official download page.
    - On macOS, `xcode-select --install` is used.
    - On Linux there is no action. The guide gives one copyable command for the detected distribution, because sudo is typed by the user, never by AgentKnife.
    - `system::launch` runs only after the user has confirmed (`doctor-confirm-<action>`), and it opens only the known official pages.
  - Downloads resume from `<file>.part`. A file whose checksum fails is deleted, never used. Programs are written to a temporary file and renamed into place. rustup-init always gets `--no-modify-path`: never touch the user's PATH, dotfiles or registry.
- **Preview:**
  - The center pane is an iframe on the Trunk dev server.
  - `ak_preview::Preview` runs one project's preview from its stack's PreviewPlan with the isolated toolchain (`Tools::from_toolchain`).
    - It picks free ports and a 128-bit random bridge token. The bridge port stays the same across backend restarts.
    - It publishes `ak_types::preview::PreviewStatus` on a watch channel: frontend and backend each Off/Starting/Building/Ready/Error{output = compiler output tail}, plus the page URL once built.
    - Frontend states come from the plan's `frontend_log` markers; the backend is ready on its stdout prefix, and an exit is an error.
    - Each process runs in its own process tree (taskkill /T or a process group), and Drop kills everything. Inherited CARGO/RUSTC/RUSTUP_*/CARGO_HOME/CARGO_TARGET_DIR are removed.
    - After the agent or user edits files, call `files_changed(paths)`: backend code (the plan's backend_watch) restarts the backend, while the dev server rebuilds the screens itself.
    - Tests use `ak-preview-fake-tool`; `cargo run -p ak-preview --example preview_live` checks the real thing.
  - `ak-bridge` exposes the app's Tauri commands over a localhost WebSocket in dev builds, so the preview is fully functional.
  - Generated apps call a unified `invoke` wrapper (WebSocket in preview, Tauri IPC in production).
    - The bridge comes first whenever the page has a bridge address (`protocol::route`): inside AgentKnife the preview is an iframe of AgentKnife's window, and Tauri gives every frame AgentKnife's own `__TAURI_INTERNALS__`. Calls sent there reach AgentKnife, whose ACL refuses the remote origin ("<cmd> not allowed. Plugin not found").
  - Bridge and inspector code is stripped from release bundles. Everything preview-only in `ak-bridge` sits behind its `dev` feature, which generated apps enable only for the preview build; `cargo xtask ci` (step `bridge`) fails if ak-bridge without `dev` pulls in WebSocket crates.
  - The preview backend is the generated app itself, started by AgentKnife with `dev` and `AK_BRIDGE_TOKEN` (at least 16 characters), plus optional `AK_BRIDGE_PORT` and `AK_BRIDGE_ORIGIN`.
    - The `ak_bridge::init()` plugin hides the app's windows, listens on 127.0.0.1 and prints `AK_BRIDGE_READY {"port":N}`.
    - Each request runs through Tauri's real IPC (`Webview::on_message`) against the `main` webview, so state, plugins and ACL behave as in the real app.
    - Apps show their windows themselves when `ak_bridge::is_preview()` is false.
    - Backend-to-frontend events (`emit`/`listen`) are not bridged yet; prefer commands.
  - Frontend code calls `ak_bridge::invoke(cmd, &args)`, never Tauri's JS directly.
  - Control channel (the agent checks its work): AgentKnife connects to the same bridge with `?token=…&role=controller` and sends `ControlAction`s.
    - The actions are click, type (optionally Enter), dom_summary, app_state and prepare_screenshot.
    - The server hands each action to the newest open preview page and routes the answer back. Only controllers may send actions.
    - A connection without an `Origin` header is a native client; browsers always send one, and it must match.
    - Pages connect at `start()` and reconnect after the backend restarts.
    - `prepare_screenshot` only scrolls, waits for paint and reports the area. The pixels are captured by AgentKnife (P1-PREV-002).
    - Page reports strip the `ak_bridge` URL parameter, because it carries the token and the reports go to the model.
    - Apps may expose state with `ak_bridge::set_state_reader`.
    - `start()` also records uncaught page errors and rejected promises (newest 10); `dom_summary` lists them at the end, so the agent can see why e.g. a canvas stays blank.
  - Crates that link Tauri and have tests need the Windows Common-Controls manifest for their test binaries (see `crates/ak-bridge/build.rs`); otherwise the tests fail to start with STATUS_ENTRYPOINT_NOT_FOUND.
  - A "Run in real window" button runs `cargo tauri dev -- --target-dir <project>/target/ak-window`. It needs its own target folder: the preview runs its `app.exe` from `target/debug`, and Windows cannot replace a running program. The command waits until the app has run for a moment and returns the output if it could not start. Stop it with `kill_tree`, because tauri leaves trunk and the app behind.
  - The project's log (`ak_preview::log::LogBook`, owned by `Preview`, shared via `OpenProject::log`) gets every line any project program prints: the preview's processes, the real window, packaging and the AI's steps. Anything new that runs a project program must push its output there too. The Log tab (full mode) shows it; see `get_log` / `LOG_EVENT`. Anything the user waits on while it builds shows its lines live in the build log window over the preview (`logs::BuildLog`), never only a spinner. It also shows how far the part building now is, in percent: the preview's and the real window's commands set `ak_preview::process::CARGO_PROGRESS_ENV`, and `LogBook::push` turns cargo's progress bar into `LogLine::progress` reports for the listener (not kept as lines). Packaging does not set it (its output feeds the chat card).
- **ak-ui:**
  - Every component takes `ak_id` and renders `data-ak-id`. Parts get derived ids via `ak_ui::part_id` (`<id>-<key>` rows, `<id>-close`, numbered toasts).
  - No fixed UI strings: labels such as `close_label` are props, in the app's language.
  - Tailwind classes are complete literals, so Tailwind can find them. Generated apps add `@source` for the ak-ui sources.
  - Tests render with `to_html()` (leptos `ssr` as a dev-dependency only).
  - Any new component must be shown in ak-ui-gallery.
- **Knowledge pack** (`knowledge/<stack>/`): what the agent reads with `read_knowledge(topic)` before writing app code.
  - Structure:
    - `pack.toml` holds [pack] id/version, [versions] (the versions the docs describe) and the [[topic]] list (id, title, one-line summary, file).
    - There is one Markdown file per topic.
    - `check/` holds snippets of the smaller examples.
  - The stack's profile embeds it (ak-preview build.rs). ak-agent gets it only as `KnowledgePack` (`project_tools(env, asker, Some(pack))`).
  - `read_knowledge` is read-only, so the policy always allows it.
  - Docs are for the pinned versions only. When a pinned version changes, update the docs, `pack.toml` [versions], and `versions.toml` [knowledge] pack.
  - `cargo xtask ci` (versions) checks that pack version, [versions], topic files, unlisted .md files and ``topic `id` `` cross-references all agree.
  - Code in the docs must compile. After editing docs, run `cargo xtask check-knowledge`: it builds a template project whose code is `example-todo.md`'s blocks plus `check/` snippets, using the isolated toolchain. Keep `check/` in step with the snippets in the topics.
  - Write for a model that does not know these versions: complete, copyable examples; the data-ak-id convention in every example; user-facing text in the user's language.
- **`tauri-leptos` template** (`templates/tauri-leptos`):
  - Layout:
    - the root package `app-ui` is the Leptos CSR frontend (index.html, Trunk.toml, style/input.css, src/);
    - `src-tauri/` holds the `app` backend;
    - `ak/ak-bridge` and `ak/ak-ui` are vendored path deps;
    - one workspace and a committed Cargo.lock.
  - `ak/` is generated by `cargo xtask vendor-kit` from `crates/ak-bridge` and `crates/ak-ui`. Never edit it by hand. xtask ci (step `kit`) fails if it is stale or if the lockfile no longer matches the manifests.
  - Both packages have a `dev` feature (= `ak-bridge/dev`).
    - The preview runs `trunk serve --features dev`, plus the backend with `--features dev` and the `AK_BRIDGE_*` variables.
    - `cargo tauri build` uses no features, so release output has no bridge.
  - The main window starts hidden; the app shows it unless `ak_bridge::is_preview()`.
  - The template builds as-is with default names (`template.toml` [replace] lists them). Its gitignore is stored as `gitignore` and renamed on creation.
  - Pinned versions are checked in both manifests, in Trunk.toml [tools] and in Cargo.lock.
  - wasm-opt is off (`data-wasm-opt="0"`) because it is not in the toolchain.
  - `[profile.dev]` keeps only line tables for the app's code and no debug info for dependencies (first build ≈ −25%, target/ ≈ −40%; release is untouched). Never share one cargo `build-dir`/target between projects: unit hashes ignore the project path, so another project's stale screens can count as fresh.
- **Running trunk** (preview, gallery): put the toolchain's `cargo/bin` and `bin` first on PATH, so trunk uses the pinned wasm-bindgen and tailwindcss and downloads nothing.
  - Remove `NO_COLOR`: trunk parses it as a true/false flag, and `NO_COLOR=1` makes it exit.
  - Set `TRUNK_SKIP_VERSION_CHECK=true`; otherwise trunk checks the network for updates.
- **AgentKnife window** (`ak-shell`, Leptos CSR; `csr` feature for the browser build, native tests render with `ssr`):
  - Top bar with the Simple/Full switch. Full: files + timeline | preview/code tabs | chat. Simple (default): preview | chat. `layout::Layout` (mode, tab, pane widths) is saved in localStorage `agentknife.layout.v1`; unreadable values give the defaults, widths are clamped (`layout::LEFT`, `layout::CHAT`).
  - Splitters (`role="separator"`): pointer-capture drag, arrow keys, Home/Enter or double-click for the default width.
  - Text comes from fluent through `i18n::t(id)` (an `I18n` context). xtask (step `i18n`) rejects plain text literals in ak-shell views (a literal alone on a line, or between `>` and `<`, with an uppercase or non-ASCII letter).
  - Every button has a `title` saying in plain language what it does (fluent `…-tip`; existing ones may end in `-hint`). `tests/tooltips.rs` fails on a `<button` without one.
  - Browser-only code (storage, navigator) sits behind `cfg(target_arch = "wasm32")`. In a browser, `?lang=zh-TW` forces the language.
  - Screens (`app::Screen`): the connect wizard on first run (no AI yet), the home screen (recent projects, new, open a folder), a project's workspace. The top bar shows the project, the way home, the AI in use (click to change it) and the Simple/Full switch in a workspace.
  - Calls to ak-app go through `backend::Backend` with the commands and types in `ak_types::ipc` (one `request` argument each). Inside AgentKnife it invokes Tauri through `js_sys::Reflect` on `window.__TAURI_INTERNALS__` (no extern block, no unsafe). In a plain browser a demo backend answers (badge "Demo"): keys containing `bad` are refused, addresses containing `nothing` do not answer, models with `3b` cannot use tools.
  - Connect wizard (`connect::Wizard`, testable without a view): choose a preset (`ak_types::ipc::PRESETS`, fluent `connect-preset-<id>`), key and/or address, test (models), pick a model (the preset's suggestion first), capability check. A model without tool calling needs "Use it anyway"; no vision gets a note. Errors are `ConnectError` → fluent `connect-error-<key>` with technical detail under "Details". The key stays in the wizard only until saved.
    - Each provider's last saved connection is kept (`AppSettings::ai_known`, set only by `save_connection` via `AppSettings::connect`; `save_settings` never changes it). `saved_connections` returns them with `has_key` (the keychain has one; the key itself never goes to the UI) and `current`. The wizard marks the one in use and those set up before, and choosing a provider fills in its address and model again; with a saved key the key field may stay empty (the app uses the keychain's).
  - `ak_providers::connect` (`provider_for`, `list_models`, `check_model`) is what ak-app runs behind those commands.
  - Chat pane (`chat`): `chat::log::ChatLog` turns `AgentEvent`s (Tauri event `ipc::AGENT_EVENT`) into items: user/assistant bubbles (streaming; a new turn starts a new bubble; thinking collapsed), steps with "Details" (`StepStarted.detail`, `StepFinished.output`), build-check lines, cards for confirmations, plans (Start / Change / Cancel), questions and non-completed outcomes (stuck: another way / roll back / details; step limit or cut off: continue; failed: retry). Answered cards keep the choice; cards a turn left unanswered expire. Keep view code out of `ChatLog` so it stays testable.
    - `chat::Chat` (context) holds the log and the preview's selection; the chip goes with the next message (`SendMessage.selection`). Opening a project fills the log from `OpenedProject.history` (messages only) and shows the continue / go-back card for `interrupted`.
    - The demo plays scripted turns: words in the message pick plan, delete (confirmation), ask (question), stuck, long (step limit), fail. Files holding demo data carry `// xtask-i18n: demo data` so the UI-text check skips them.
  - Preview pane (`preview`): the page in an iframe; computer fills the stage, tablet (820×1180) and phone (390×844) are framed and scaled to fit (`layout::fit_scale`, ResizeObserver); the device is saved in the layout. `preview::cover` maps `PreviewStatus` to what covers the page: off, first build, a "Updating…" badge over the last good page, or an error panel (screens or background part) with Details and "Ask the AI to fix it" (a chat message; disabled while the AI works). "Run in a real window" calls `ipc::cmd::RUN_APP`.
    - Point-and-select uses `ak_types::preview::{InspectorCommand, InspectorEvent}` (copies of ak-bridge's, checked by ak-preview's tests). Commands go to the iframe's origin only; only messages from that iframe and origin count. Select → `Chat.selection` (chip + outline at the picked rect inside the scaled frame), mode off; Cancel or Escape → off; a reloaded page (Ready) gets Enable again while the mode is on.
    - Status arrives as the Tauri event `ipc::PREVIEW_EVENT` and `cmd::PREVIEW_STATUS` on open. The demo shows a static page (srcdoc), or a real preview with `?preview=<url-encoded page URL>` (from `preview_live` with `AK_PREVIEW_HOLD`) to try point-and-select for real.
  - Files, version history and code (`files`, full mode): `files::Files` (context) holds the tree, the open file, unsaved edits per path and the timeline; `visible_rows` builds the tree (folders first, closed folders hidden). Clicking a file opens it in the Code tab; Save or Ctrl/Cmd+S writes through `cmd::WRITE_FILE` (the app tells the preview, which rebuilds). After every turn, a restore or a marked version, the tree and timeline reload; an open file without edits reloads too, one with edits shows "changed by the AI" (reload or keep mine). Going back asks first (the current work is set aside, restorable).
  - The editor view is made again only when the file or its version changes (a `Memo` over path/version), never on save or typing.
  - Window listeners and observers outlive a pane that is switched away (preview ↔ code): remove them in `on_cleanup` and use `try_get_untracked`/`try_set` in their callbacks, or the disposed `NodeRef`s panic.
  - Tools check and packaging are chat cards (`chat::log::{DoctorCard, BundleCard}`, views in `chat/tools.rs`); the latest card of each kind is the one updated. The chat header has "Check tools" and "Package the app"; opening a project checks quietly and adds the card only when something is missing.
    - Doctor: items with status (fluent `doctor-item-*`, `doctor-status-*`), "Install the missing tools" for isolated items with live `DoctorProgress` (`ipc::DOCTOR_EVENT`, download bar), one guide per missing system dependency (steps, copyable commands, links), the official installer only after an inline confirmation (`doctor-confirm-*`), then "Check again".
    - Bundle: stage and output while building (`ipc::BUNDLE_EVENT`), then `BundleResult` (install steps with `$file`/`$app`, "Show in folder" via `cmd::REVEAL`, the other-system note); a `BundleFailure` shows `bundle-error-<kind>` with Details and, for build problems, "Ask the AI to fix it".
  - Backend events go through `Backend::on::<T>(event, f)`; the demo keeps listeners per event name and also plays the tools check (install progress, WebView2 via its installer) and packaging ("Broken" in README.md makes it fail).
  - Do not run `cargo test` while `trunk serve` watches ak-shell: both build in the same target folder, and the doctest step then fails with "can't find crate".
  - `cargo test --workspace` unifies leptos features (the gallery's `csr` with the tests' `ssr`), and an `Effect` then panics without an executor. Code the render tests reach saves on change (`shell::change`) instead of using effects.
- **Point-and-select:**
  - Every meaningful element in generated UI carries a stable `data-ak-id`. The agent must add these, and AgentKnife auto-inserts missing ones.
    - `StackProfile::add_missing_ids(rel_path, source)` (default: none). For tauri-leptos it is `ak_preview::ids` on `src/**.rs`: syn (span-locations) finds each `view!`, rstml 0.13 parses it like leptos_macro (`recover_block`), and ` data-ak-id="…"` goes right after the tag name.
    - Only meaningful lowercase HTML elements without an id (buttons, links, inputs, headings, lists, landmarks…). Never components (they take `ak_id`), and never `view!` inside closures (rows, `.map`, `move ||`), where a fixed id would repeat; those need ids built from data, which the agent writes.
    - Ids are words from the text or a describing attribute plus the kind (`sign-up-button`, `email-input`), else the component name; unique per file (-2, -3). The pass is idempotent, and a file that does not parse is left alone.
    - `Agent::with_id_pass` runs it on the turn's changed files when the model ends the turn, before the build check, and tells the preview.
    - Template, examples and the gallery must already pass it unchanged (tested), so `<main>` screens carry ids too.
  - The inspector reports the id, DOM path, and screenshot rect via `postMessage`.
  - For `<canvas>` regions, send the screenshot rect and coordinates. Apps may implement `ak-bridge`'s `hit_test(x, y)` for exact mapping.
  - Generated frontends call `ak_bridge::start()` before mounting. Canvas apps may call `ak_bridge::set_hit_test(…)`, which receives canvas pixel coordinates. In release builds both are no-ops.
  - Messages are JSON strings `{"ak_inspector": …}`. AgentKnife sends `{"cmd":"enable"|"disable"}`; the preview answers `ready`, `hover`, `select` and `cancel` (`ak_bridge::protocol`).
    - The inspector obeys only its parent window and posts only to the origin that enabled it.
    - While enabled, clicks never reach the app.
    - AgentKnife's own UI must also treat Escape as cancel, because focus may be outside the frame.
- **Project tools** (`ak_agent::tools`): read/write/edit/list/search files, run_command, ask_user.
  - Every path goes through `tools::path::resolve`, which rejects absolute paths, `..` escapes and symlinks leading outside the project. This applies regardless of the permission policy.
  - `run_command` on Windows must use `raw_arg("/S /C \"…\"")`, because Rust's argument quoting breaks cmd.
  - On timeout or cancel, `run_command` must kill the whole process tree, because killing only the shell leaves cargo/trunk running.
- **Agent sees results:** a preview-screenshot tool when the model has vision, and a DOM summary otherwise. In dev builds, the `ak-bridge` control channel lets the agent click, type, and read state for self-verification.
  - Tools (`ak_agent::tools::preview`, added with `with_preview_tools(registry, preview, vision)`):
    - `build_status`, `dom_summary`, `interact` (click/type/read_state by data-ak-id) and `get_selection`.
    - `screenshot_preview`, only with vision and a `Capturer` (ak-app supplies the capture).
    - All go through the `PreviewAccess` trait (implemented by `ak_preview::PreviewSession`), and the policy allows them.
  - `Agent::with_preview`:
    - After a step that touched files, it calls `files_changed`.
    - When a turn that changed files ends, it waits for the build (BuildChecking/BuildChecked events). On failure it adds a context note with the compiler output and the model continues, up to `max_fix_attempts`, shared with failed runs; after that the turn ends as Stuck.
  - `PreviewSession::act` retries while the page reloads after a rebuild (10 s): always when no page got the action, and only read-only actions when the page left mid-action (never a second click).
  - Template rule: Trunk watches only what the screens are built from (`[watch] watch = [...]`). Writes elsewhere (`.agentknife/`, notes, the backend) must not reload the page while the agent works.
- **Projects:**
  - The project list is the home screen. Projects default to `~/AgentKnife/Projects/<name>`.
  - Deleting a project (`delete_project`) either removes it from the list only, or also moves its folder to the system trash (`trash` crate). AgentKnife never permanently deletes a user's project folder. An open project is closed first, because its preview holds files.
  - The AI connection is global, with an optional per-project override.
  - Each project's `.agentknife/` folder contains `PROJECT.md` (requirements and decisions in plain language), `plan/STATUS.md`, `plan/tasks.md` (same formats as §8), and the chat history.
  - The agent reads these at session start. Long chats are compacted.
    - `Agent::open_session` appends the notes as a `context` entry (deduplicated).
    - Compaction: near `compact_at` × the context window, the agent appends a `compaction` entry. Its summary replaces everything before the previous turn in the *model's view*; the stored chat keeps every entry.
    - The size estimate counts the system prompt and the tool schemas too.
    - `Agent::resume_turn` continues an interrupted turn from `turn.lock`. Unanswered tool calls get "interrupted" results first, so the history stays well-formed.
    - Hosts must attach a `ConversationStore`: the agent saves after every append, so a crash loses at most the step in progress.
- **Chat history format:** it is stored in AgentKnife's own provider-neutral schema, defined in `ak-types`, never in a provider's raw wire format.
  - Message kinds: user, assistant text, thinking, tool call, tool result, checkpoint marker.
  - The format carries `schema_version`, with migrations for older versions.
  - Provider adapters convert to and from the neutral schema.
  - Assistant messages carry an optional `ProviderReplay { origin, content }`: the provider's exact output, including signed thinking and block order.
    - The origin provider echoes it **verbatim**. Anthropic requires signed thinking back unchanged, and drops on its side any blocks the current model can't read.
    - Other providers rebuild the request from the neutral text and tool calls instead.
    - Neutral `thinking` is display-only.
  - **Histories are append-only.** Never edit, reorder or remove earlier messages, and never rebuild `system` or `tools` mid-conversation. Providers bind reasoning signatures to the exact prefix, so an edit makes later thinking invalid (a 400 on new Anthropic accounts).
    - Compaction must use the provider's server-side compaction, or append a summary. It must never rewrite.
    - The Anthropic adapter also sends `thinking.block_binding.prefix_mismatch_behavior: "drop_block"` as a safety net.
  - Switching providers mid-project must keep working.
- **Interrupted turns:** at turn start, the agent writes `.agentknife/turn.lock`, holding the request, the files touched so far, and the current step. The lock is deleted only after the turn's checkpoint is created.
  - On project open, a leftover lock or un-checkpointed changes trigger a plain-language prompt: "Last time I stopped halfway (doing: …). **Continue**, or **go back to the last checkpoint**?"
  - **Continue:** the agent resumes from the lock and the current files.
  - **Go back:** files are restored, and the partial work is saved as an "abandoned" timeline entry so it can still be recovered.
  - The same path covers app crashes, force-quit, network loss, and provider errors.
- **Checkpoint storage** (`ak_project::Timeline`):
  - A bare gix repository at `.agentknife/checkpoints.git`, with the project root as worktree. The user's own `.git` is never touched, and no system git is needed.
  - `.agentknife/timeline.json` is the append-only index; every entry has a ref `refs/checkpoints/<id>`.
  - Restore saves unsaved work as an `abandoned` entry first and appends a `restore` entry, so nothing is ever lost.
  - Snapshots respect .gitignore and never include `.git`, `target`, the chat, or `.agentknife` internals, so restoring never rewinds the chat.
- **Checkpoints:** the git commits are hidden from the user. The UI shows a plain-language timeline and never uses words like commit or branch. Checkpoints come in two kinds:
  - *turn checkpoints*, after every agent turn
  - *task checkpoints*, when a task completes, with the task id in the message
- **Checkpoint GC:**
  - Keep the latest **50** turn checkpoints (configurable).
  - Squash older turn checkpoints into their task checkpoint, in the background. In practice: `Timeline::gc` removes their index entries and refs, and their changed files fold into the next surviving entry.
  - Never delete task checkpoints, bundle checkpoints, or user-marked versions ("Mark this version").
  - Also never delete `abandoned` entries (the user's set-aside work), `restore` or `initial` entries, or the newest entry, which is the state on disk.
  - Task checkpoints are always recorded, even without new changes, because a task usually ends right after its last turn.
- **Bundling:** build for the host platform only. Windows: `.zip`. macOS: `.dmg`. Linux: `.tar.gz`. Installers (NSIS `.exe`, `.deb`) come later (P1-BUNDLE-003); Phase 2 adds Android `.apk`. Output is unsigned, and the AI explains SmartScreen/Gatekeeper workarounds in plain language.
  - `ak_bundle::Bundler::build` runs the plan's command (`cargo tauri build --no-bundle` on Windows and Linux, `--bundles dmg` on macOS), streams stages and output lines, and refuses a build that contains a preview marker (the bridge's ready line in the program, the inspector's overlay id under `dist/`).
  - Archives hold `<Name>/<Name>[.exe]` (mode 755 in tar.gz) and go to `target/release/bundle/{zip,tar.gz}/<Name>_<version>_<arch>.<ext>`; name and version come from tauri.conf.json. Every success records a `Bundle` checkpoint.
  - The preview's `trunk serve` writes to `target/ak-preview-dist`, never `dist/`, so a release build cannot swap the preview's screens for ones without `dev`.
  - On Windows the zip needs WebView2 (built into Windows 10/11); on Linux the tar.gz needs WebKitGTK 4.1 installed on the target computer.
  - `ak_bundle::guide`: `install_guide(os, output)` returns doctor-style `GuideStep`s (fluent `bundle-guide-…` with `$file` and `$app` from `guide_args`, and a command where one is needed), ending with a sharing step for the same OS. `reveal(path)` shows the file in Explorer/Finder or opens its folder on Linux. `other_os_note` gives `bundle-other-os` (with `os-*` names) for requests for another system; the product prompt tells the agent the same.
- **AgentKnife app** (`ak-app`, binary `agentknife`; a library plus src/main.rs so tests can use it): one Tauri command per `ak_types::ipc::cmd` (the test `every_ipc_command_is_handled` checks the list and the handler macro), events `agent-event`, `preview-status`, `doctor-progress`, `bundle-progress`. Errors are JSON (text, `ConnectError`, `BundleFailure`); `anyhow` stays inside ak-app.
  - State: the registry, `<data>/AgentKnife/settings.json` (the AI connection; never keys), the usage ledger, and the open project (Project, PreviewSession, conversation, running turn, "before turn" entry for roll back, the real-window process).
  - Opening a project starts its preview, loads the chat, reports an interrupted turn, then adds the project notes (`open_session`) and greets a brand-new chat.
  - Each turn builds its agent from the current connection (keychain key, capabilities → vision/context/max tokens, cost basis), with ProjectPolicy, the UI bridge (confirmations, plans, questions answered by request id), preview tools, the id pass, timeline checkpoints, the conversation store and the usage ledger. The selection goes to the session and to the AI as a context note. Continue / try another way send fixed requests in the UI language (`agent-continue-request`, `agent-try-another-request`).
  - Files come from `ignore` (honours .gitignore; hides .agentknife, .git, ak/); every path goes through `ak_agent::tools::path::resolve`. Links open through `open_url` (https only, tauri-plugin-opener); folders are picked with tauri-plugin-dialog.
  - Run it with `cargo xtask dev`: `cargo tauri dev` in `crates/ak-app` with AgentKnife's own toolchain (cargo-tauri, trunk, Tailwind; nothing to install or put on PATH by hand; it starts `trunk serve` for ak-shell). Arguments after `dev` go to `cargo tauri dev`. To drive the window from scripts, start it with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223` and use the DevTools protocol (Runtime.evaluate, Input.dispatchMouseEvent, Page.captureScreenshot).
  - Settings (`ak_types::settings::AppSettings` in `<data>/AgentKnife/settings.json`, loaded clamped; files without the newer fields still load): language (None = the system's), permissions (→ `ProjectPolicy` mode), plans (→ `PlanMode`), max_fix_attempts, keep_turns (timeline GC after each turn), daily_cap_usd, read_folders (folders the AI reads without asking: the settings page adds them with the `pick_folder` dialog; ak-app keeps existing absolute folders, adds them to the agent's reference folders and lists them in the system prompt as read-only). `save_settings` keeps the AI connection (only the wizard changes it, with its key) and switches the UI language at once (`i18n::Tr::set_locale`; take the `Tr` before going async, contexts are not reachable from spawned tasks).
  - No telemetry. Crash reports: a panic hook writes `<data>/AgentKnife/crash-reports/<unix time>.txt` (version, OS, message, place, backtrace; the newest 20 are kept). AgentKnife never sends them; the settings page lists them with "Copy" and "Show in folder".
  - Editing a crate while `cargo tauri dev` runs restarts the window (state is in memory: open the project again).
  - Commands are generic over `tauri::Runtime`; `ak_app::with_commands(builder, state)` adds the state and the handler (plugins are added by main). `AppState::new(state_dir, registry, toolchain_root)`; `AppState::provider` replaces the connection's AI (tests).
  - End-to-end test (P1-E2E-001, `crates/ak-app/tests/e2e.rs`, ignored by `cargo test`; `cargo xtask e2e`): Tauri's mock runtime (`tauri/test`) runs the real commands with a `ScriptedProvider`, collecting the window's events: connect → doctor check → new project → greeting → to-do app (`tests/fixtures/todo/main.rs`, local storage) → selected button turns green → bundle → install steps. It builds and packages a real app with AgentKnife's toolchain (`AK_E2E_TOOLCHAIN`, default the data folder; `AK_E2E_PROVISION=1` installs a missing one). CI's `e2e` job caches the toolchain folder.
- **Privacy:** no telemetry by default. Crash reports are opt-in only.

## 5. Coding rules

- Rust edition 2024.
- **unsafe:**
  - Every crate has `#![forbid(unsafe_code)]`, except `ak-shell`.
  - `ak-shell` uses `#![deny(unsafe_code)]`. Only its `interop` module (wasm-bindgen / CodeMirror glue) may use `#[allow(unsafe_code)]`.
  - Every `unsafe` block needs a `// SAFETY:` comment.
- **unwrap/expect:**
  - `unwrap()` is banned outside tests.
  - `expect("reason")` is allowed only for invariants that cannot fail, such as static init or validated constants. It needs a `// INVARIANT:` comment on the line above.
  - Everything else propagates errors with context.
- **Two-layer i18n:**
  - *Fixed strings* use fluent through `ak_types::i18n`.
    - Files live in `crates/ak-types/locales/<locale>/*.ftl` and are embedded with `include_str!`.
    - `cargo xtask ci` checks that each locale has the same files, ids and attributes, and that every file is embedded.
    - Keys use `area-name` (for example `doctor-installing`).
    - Missing keys fall back to en, then to the key id.
    - Every Chinese OS locale resolves to zh-TW.
    - Every fixed string must have both zh-TW and en entries. These are UI chrome, deterministic progress labels (doctor, bundle, checkpoint), errors, and dialogs. Never hard-code them.
  - *AI-generated text* does not use fluent. That covers replies, plans, and tool-call narration. Its language is controlled by the system prompt (§4 "AI reply language").
- Write plain-language messages for non-programmers. No jargon in default UI text; put technical detail behind "Details".
- Keep public APIs of core crates documented with `///`.
- Format with `rustfmt` and lint with `clippy -D warnings`.

## 6. Command cheatsheet

Prerequisites for developing AgentKnife:
- Rust (stable, version per `rust-toolchain.toml`) plus the `wasm32-unknown-unknown` target
- `trunk`
- `tauri-cli` (`cargo install tauri-cli`)
- Windows: MSVC Build Tools and WebView2
- macOS: Xcode CLT
- Linux: `webkit2gtk-4.1` and the other packages from the Tauri prerequisites

| Task | Command |
|---|---|
| Run AgentKnife (shell + UI, hot reload; uses AgentKnife's toolchain) | `cargo xtask dev` |
| Build AgentKnife (release, screens built in) → `target/release/agentknife.exe`; run that file | `cargo xtask build` |
| End-to-end test with a scripted AI (builds and packages a real app; minutes; needs AgentKnife's toolchain) | `cargo xtask e2e` |
| Run the UI alone in a browser (http://127.0.0.1:47120, toolchain on PATH as for the gallery) | `trunk serve` (from `crates/ak-shell`) |
| Test one crate | `cargo test -p <crate>` |
| Full pre-commit gate | `cargo xtask ci` |
| Refresh the vendored CodeMirror | `cargo xtask vendor-codemirror` |
| Check that the knowledge pack's code compiles (slow; needs the isolated toolchain) | `cargo xtask check-knowledge` |
| Refresh the template's vendored ak-bridge / ak-ui after changing them | `cargo xtask vendor-kit` |
| Re-pin download checksums after a tool version change | `cargo xtask pin-downloads` |
| See every ak-ui component (toolchain on PATH, see "Running trunk") | `cd crates/ak-ui-gallery && trunk serve` (http://127.0.0.1:47130) |
| Real OS keychain round trip (writes and deletes a test entry) | `cargo test -p ak-providers --test credentials -- --ignored` |
| Live check of the preview manager (new project, real toolchain; minutes) | `cargo run -p ak-preview --example preview_live` |
| Add missing data-ak-ids to Leptos files, in place | `cargo run -p ak-preview --example add_ids -- <file.rs>…` |
| Live check of bundling (new project, real toolchain; minutes) | `cargo run -p ak-bundle --example bundle_live` |
| Doctor report for this computer (read-only) | `cargo run -p ak-doctor --example doctor_report` |
| Install the isolated toolchain for real (several hundred MB) | `cargo run -p ak-doctor --example doctor_install` |
| Record real Anthropic fixtures (costs cents) | `ANTHROPIC_API_KEY=… cargo run -p ak-providers --example record_anthropic` |
| Record real Chat Completions fixtures (free with local Ollama) | `AK_MODEL=qwen3:8b cargo run -p ak-providers --example record_openai_compat` (`AK_WIRE=responses` records the Responses API, `AK_VISION=1` adds an image scenario) |
| Run one live agent turn (manual check) | `AK_BASE_URL=http://127.0.0.1:8080/v1 AK_MODEL=… cargo run -p ak-agent --example agent_live` |
| Live check: agent + running preview + real model (open the printed URL in a browser) | `AK_BASE_URL=… AK_MODEL=… cargo run -p ak-agent --example agent_preview_live -- "<request>"` |
| Live check of the project tools (edits a scratch project) | same env vars, `--example agent_tools_live` |
| Regenerate `rust-toolchain.toml` | `cargo xtask gen-toolchain` (never edit it by hand) |

**Windows gotcha:** run cargo from PowerShell or cmd, not Git Bash. Git Bash's `/usr/bin/link` shadows MSVC `link.exe`, and every build fails with `link: extra operand`.

When building the workspace skeleton, implement exactly these entry points. If a command changes, update this table in the same change.

## 7. Testing

- Every crate has unit tests.
- Providers are tested against fixtures (`crates/ak-providers/tests/fixtures/<provider>/`) and never call real APIs in CI.
  - Each fixture is marked `"source": "synthetic"` (hand-written from the documented wire format, with exact request bodies asserted) or `"recorded"`.
  - Recorded fixtures are captured with `cargo run -p ak-providers --example record_<provider>` using an env-var key. They live in `recorded/` and are replayed by a decode-to-completion test.
  - Fixtures never contain request headers or keys.
- Before coding against a provider API, check its current docs. For Anthropic, load the `claude-api` skill. Model behaviour changes quickly; for example, `budget_tokens` is rejected by current Claude models.
- Agent flows are tested end-to-end with a **scripted fake LLM**.
- MVP acceptance scenario, as a fake-LLM E2E test and run manually on clean machines (Windows 11, macOS, Linux): a non-programmer installs AgentKnife, connects an AI, lets doctor set up tools, and builds a to-do list app (add and delete items, local storage) only through chat. Along the way they click a button in the preview to change its color. At the end they get an installer and successfully install and run the app.
- CI: GitHub Actions on Windows, macOS, and Linux. CI makes no network fetches beyond crate downloads.

## 8. Workflow for agents

1. At session start, read `AGENTS.md`, `plan/STATUS.md`, and `plan/execution-plan.md`, then run `git status`. If there are uncommitted changes:
   - If they match a `Doing` entry, read its `Notes:` and continue that task.
   - Otherwise, **stop and ask the owner**. Never discard work you cannot attribute.
2. Work in small tasks, each with an id from `plan/execution-plan.md`.
3. **Before writing any code** for a task, add its `Doing` line to `plan/STATUS.md` (format below).
4. Add `Notes:` whenever you learn something non-obvious: a pitfall, a rejected approach, or where you are. Do it at the moment you learn it, not at the end. The notes are the handover if the session dies.
5. Design changes go in the **same commit** as the code. That means AGENTS.md, SPEC, and `versions.toml`.
6. Before every commit, run `cargo xtask ci` and fix everything it reports.
7. When a task finishes:
   - Move its line from `Doing` to `Done` with the commit hash.
   - Move any `Notes:` worth keeping into AGENTS.md or code comments.
   - Commit with the task id in the message.
8. **Parallel agents** each use their own `CARGO_TARGET_DIR` and only edit crates whose tasks they own (the owner is listed in `execution-plan.md`).
9. **Shared files** may be edited only by the coordinator (the main agent or the owner): root `Cargo.toml`, `versions.toml`, `xtask/`, `AGENTS.md`, `.github/`, and `plan/execution-plan.md`. If another agent needs a change there, it adds a `Blocked` entry in `STATUS.md`. Every agent may edit its own lines in `STATUS.md`.
10. Stop and ask the owner before changing: the tech stack, licensing, publishing or releases, new remotes, secrets, telemetry, or anything in §2.

### Plan file formats

`plan/execution-plan.md`: one entry per task.

```
### P1-AGENT-003 — Provider trait + Anthropic implementation
- Crate(s): ak-providers
- Owner: <agent name | unassigned>
- Depends on: P1-CORE-001
- Acceptance: <observable, testable criteria>
```

The id format is `P<phase>-<AREA>-<NNN>`.

`plan/STATUS.md` has exactly three sections:

```
## Doing
- P1-AGENT-003 — <agent> — started YYYY-MM-DD
  Notes: <pitfalls found, rejected approaches, current step>
## Blocked
- P1-PREV-002 — needs: <who/what> — <reason>
## Done
- P1-CORE-001 — <commit short hash>
```

The user-project files `.agentknife/plan/tasks.md` and `STATUS.md` use the same formats, with templates in `templates/tauri-leptos/`.

## 9. Roadmap

- **Phase 1 (MVP):** desktop AgentKnife for Windows, macOS, and Linux; AI connect (cloud and local); doctor; the `tauri-leptos` profile with template and `ak-ui`; preview with bridge, device frames, and point-and-select; chat; checkpoint timeline; host bundling with install guidance; zh-TW and en.
- **Phase 2:** Android (isolated SDK/NDK/JDK, `.apk`), auto-update, more providers.
- **Phase 3:** iOS (macOS hosts only); large-project mode with parallel multi-agent work; egui stack profile; code signing and publishing; wasmtime plugins.

## 10. License

MIT OR Apache-2.0.
