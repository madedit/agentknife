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
bundle-done = Your app is ready: { $file }
bundle-open-folder = Show in folder

## Operating system names
os-windows = Windows
os-macos = macOS
os-linux = Linux

## Install guidance after bundling ($file: the packed file's name; $app: the app's name)
bundle-guide-win-extract = Right-click “{ $file }” and choose “Extract All…”, then click “Extract”.
bundle-guide-win-move = A folder named “{ $app }” appears. You can move it anywhere you like, for example to Documents. Keep everything in it together.
bundle-guide-win-run = Open the folder and double-click “{ $app }” to start the app.
bundle-guide-win-smartscreen = If Windows says “Windows protected your PC”, click “More info” and then “Run anyway”. Windows shows this for apps that are not signed by a known publisher; this is the app you made.
bundle-guide-win-shortcut = To start it from the desktop later: right-click “{ $app }”, choose “Show more options” → “Send to” → “Desktop (create shortcut)”.
bundle-guide-mac-open = Double-click “{ $file }”. A window opens with the app inside.
bundle-guide-mac-drag = Drag “{ $app }” onto the “Applications” folder in that window.
bundle-guide-mac-eject = Close the window and eject “{ $app }” in the Finder sidebar.
bundle-guide-mac-first-open = The first time: open “Applications”, hold Control and click “{ $app }”, choose “Open”, then click “Open” again. macOS asks this because the app is not signed by a known developer; this is the app you made.
bundle-guide-mac-settings = If macOS says the app cannot be opened and shows no “Open” button: open System Settings → Privacy & Security, scroll down and click “Open Anyway” next to “{ $app }”, then confirm with your password.
bundle-guide-linux-extract = Right-click “{ $file }” and choose “Extract Here”. A folder named “{ $app }” appears.
bundle-guide-linux-libraries = The app needs the WebKitGTK 4.1 library. This computer already has it. On another computer with Ubuntu or Debian, run this command in the Terminal (other Linux systems call the package “webkit2gtk4.1”):
bundle-guide-linux-run = Open the folder and double-click “{ $app }”. If your file manager opens it as text instead, right-click it and choose “Run as a Program”.
bundle-guide-open = Open “{ $file }” to start the app.
bundle-guide-share-windows = To give the app to someone else, send them “{ $file }”. They follow the same steps. It works on Windows computers only.
bundle-guide-share-macos = To give the app to someone else, send them “{ $file }”. They follow the same steps. It works on Macs only.
bundle-guide-share-linux = To give the app to someone else, send them “{ $file }”. They follow the same steps. It works on Linux computers only.
# $host, $requested: names of operating systems (os-*)
bundle-other-os = This computer runs { $host }, so AgentKnife can only make the { $host } version here. To get a { $requested } version, copy this project folder to a { $requested } computer, open it in AgentKnife there and package it again.

## AgentKnife window (shell)
shell-no-project = No project open
shell-mode = View
shell-mode-simple = Simple
shell-mode-simple-hint = Only the preview and the chat
shell-mode-full = Full
shell-mode-full-hint = Also files, version history and code
shell-files = Files
shell-files-empty = The project’s files appear here once a project is open.
shell-timeline = Version history
shell-timeline-empty = Every change is saved here, so you can always go back.
shell-tab-preview = Preview
shell-tab-code = Code
shell-tab-log = Log
log-sources = Show output of
log-source-all = All
log-source-screens = Screens
log-source-backend = Background part
log-source-window = Real window
log-source-bundle = Packaging
log-source-ai = AI steps
log-copy = Copy all
log-copied = Copied
log-clear = Clear
log-empty = Nothing has been printed yet. What the app's build, its background part, the real window, packaging and the AI's steps print shows up here.
build-log-all = Full log
build-log-shrink = Make small
build-log-expand = Show more
build-log-waiting = Waiting for the build to print something…
build-log-window = Building your app's own window…
shell-preview-empty = Your app will appear here
shell-code-empty = Pick a file on the left to see its code.
shell-chat = Chat with the AI
shell-chat-placeholder = Describe what you want…
shell-resize-files = Resize the files panel
shell-resize-chat = Resize the chat panel
shell-loading = Loading…
shell-failed = AgentKnife’s background part is not answering. Restart AgentKnife; if it keeps happening, the details help find out why.
shell-projects = ‹ Projects
shell-demo = Demo
shell-demo-hint = Running in a browser without AgentKnife: nothing is saved.
shell-ai = { $provider } · { $model }
shell-ai-none = Connect an AI
shell-ai-change = Change the AI

## Home screen
home-title = Your projects
home-new = New project
home-open-folder = Open a folder…
home-new-name = Name of the app
home-new-where = It is saved in { $folder }.
home-create = Create
home-recent = Recently opened
home-empty = No projects yet. Click “New project” to start your first app.
home-opened-today = Opened today
home-opened-yesterday = Opened yesterday
# $days (Number) at least 2
home-opened-days = Opened { $days } days ago
home-missing = Folder not found
home-delete = Delete
home-delete-title = Delete “{ $name }”?
home-delete-about = { $os ->
    [macos] “Remove from list only” keeps the folder on this computer: “Open a folder…” brings it back. Moving it to the Trash removes the folder too; you can still get it back from the Trash.
   *[other] “Remove from list only” keeps the folder on this computer: “Open a folder…” brings it back. Moving it to the Recycle Bin removes the folder too; you can still get it back from the Recycle Bin.
}
home-delete-forget = Remove from list only
home-delete-trash = { $os ->
    [macos] Move the folder to the Trash too
   *[other] Move the folder to the Recycle Bin too
}
home-error = That did not work.

## Connect wizard
connect-title = Connect an AI
connect-intro = AgentKnife needs an AI to build your app. Choose one; you can change it later.
connect-cloud = On the internet
connect-cloud-about = The smartest models. You need an account and an API key, and you pay the service for what you use.
connect-local = On this computer
connect-local-about = Free and private, but it needs a fast computer and a program that runs the model.
connect-preset-anthropic = Claude (Anthropic)
connect-preset-anthropic-about = Recommended. Very good at building apps.
connect-preset-openai = OpenAI
connect-preset-openai-about = The GPT models from OpenAI.
connect-preset-openrouter = OpenRouter
connect-preset-openrouter-about = One key for models from many companies.
connect-preset-ollama = Ollama
connect-preset-ollama-about = A free program that runs models on your computer.
connect-preset-lmstudio = LM Studio
connect-preset-lmstudio-about = A free app for downloading and running models.
connect-preset-openai-compatible = Another program
connect-preset-openai-compatible-choice = Another program (enter its address)
connect-preset-openai-compatible-about = llama.cpp, vLLM, Jan and others that work like OpenAI’s: enter its address, and a key only if it needs one. An address on another computer or on the internet works too.
connect-key = API key
connect-key-optional = API key (optional: only if the program asks for one)
connect-key-safe = Stored in your computer’s keychain, never in a file.
connect-in-use = In use
connect-set-up = Set up before
connect-current = Now using { $name } · { $model }
connect-key-saved = A key is saved; leave this empty to keep using it
connect-address = Address
connect-local-running = Make sure { $name } is running on this computer.
connect-other-running = Make sure the program is running. If the address is another computer or an online service, make sure this computer can reach it.
connect-get-key = Get an API key
connect-get-program = Download the program
connect-back = Back
connect-test = Test connection
connect-testing = Testing…
connect-ok = Connected.
connect-model = Model
connect-checking = Checking what this model can do…
connect-check-tools = This model can build apps (it can use tools).
connect-check-no-tools = This model cannot use tools, so it cannot change your app’s files. Pick another model if you can.
connect-check-use-anyway = Use it anyway
connect-check-no-vision = It cannot see images, so it checks the preview from a text description instead of screenshots.
connect-save = Connect
connect-error-unreachable = Nothing answered at that address. If it is a program on this computer, make sure it is running; otherwise check your internet connection.
connect-error-wrong-key = The API key was not accepted. Copy it again from the service’s website.
connect-error-wrong-address = Something answered, but it is not an AI service. Check the address (it usually ends in /v1).
connect-error-no-models = Connected, but there are no models yet. Download a model in the program first.
connect-error-rate-limited = The service is busy. Try again in a minute.
connect-error-other = Something went wrong. The details may help.

## Chat pane
chat-empty = Tell the AI what you want to build or change. You can also click something in the preview first.
# $what: the element the user picked in the preview
chat-selected = Selected: { $what }
chat-selected-remove = Remove the selection
chat-thinking = How the AI thought about it
chat-thinking-now = Thinking…
chat-compacted = The AI summarized the earlier conversation to make room. Nothing was lost here.
chat-allow = Allow
chat-deny = Don’t allow
chat-allowed = You allowed this step.
chat-denied = You did not allow this step.
# $estimate: the AI's own estimate ("about 3 minutes")
chat-plan-estimate = Estimate: { $estimate }
chat-plan-feedback = What should change in the plan?
chat-plan-started = You started this plan.
chat-plan-revised = You asked for changes to this plan.
chat-plan-cancelled = You cancelled this plan.
chat-answered = Your answer: { $answer }
chat-other-answer = Or type your own answer
chat-send = Send
chat-stop = Stop
chat-stopping = Stopping…
chat-continue = Continue
chat-step-limit = I took many steps and paused so you can check. Shall I continue?
chat-max-tokens = My answer was cut off because it got too long. Shall I continue?
chat-refused = The AI declined this request. Try asking in a different way.
# $spent, $cap: US dollars
chat-over-budget = Stopped for today: US${ $spent } of your US${ $cap } daily limit is used.
chat-cancelled = Stopped.
chat-failed = Something went wrong with the AI service.
# $request: what the user had asked for
chat-interrupted = Last time I stopped halfway (doing: “{ $request }”). Shall I continue, or go back to the last checkpoint?
chat-interrupted-continue = Continue
chat-interrupted-go-back = Go back to the last checkpoint
# $session, $today: US dollars
chat-usage = This session US${ $session } · today US${ $today }
chat-usage-at-least = This session at least US${ $session } · today at least US${ $today }
chat-expired = This is no longer needed: the AI stopped before you answered.

## Preview pane
preview-device = Screen size
preview-device-computer = Computer
preview-device-tablet = Tablet
preview-device-phone = Phone
preview-pick = Point at something
preview-pick-hint = Click an element in the preview to tell the AI what to change
preview-picking = Click the part of your app you want to change. Press Esc to stop.
preview-reload = Reload
preview-run-app = Run in a real window
preview-run-app-hint = Opens your app in its own window, the way it will run once installed
preview-opening-window = Opening your app in its own window… The first time takes a few minutes.
preview-window-failed = Your app could not open in its own window. Ask the AI to fix it, and show it the details.
preview-frame-title = Preview of your app
preview-off = The preview starts when the project opens.
preview-first-build = Getting the preview ready…
preview-first-build-hint = The first time takes a few minutes, because everything is built once. Later changes show up in seconds.
preview-rebuilding = Updating…
preview-error-screens = Your app’s screens cannot be built right now.
preview-error-backend = Your app’s background part stopped with an error.
preview-error-hint = The AI fixes this when it makes changes. If you changed something yourself, ask it to fix it.
preview-error-ai-working = The AI is working on it.
preview-fix = Ask the AI to fix it
# Sent to the AI as the user's message
preview-fix-message = The preview shows an error: the app no longer builds. Please fix it.

## Files, version history and code
files-unsaved = Not saved
files-save = Save
files-save-hint = Save the file (Ctrl+S); the preview updates
files-error = That did not work.
files-changed-outside = The AI changed this file. Reloading throws away your unsaved changes.
files-reload = Reload
files-keep-mine = Keep mine
files-binary = This file is not text (for example a picture), so it cannot be edited here.
timeline-mark = Mark this version
timeline-mark-name = Name for this version
timeline-mark-save = Save
timeline-current = You are here
timeline-restore-button = Go back to this version
timeline-restore-confirm = Go back to this version? What you have now is kept as “set aside”, so you can bring it back later.
timeline-restore-yes = Go back
time-just-now = Just now
# $n (Number)
time-minutes-ago = { $n } min ago
time-hours-ago = { $n } h ago
time-days-ago = { $n ->
    [one] Yesterday
   *[other] { $n } days ago
}

## Tools check and packaging in the chat
chat-check-tools = Check tools
chat-bundle = Package the app
chat-bundle-hint = Make the file to install your app on this computer or share it
doctor-title = Development tools
doctor-checking = Checking what is installed…
doctor-isolated-note = AgentKnife installs these in its own folder; nothing else on your computer changes.
doctor-install-button = Install the missing tools
doctor-system-intro = A few things must be installed on the computer itself. Here is how:
doctor-copy = Copy
doctor-copied = Copied
doctor-confirm-yes = Go ahead
doctor-error = The check did not work.
bundle-title = Packaging the app
bundle-steps-title = How to install it
bundle-other-os-question = Need it for another system?
bundle-error-build-failed = The finished app could not be built. The AI can usually fix this.
bundle-error-preview-code = The finished app still contained preview-only parts. The AI can fix this.
bundle-error-unsupported = AgentKnife cannot package apps on this kind of computer yet.
bundle-error-app-info = The app’s name or version is missing from its settings.
bundle-error-cancelled = Packaging was stopped.
bundle-error-other = Packaging did not work.
# Sent to the AI as the user's message
bundle-fix-message = Packaging the app failed because it does not build. Please fix it.

## Requests AgentKnife sends to the AI for the user (buttons in the chat)
agent-continue-request = Please continue.
agent-try-another-request = That did not work. Please try a different way.

## Settings
settings-title = Settings
settings-save = Save
settings-saved = Saved.
settings-language = Language
settings-language-system = Same as the computer
# Language names are written in their own language.
language-en = English
language-zh-TW = 繁體中文
settings-ai = AI
settings-permissions = What the AI may do on its own
settings-permissions-standard = Usual
settings-permissions-standard-about = Changes inside the project happen right away; downloads, deleting files and anything outside the project ask you first.
settings-permissions-ask = Ask me before every change
settings-permissions-ask-about = Every file change and command waits for your OK.
settings-read = Folders the AI may read
settings-read-about = The AI reads files in these folders (and all folders inside them) without asking each time. It only reads them and never changes them. Useful for documents or other code you want it to look at.
settings-read-none = No folders yet.
settings-read-add = Add a folder…
settings-read-remove = Remove
settings-plans = Plans before changes
settings-plans-about = A plan lists the steps, the time and the cost, and waits for you to click Start.
settings-plans-big = For bigger changes (the AI decides)
settings-plans-always = Before every change
settings-plans-never = Never
settings-fix = Automatic fixes
settings-fix-about = When the app stops building, the AI tries to fix it on its own this many times before it stops and asks you.
settings-fix-unit = tries
settings-history = Version history
settings-history-about = How many recent changes stay one by one. Older ones are combined into their finished task; tasks, packaged versions and versions you marked are always kept.
settings-history-unit = recent changes
settings-cap = Daily spending limit
settings-cap-about = For AI services that charge. AgentKnife warns you at 80% and asks before going over. Leave it empty for no limit.
settings-cap-currency = US$
settings-cap-none = No limit
settings-privacy = Privacy
settings-privacy-text = AgentKnife collects no usage data and sends nothing about you. Your projects, your conversations and your API keys stay on this computer (keys in the system keychain), except what you send to the AI service you chose.
settings-crash = Crash reports
settings-crash-about = If AgentKnife ever crashes, a report is saved on this computer. It is never sent automatically; to report a problem, copy a report and share it yourself.
settings-crash-none = No crash reports.
settings-crash-copy = Copy

## What each button does, shown when the mouse rests on it (title).

common-cancel-tip = Close this without changing anything
common-ok-tip = Close this message
shell-projects-tip = Back to the list of your projects (this one keeps its place)
settings-tip = Language, the AI, folders the AI may read, crash reports
shell-tab-preview-tip = Your app as it looks right now
shell-tab-code-tip = The open file's code, to read or change by hand
shell-tab-log-tip = Everything the build, your app and the AI's steps printed
chat-check-tools-tip = Check that the tools for building apps are on this computer, and get the missing ones
chat-allow-tip = Let the AI do this
chat-deny-tip = Don’t let the AI do this; it will look for another way
plan-start-tip = The AI starts working through this plan
plan-revise-tip = Tell the AI what to change in the plan before it starts
plan-cancel-tip = Drop this plan; nothing is changed
plan-send-revision-tip = Send your changes to the plan; the AI writes a new one
chat-option-tip = Answer with this choice
chat-other-send-tip = Answer with what you typed
chat-continue-tip = Let the AI carry on where it paused
stuck-try-another-tip = The AI tries again with a different approach
stuck-roll-back-tip = Undo the AI’s unfinished changes and go back to the last saved version
chat-retry-tip = Send the same message to the AI again
chat-interrupted-continue-tip = The AI picks up the unfinished request where it stopped
chat-send-tip = Send your message to the AI (Enter)
chat-stop-tip = Stop the AI now; what it already changed stays
doctor-copy-tip = Copy this command, to paste it in a terminal
doctor-action-tip = Shows what will happen first; nothing runs until you say “Go ahead”
doctor-confirm-yes-tip = Run it now (Windows or macOS may ask you to allow it)
doctor-install-tip = Download the missing tools into AgentKnife’s own folder; the rest of your computer is not touched
doctor-check-again-tip = Look again, after you installed something yourself
preview-fix-tip = Send the error to the AI and ask it to fix it
bundle-other-os-tip = How to install it on this kind of computer
bundle-open-folder-tip = Open the folder with this file
connect-preset-tip = Use this AI
connect-back-tip = Back to the previous step
connect-test-tip = Try the address and key, and list the models you can use
connect-save-tip = Use this AI from now on
files-row-tip = Open in the Code tab (folders open and close)
timeline-mark-tip = Give the current version a name, to find it again later
timeline-mark-save-tip = Save the name
timeline-restore-tip = Make the app the way it was at this version (asks first)
timeline-restore-yes-tip = Go back to this version; the newer ones stay in the list
files-reload-tip = Show the AI’s version of the file; your unsaved changes are lost
files-keep-mine-tip = Keep what you typed; saving overwrites the AI’s change
home-open-project-tip = Open this project
home-delete-tip = Remove this project from the list (asks first)
home-delete-forget-tip = Only take it off this list; the folder stays where it is
home-delete-trash-tip = { $os ->
    [macos] Take it off the list and move its folder to the Trash; you can still get it back from there
   *[other] Take it off the list and move its folder to the Recycle Bin; you can still get it back from there
}
home-open-folder-tip = Open an app folder you already have
home-new-tip = Start a new app
home-create-tip = Create the project with this name
log-filter-tip = Show only these lines
log-copy-tip = Copy the lines shown, e.g. to send when asking for help
log-clear-tip = Empty the log (nothing else changes)
build-log-all-tip = Open the Log tab with everything
build-log-toggle-tip = Make this window small or big; it closes by itself when the build is done
preview-device-tip = See the preview at this screen size
preview-reload-tip = Load the preview again from the start (doesn’t rebuild; changes show up by themselves)
settings-close-tip = Back to where you were
settings-change-ai-tip = Connect another AI service or model
settings-read-remove-tip = The AI may no longer read this folder
settings-read-add-tip = Pick a folder the AI may read without asking
settings-crash-copy-tip = Copy this report, to share it when reporting a problem
settings-crash-folder-tip = Open the folder with the crash reports
settings-save-tip = Save these settings
