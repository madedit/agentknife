# Execution plan

Task format and id scheme: see AGENTS.md §8. Only the coordinator edits this file.

<!-- Template:
### P1-AREA-NNN — <title>
- Crate(s): <crates>
- Owner: <agent name | unassigned>
- Depends on: <task ids | none>
- Acceptance: <observable, testable criteria>
-->

## Phase 1 — MVP

Areas: CORE (workspace, xtask, CI), I18N, PROV (providers), AGENT, DOC (doctor), PROJ (projects/checkpoints), BRIDGE, PREV (preview), UI (ak-ui kit), TPL (template), KNOW (knowledge), BUNDLE, SHELL (AgentKnife UI), APP (Tauri wiring), E2E.

Suggested parallel tracks once P1-CORE-003 is done:
- A: PROV → AGENT
- B: DOC
- C: PROJ
- D: BRIDGE → UI → TPL → KNOW
- E: PREV → BUNDLE
- F: SHELL

APP and E2E integrate all tracks.

---

### Core / tooling

### P1-CORE-001 — Workspace skeleton + pinned versions
- Crate(s): root, all `crates/*` (empty lib/bin stubs), `xtask/`
- Owner: coordinator
- Depends on: none
- Acceptance:
  - All crates from AGENTS.md §3 exist and the workspace builds.
  - Every value in `versions.toml` holds the latest stable release, checked against crates.io or the upstream release page.
  - `rust-toolchain.toml` is generated from `versions.toml`.
  - Every crate has `#![forbid(unsafe_code)]`. `ak-shell` has `#![deny(unsafe_code)]`.
  - Root `README.md`, `LICENSE-MIT`, and `LICENSE-APACHE` exist.

### P1-CORE-002 — `cargo xtask ci`
- Crate(s): xtask
- Owner: coordinator
- Depends on: P1-CORE-001
- Acceptance: `cargo xtask ci` runs, in order, and exits non-zero on any failure:
  - `fmt --check`
  - `clippy -D warnings`
  - tests
  - the layering check (AGENTS.md §3 rules, from `cargo metadata`)
  - the versions check (`Cargo.toml`, `rust-toolchain.toml`, templates and knowledge metadata against `versions.toml`)
  - the vendor hash check
  - the no-Node check (no `package.json` or `node_modules`)
  - the wasm build of `ak-shell`

  Each check has a test case that makes it fail on purpose.

### P1-CORE-003 — GitHub Actions CI
- Crate(s): `.github/`
- Owner: coordinator
- Depends on: P1-CORE-002
- Acceptance: `cargo xtask ci` runs on windows-latest, macos-latest, and ubuntu-latest for every push and PR. Cargo caching is on. Nothing is fetched from the network except crates.

### P1-CORE-004 — Vendor CodeMirror 6
- Crate(s): xtask, ak-shell
- Owner: coordinator
- Depends on: P1-CORE-002
- Acceptance:
  - `cargo xtask vendor-codemirror` fetches the esm.sh `?bundle` builds at the pinned versions into `crates/ak-shell/vendor/codemirror/` and writes `SHA256SUMS`.
  - Re-running it produces identical files.
  - CI verifies the hashes and fails if a single byte changes.

### P1-I18N-001 — Fluent infrastructure
- Crate(s): ak-types, ak-shell
- Owner: unassigned
- Depends on: P1-CORE-001
- Acceptance:
  - zh-TW and en `.ftl` bundles load.
  - Locale is detected from the OS and can be overridden in settings.
  - A check in `xtask ci` fails if a key exists in one locale but not the other.
  - Native crates and the WASM UI use the same API.

---

### Providers & agent

### P1-PROV-001 — Provider trait + streaming model
- Crate(s): ak-providers
- Owner: unassigned
- Depends on: P1-CORE-001
- Acceptance:
  - A `Provider` trait covers streaming chat with tool calls, thinking/reasoning deltas, usage (tokens), and a capability probe (tool calling, vision, context size).
  - Errors are typed with `thiserror`.
  - A fixture-replay test harness exists.

### P1-PROV-002 — Anthropic provider
- Crate(s): ak-providers
- Owner: unassigned
- Depends on: P1-PROV-001
- Acceptance: streaming text, thinking, tool use, vision input, and usage all pass against recorded fixtures. No network in tests.

### P1-PROV-003 — OpenAI + OpenAI-compatible provider
- Crate(s): ak-providers
- Owner: unassigned
- Depends on: P1-PROV-001
- Acceptance:
  - One implementation covers OpenAI, Ollama, LM Studio, and OpenRouter by base URL.
  - The capability probe flags models without tool calling.
  - Tests use fixtures for OpenAI and Ollama responses.

### P1-PROV-004 — Credential storage
- Crate(s): ak-providers
- Owner: unassigned
- Depends on: P1-PROV-001
- Acceptance:
  - API keys are read and written only through `keyring`.
  - A test asserts keys never appear in serialized config or `tracing` output.

### P1-AGENT-001 — Agent loop core
- Crate(s): ak-agent
- Owner: unassigned
- Depends on: P1-PROV-001
- Acceptance:
  - A turn loop drives model → tool calls → results → model until done.
  - It emits a typed event stream: text, thinking, step-started/finished with an AI-generated label, usage, and needs-confirmation.
  - Calls can be cancelled.
  - A scripted fake-LLM provider exists and is used by the tests.
  - The provider-neutral chat schema lives in `ak-types`, with `schema_version`.
  - Provider adapters convert to and from it, and thinking is never sent back to a model.
  - Test: a history recorded with the Anthropic adapter continues correctly with the OpenAI-compatible adapter.
  - At turn start the loop writes `.agentknife/turn.lock` (request, touched files, current step), and removes it after the turn's checkpoint.

### P1-AGENT-002 — Project tools
- Crate(s): ak-agent
- Owner: unassigned
- Depends on: P1-AGENT-001
- Acceptance:
  - Tools: read, write, edit, list, and search files; run command (cwd locked to the project); ask the user (structured choices).
  - Every tool call carries a plain-language label.
  - Unit tests cover each tool.

### P1-AGENT-003 — Permission policy
- Crate(s): ak-agent
- Owner: unassigned
- Depends on: P1-AGENT-002
- Acceptance:
  - In-project actions run automatically.
  - Downloads, out-of-project paths, and deletions emit needs-confirmation and block until the user answers.
  - Path-escape attempts are denied (`..`, symlinks, absolute paths).
  - Table-driven tests cover these cases.

### P1-AGENT-004 — Behaviour policies
- Crate(s): ak-agent
- Owner: unassigned
- Depends on: P1-AGENT-002
- Acceptance:
  - Plan-before-big-change: the plan event waits for "Start", with an "always ask" / "never ask" setting.
  - Auto-fix retries up to 3 times (configurable), then emits the 3-option fallback.
  - The reply-language rule (last user message, falling back to the UI language) is injected into the system prompt.
  - Fake-LLM tests cover each policy.

### P1-AGENT-005 — Context management
- Crate(s): ak-agent, ak-project
- Owner: unassigned
- Depends on: P1-AGENT-001, P1-PROJ-002, P1-PROJ-003
- Acceptance:
  - At session start the agent loads `.agentknife/PROJECT.md`, `plan/STATUS.md`, and `plan/tasks.md`.
  - Older turns are compacted when the context hits a threshold.
  - The agent updates PROJECT.md and STATUS.md through tools.
  - A test shows a long scripted session staying under the context limit.
  - Resume after interruption: given a `turn.lock`, the agent continues the interrupted turn.
  - A fake-LLM test kills a turn midway, then resumes it.

### P1-AGENT-006 — Usage & cost tracking
- Crate(s): ak-agent
- Owner: unassigned
- Depends on: P1-AGENT-001
- Acceptance:
  - Per-session and per-day tokens and estimated cost are tracked, using a price table per model.
  - A daily cap triggers a warning at 80% and a confirmation at 100%.

### P1-AGENT-007 — Greeting & onboarding script
- Crate(s): ak-agent
- Owner: unassigned
- Depends on: P1-AGENT-004
- Acceptance:
  - After connect, the agent greets the user in their language, explains what AgentKnife can do, and proposes running the doctor.
  - The system prompt template is versioned in the repo.

---

### Doctor

### P1-DOC-001 — Detection
- Crate(s): ak-doctor
- Owner: unassigned
- Depends on: P1-CORE-001
- Acceptance:
  - Detects the isolated Rust (custom `RUSTUP_HOME`/`CARGO_HOME`), `wasm32-unknown-unknown`, trunk, wasm-bindgen, tauri-cli, and Tailwind CLI, plus system dependencies:
    - Windows: MSVC Build Tools and WebView2
    - macOS: Xcode CLT
    - Linux: webkit2gtk and the other required packages
  - Returns a structured report with versions compared against `versions.toml`.
  - Makes no LLM calls.

### P1-DOC-002 — Isolated install
- Crate(s): ak-doctor
- Owner: unassigned
- Depends on: P1-DOC-001
- Acceptance:
  - Installs every isolated tool into AgentKnife's data dir.
  - Installs are idempotent and resumable, and verify checksums.
  - Progress events map to fluent keys.
  - Re-running after a killed install finishes cleanly.
  - The user's system PATH and dotfiles are never modified.

### P1-DOC-003 — Guided system-dependency install
- Crate(s): ak-doctor
- Owner: unassigned
- Depends on: P1-DOC-001
- Acceptance:
  - Each missing system dependency gets step-by-step plain-language instructions in fluent, with a "check again" action.
  - Where an official installer exists (MSVC, WebView2), it is launched with user confirmation.

---

### Projects & checkpoints

### P1-PROJ-001 — Project registry
- Crate(s): ak-project
- Owner: unassigned
- Depends on: P1-CORE-001
- Acceptance:
  - Create, open, list recent, and rename projects.
  - The default location is `~/AgentKnife/Projects/<name>`, and a custom path can be chosen.
  - The global AI config can be overridden per project.

### P1-PROJ-002 — `.agentknife/` files
- Crate(s): ak-project
- Owner: unassigned
- Depends on: P1-PROJ-001
- Acceptance:
  - PROJECT.md, plan/STATUS.md, and plan/tasks.md (in the AGENTS.md §8 formats) are created from the template.
  - Chat history is saved and reloaded across restarts, in the neutral `ak-types` schema.
  - A schema-version migration path is covered by a test fixture.

### P1-PROJ-003 — Embedded git checkpoints
- Crate(s): ak-project
- Owner: unassigned
- Depends on: P1-PROJ-001
- Acceptance:
  - Uses `gix`, with no system git required.
  - Supports turn, task, bundle, and user-marked checkpoints, and restore to any of them.
  - The timeline API returns plain-language entries.
  - Interrupted-turn detection works: a leftover `turn.lock` or un-checkpointed changes are reported.
  - "Go back" restores files and records the partial work as an "abandoned" entry that can still be restored.
  - Tests run in temp repos.

### P1-PROJ-004 — Checkpoint GC
- Crate(s): ak-project
- Owner: unassigned
- Depends on: P1-PROJ-003
- Acceptance:
  - Keeps the latest 50 turn checkpoints (configurable) and squashes older ones into their task checkpoint in the background.
  - Task, bundle, and user-marked checkpoints are never removed.
  - Tests cover the retention edge cases.

---

### Generated-app side: bridge, UI kit, template, knowledge

### P1-BRIDGE-001 — WebSocket invoke
- Crate(s): ak-bridge
- Owner: unassigned
- Depends on: P1-CORE-001
- Acceptance:
  - In dev builds, the app's Tauri commands are exposed over a localhost WebSocket bound to 127.0.0.1 with a random token.
  - The frontend's unified `invoke` uses WebSocket in preview and Tauri IPC in production.
  - Release builds compile all of this out, verified by a test that inspects the feature flags.

### P1-BRIDGE-002 — Inspector
- Crate(s): ak-bridge
- Owner: unassigned
- Depends on: P1-BRIDGE-001
- Acceptance:
  - In dev builds, hover and click selection report `data-ak-id`, DOM path, and bounding rect via `postMessage`.
  - On `<canvas>`, it reports coordinates and calls the optional `hit_test(x, y)` if the app implements it.

### P1-BRIDGE-003 — Control channel
- Crate(s): ak-bridge
- Owner: unassigned
- Depends on: P1-BRIDGE-001
- Acceptance:
  - In dev builds, supports: click by `data-ak-id`, type text, read DOM summary, read app state, and screenshot request.
  - Dev builds only.

### P1-UI-001 — ak-ui component kit
- Crate(s): ak-ui
- Owner: unassigned
- Depends on: P1-CORE-001
- Acceptance:
  - Leptos + Tailwind components: Button, Input, Form, List, Dialog, Nav, Card, Toggle, and Toast.
  - Every component renders a `data-ak-id` passed in by the caller.
  - A gallery example renders all of them.

### P1-TPL-001 — `tauri-leptos` template
- Crate(s): templates/tauri-leptos
- Owner: unassigned
- Depends on: P1-BRIDGE-001, P1-BRIDGE-002, P1-UI-001
- Acceptance:
  - A fresh copy builds with the doctor-installed toolchain: `trunk serve` works and `cargo tauri build` works.
  - Includes ak-bridge (dev only), ak-ui, the Tailwind standalone config, the `.agentknife/` templates, and one sample screen.
  - Versions match `versions.toml` (checked by xtask).

### P1-TPL-002 — StackProfile trait
- Crate(s): ak-types (trait), ak-preview (tauri-leptos impl)
- Owner: unassigned
- Depends on: P1-TPL-001
- Acceptance:
  - The trait covers template, knowledge pack, doctor requirements, preview strategy, and bundle strategy.
  - `tauri-leptos` is the only implementation.
  - No other crate references `tauri-leptos` directly (checked by grep in xtask).

### P1-KNOW-001 — Knowledge pack
- Crate(s): knowledge/
- Owner: unassigned
- Depends on: P1-UI-001
- Acceptance:
  - Concise docs and examples for the pinned Leptos (CSR, signals, components, view! macro), Tauri 2 commands, ak-bridge `invoke`, ak-ui, the `data-ak-id` convention, and Tailwind.
  - The agent tool `read_knowledge(topic)` serves them.
  - Version metadata matches `versions.toml`.

---

### Preview & bundling

### P1-PREV-001 — Dev-server manager
- Crate(s): ak-preview
- Owner: unassigned
- Depends on: P1-DOC-002, P1-TPL-001
- Acceptance:
  - Starts, stops, and restarts `trunk serve` for a project using the isolated toolchain.
  - Reports ready, rebuilding, and error state with compiler output.
  - Spins up the bridge backend process.
  - The port is chosen automatically.

### P1-PREV-002 — Agent-facing preview tools
- Crate(s): ak-preview, ak-agent
- Owner: unassigned
- Depends on: P1-PREV-001, P1-BRIDGE-003
- Acceptance:
  - Agent tools: `screenshot_preview` (vision models), `dom_summary` (fallback), `interact` (control channel), `get_selection` (last inspector pick), and `build_status`.
  - Build errors feed the auto-fix loop.

### P1-PREV-003 — `data-ak-id` auto-insert
- Crate(s): ak-preview
- Owner: unassigned
- Depends on: P1-TPL-001
- Acceptance:
  - A pass over `view!` macros adds stable ids to elements that lack them, without touching existing ids.
  - Idempotent.
  - Tested on sample files.

### P1-BUNDLE-001 — Host-platform bundling
- Crate(s): ak-bundle
- Owner: unassigned
- Depends on: P1-DOC-002, P1-TPL-001
- Acceptance:
  - Runs `cargo tauri build` for the host and produces: Windows NSIS `.exe`, macOS `.dmg`, Linux `.AppImage` + `.deb`.
  - Bridge and inspector are absent from the output.
  - Creates a bundle checkpoint.
  - Returns artifact paths.

### P1-BUNDLE-002 — Install guidance
- Crate(s): ak-bundle
- Owner: unassigned
- Depends on: P1-BUNDLE-001
- Acceptance:
  - Per-platform plain-language install steps in fluent, including the SmartScreen "More info → Run anyway" and Gatekeeper right-click-Open / System Settings flows.
  - An "open folder" action.
  - When the user asks for another OS, explains that they need to build on that OS.

---

### AgentKnife UI & app wiring

### P1-SHELL-001 — Layout & simple mode
- Crate(s): ak-shell
- Owner: unassigned
- Depends on: P1-I18N-001
- Acceptance:
  - Three resizable panes: files + timeline, preview + editor tabs, chat.
  - Simple mode (preview + chat only) is the default.
  - The layout persists.
  - All chrome text comes from fluent.

### P1-SHELL-002 — Home & AI connect screens
- Crate(s): ak-shell
- Owner: unassigned
- Depends on: P1-SHELL-001
- Acceptance:
  - Project list home with new, open, and recent.
  - The AI connect wizard covers cloud (Anthropic, OpenAI, compatible) and local (Ollama, LM Studio) providers, with a test-connection step, a capability warning, and keys saved to the keychain.

### P1-SHELL-003 — Chat pane
- Crate(s): ak-shell
- Owner: unassigned
- Depends on: P1-SHELL-001
- Acceptance:
  - Streaming replies.
  - Collapsed thinking.
  - Plain-language step list with expandable "Details" (raw command and output).
  - Plan card with a "Start" button.
  - Confirmation dialogs, the auto-fix fallback options, and a usage indicator.
  - Selected-element chip attached to the input.
  - On project open, the "continue or go back" prompt appears for an interrupted turn.

### P1-SHELL-004 — Preview pane
- Crate(s): ak-shell
- Owner: unassigned
- Depends on: P1-SHELL-001
- Acceptance:
  - An iframe on the dev server.
  - Device-frame switcher: desktop, phone, tablet.
  - Point-and-select mode receives inspector `postMessage` events and highlights the picked element.
  - Rebuilding and error overlays in plain language.
  - A "Run in real window" button.

### P1-SHELL-005 — Files, timeline & code editor
- Crate(s): ak-shell
- Owner: unassigned
- Depends on: P1-SHELL-001, P1-CORE-004
- Acceptance:
  - File tree.
  - Plain-language timeline with restore and "Mark this version".
  - The CodeMirror 6 editor tab, loaded from the vendored bundle through the `interop` module, can open, edit, and save, and edits trigger a preview rebuild.

### P1-SHELL-006 — Doctor & bundle screens
- Crate(s): ak-shell
- Owner: unassigned
- Depends on: P1-SHELL-003
- Acceptance:
  - Doctor progress and system-dependency guidance render inside the chat flow.
  - The bundle result shows install steps and an "open folder" button.

### P1-APP-001 — Tauri wiring
- Crate(s): ak-app
- Owner: unassigned
- Depends on: P1-AGENT-001, P1-DOC-001, P1-PROJ-001, P1-PREV-001, P1-BUNDLE-001, P1-SHELL-001
- Acceptance:
  - Tauri commands and events adapt every core crate to `ak-shell` through shared DTOs.
  - `anyhow` is used only here.
  - `cargo tauri dev` runs the full app.
  - The layering check passes.

### P1-APP-002 — Settings, privacy & crash reports
- Crate(s): ak-app, ak-shell
- Owner: unassigned
- Depends on: P1-APP-001
- Acceptance:
  - Settings: language, AI connection, permissions mode, auto-fix limit, checkpoint retention, and daily cost cap.
  - No telemetry.
  - Crash reports are written locally and sent only after explicit opt-in.

### P1-APP-003 — App packaging for AgentKnife itself
- Crate(s): ak-app, `.github/`
- Owner: coordinator
- Depends on: P1-APP-001
- Acceptance: a release workflow builds AgentKnife installers for Windows, macOS, and Linux on GitHub Actions. Auto-update stays out of scope until Phase 2.

---

### End-to-end

### P1-E2E-001 — Fake-LLM acceptance test
- Crate(s): ak-agent (test harness), ak-app
- Owner: unassigned
- Depends on: P1-APP-001, P1-AGENT-007, P1-PREV-002, P1-BUNDLE-002
- Acceptance:
  - A scripted run: connect → greeting → doctor → new project → to-do app (add and delete items, local storage) → select a button and change its color → bundle → install guidance.
  - Every step is asserted through the event stream and the file system.
  - Runs in CI on all three OSes, skipping the network-dependent doctor installs via a pre-provisioned toolchain cache.

### P1-E2E-002 — Manual clean-machine acceptance
- Crate(s): none (checklist in `docs/acceptance.md`)
- Owner: owner
- Depends on: P1-E2E-001, P1-APP-003
- Acceptance: on clean Windows 11, macOS, and Linux machines, a non-programmer completes the SPEC §9 scenario with a real AI provider. The results are recorded in `docs/acceptance.md`.
