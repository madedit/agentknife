## AgentKnife fixed UI strings (English).
## Every message here must also exist in zh-TW/ak.ftl (checked by `cargo xtask ci`).

app-name = AgentKnife

## Common actions

common-ok = OK
common-cancel = Cancel
common-continue = Continue
common-start = Start
common-details = Details
common-retry = Try again

## Doctor progress (deterministic steps)

# $tool (String) tool being installed; $step / $total (Number) progress
doctor-installing = Installing { $tool } ({ $step }/{ $total })…
doctor-ready = Everything you need is installed.

## Agent step confirmations ($detail: the command or path, shown verbatim)

confirm-download = This step downloads or installs software from the internet: { $detail }
confirm-delete = This step deletes files or throws away changes: { $detail }
confirm-outside-project = This step touches files outside your project folder: { $detail }
confirm-elevated = This step asks for administrator rights: { $detail }
confirm-unrecognized = AgentKnife cannot tell what this step will do: { $detail }
confirm-change = You asked to approve every change. Allow this one? { $detail }
denied-path-escape = Blocked: this is outside your project folder: { $detail }
denied-protected-path = Blocked: this is AgentKnife's own history and must not be edited by hand: { $detail }
denied-dangerous = Blocked: this could damage your computer: { $detail }

## Timeline entries (checkpoints)

timeline-initial = Starting point
timeline-turn = Change
timeline-task = Task finished
timeline-bundle = Installer built
timeline-marked = Marked version
timeline-abandoned = Set aside (can be brought back)
timeline-restore = Went back to an earlier version

## Plans and getting stuck

plan-title = Here is my plan
plan-start = Start
plan-revise = Change the plan
plan-cancel = Cancel
# $attempts (Number) automatic fix attempts made
stuck-title = I tried to fix this { $attempts } times and it still fails.
stuck-try-another = Try a different way
stuck-roll-back = Go back to the last checkpoint
stuck-show-details = Show technical details

## Usage and spending ($detail: "US$1.23 of US$1.00")

confirm-daily-cap = Today's AI spending has reached your daily limit ({ $detail }). Keep going today?
# $spent, $cap (Number, US dollars)
budget-warning = You have used { $spent } of today's { $cap } limit (US dollars).
usage-free = Free (runs on your computer)
usage-unknown = Cost unknown for this AI service

## Development tools check (doctor)

doctor-item-rust = Rust
doctor-item-wasm32-target = Rust for the web (WebAssembly)
doctor-item-trunk = Trunk (builds the app's screens)
doctor-item-wasm-bindgen = wasm-bindgen
doctor-item-tauri-cli = Tauri tools (packages the app)
doctor-item-tailwind = Tailwind CSS (styles)
doctor-item-msvc-build-tools = Microsoft C++ Build Tools
doctor-item-windows-sdk = Windows SDK
doctor-item-webview2 = Microsoft Edge WebView2
doctor-item-xcode-clt = Xcode Command Line Tools
doctor-item-linux-webkit2gtk = WebKitGTK (webkit2gtk-4.1)
doctor-item-linux-javascriptcore = JavaScriptCore (javascriptcoregtk-4.1)
doctor-item-linux-libsoup = libsoup 3
doctor-item-linux-appindicator = Ayatana AppIndicator
doctor-item-linux-librsvg = librsvg
doctor-item-linux-c-compiler = C compiler (build-essential)
doctor-status-ok = Ready
doctor-status-missing = Not installed
doctor-status-wrong-version = Different version needed
doctor-status-error = Could not check
