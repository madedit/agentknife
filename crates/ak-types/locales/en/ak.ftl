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
doctor-install-download = Downloading { $tool } ({ $step }/{ $total })…
doctor-install-verify = Checking the download of { $tool }…
doctor-install-unpack = Unpacking { $tool }…
doctor-install-setup = Setting up { $tool } ({ $step }/{ $total })…
doctor-install-compile = Building { $tool } from source ({ $step }/{ $total }). This can take several minutes…
doctor-install-done = { $tool } is installed.
doctor-install-failed = Could not install { $tool }.
doctor-ready = Everything you need is installed.

## Build check after the agent's changes ($attempt, $max: numbers)

agent-build-checking = Checking that the app still builds…
agent-build-ok = The app builds and runs.
agent-build-fixing = The app no longer builds. Fixing it (try { $attempt } of { $max })…

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
timeline-bundle = Packaged for installing
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
doctor-item-linux-packages = Linux system libraries

## System dependency guides (one line per step)
doctor-guide-check-again = When it has finished, click “Check again”.
doctor-guide-admin-prompt = Windows will ask whether to allow changes to your computer. Click “Yes”.
doctor-guide-msvc-what = Apps for Windows are built with Microsoft’s free C++ Build Tools. They are installed once for the whole computer (about 5 GB).
doctor-guide-msvc-winget = Click “Install” and AgentKnife starts Microsoft’s official installer. It shows its own progress window and can take 10–30 minutes.
doctor-guide-msvc-download = Open Microsoft’s download page and click “Download Build Tools”, then run the file you downloaded.
doctor-guide-msvc-workload = In the installer, tick “Desktop development with C++” and click “Install”.
doctor-guide-sdk-open-installer = The C++ Build Tools are there, but the Windows SDK is missing. Click “Open installer” to open the Visual Studio Installer.
doctor-guide-sdk-modify = Next to “Build Tools”, click “Modify”, tick “Desktop development with C++” (with “Windows 11 SDK” on the right), then click “Modify” again.
doctor-guide-webview2-what = Apps show their screens with Microsoft Edge WebView2. It is part of Windows 11 but sometimes missing on Windows 10.
doctor-guide-webview2-winget = Click “Install” and AgentKnife starts Microsoft’s official WebView2 installer. It takes about a minute.
doctor-guide-webview2-download = Open Microsoft’s download page, download the “Evergreen Bootstrapper” and run it.
doctor-guide-xcode-what = Apps for macOS are built with Apple’s free Command Line Tools (about 2 GB).
doctor-guide-xcode-dialog = Click “Install”. Apple shows a window: click “Install” there and accept the license.
doctor-guide-xcode-wait = The download can take 10–30 minutes, depending on your connection.
doctor-guide-linux-what = Apps for Linux need a few system libraries. Installing them needs your administrator password, so you run one command yourself.
doctor-guide-linux-terminal = Open the Terminal app.
doctor-guide-linux-run = Copy this command, paste it into the Terminal and press Enter:
doctor-guide-linux-password = Type your password when asked (nothing appears while you type) and press Enter. Answer “y” if asked to confirm.
doctor-guide-linux-unknown = AgentKnife does not know the package manager of this Linux. The Tauri page lists the commands for each distribution.

## Buttons and confirmations for official installers
doctor-check-again = Check again
doctor-action-winget-build-tools = Install
doctor-action-winget-webview2 = Install
doctor-action-open-vs-installer = Open installer
doctor-action-xcode-install = Install
doctor-action-open-page = Open download page
doctor-confirm-winget-build-tools = Install Microsoft C++ Build Tools? AgentKnife runs Microsoft’s official installer through Windows Package Manager (winget), which accepts Microsoft’s license for you. It downloads about 5 GB and Windows will ask for administrator permission.
doctor-confirm-winget-webview2 = Install Microsoft Edge WebView2? AgentKnife runs Microsoft’s official installer through Windows Package Manager (winget), which accepts Microsoft’s license for you.
doctor-confirm-open-vs-installer = Open the Visual Studio Installer? Windows may ask for administrator permission.
doctor-confirm-xcode-install = Install Apple’s Command Line Tools? macOS shows its own window to confirm and download them.
doctor-confirm-open-page = Open the official download page in your browser?

# Bundling
bundle-building = Building the finished app (the first time takes a few minutes)…
bundle-checking = Checking that no preview-only parts are inside…
bundle-packing = Packing the app into one file…
