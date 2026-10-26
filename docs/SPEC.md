# AgentKnife Product Spec

> Development rules and architecture conventions are in [`AGENTS.md`](../AGENTS.md). This document describes *what* we build and the user experience.

## 1. Product positioning

AgentKnife is a desktop development tool driven by an AI agent, for **ordinary people or people with only basic programming knowledge**. The user describes what they want in chat; the AI writes the code, builds it, fixes errors, packages it, and guides the user step by step in plain language.

## 2. Target users

- People who have never written code but want to make their own small tool or app
- People who know a little programming, want the AI to produce a finished app quickly, and sometimes want to look at or tweak the code

## 3. Screen layout

| Area | Contents |
|---|---|
| Left: files | The project's file tree; below it, the "timeline", which can go back to any earlier step |
| Center: WYSIWYG preview | The user's app running live. At the top, a switch for the device frame (desktop window / phone / tablet); the code editor is a tab that switches with the preview |
| Right: AI chat | Enter requests; shows what the AI is doing (plain-language steps), its thinking (collapsed by default), and the results |

- **Simple mode** (default): only the preview and the chat
- **Advanced mode**: opens the file area and the code editor
- Each area's width can be adjusted by dragging

## 4. User flow

### 4.1 First launch
1. Show the **project list** home page (new project / open a recent project). Projects are saved in `~/AgentKnife/Projects/<name>` by default; the user can also choose a location.
2. On first use, **connect an AI** first (set once globally; individual projects can override it):
   - Cloud: Anthropic, OpenAI, OpenAI-compatible endpoints (OpenRouter, DeepSeek, Groq, etc.)
   - Local: Ollama, LM Studio
   - API keys are stored in the operating system's keychain
   - On connecting, check whether the model supports tool calling; if it does not, or its abilities are weak, warn in plain language that "results may be worse"

### 4.2 The AI says hello
The AI greets the user in the user's language, explains what AgentKnife can do, and suggests a next step (for example, "Let's first check whether your computer has the tools we need").

### 4.3 Tool check and install (doctor)
- The program detects and installs tools automatically; the AI only explains progress in plain language and answers questions
- Tools such as Rust, Trunk, tauri-cli and Tailwind are installed in AgentKnife's own folder, **without messing up the user's computer**
- Things that must be installed system-wide (MSVC Build Tools and WebView2 on Windows; Xcode Command Line Tools on macOS; webkit packages on Linux): the AI guides the user through them step by step
- An interrupted install can be retried and resumed
- Node.js is not needed

### 4.4 Making requests, live preview
- The user describes what they want in chat, the AI changes the code, and the preview in the center updates automatically
- The preview is **a real, usable app** (buttons and data saving really work)
- **Point and pick**: click an element in the preview, for example "this button", then say how to change it
- **Small changes** are made directly; for **big features**, the AI first lists a 3–5 point plan with estimated time and cost, and starts only after the user presses "Start"
- On errors, the AI reads the error and fixes it itself, **at most 3 times**; if that still fails, it explains in plain language and lets the user choose: try another way / go back a step / see the technical details
- After a change, the AI looks at a screenshot of the preview itself and operates the app to verify the result
- Each round of changes automatically leaves a "checkpoint" on the timeline, and the user can go back to that step at any time
  - The latest 50 are kept (adjustable); older ones are merged automatically into the "task done" checkpoint they belong to
  - "Task done", "packaging", and the user's own "mark this version" checkpoints are always kept
- **Interrupted halfway** (for example a crash, a forced close, a lost connection, an AI service error): when the project is reopened, AgentKnife asks in plain language: "Last time the work was interrupted halfway (doing: …). Continue, or go back to the last checkpoint?"
  - "Continue": the AI picks up where it stopped
  - "Go back to the last checkpoint": the half-done work is saved as an "abandoned" timeline entry, so it can still be found later
- Switching to another AI midway (for example from cloud to local) does not affect the earlier chat history; work can continue directly
- A "Run in a real window" button: opens the actual app window for final testing

### 4.5 Packaging and installing
- Produce the app file for **the platform of the current computer**:
  - Windows: `.zip` (unzip and run directly; an `.exe` installer comes later)
  - macOS: `.dmg`
  - Linux: `.tar.gz` (`.deb` comes later)
  - (Phase 2) Android: `.apk`
- Installers are not signed. The AI teaches the user, in plain language and per platform, how to install, including how to handle the Windows SmartScreen and macOS "cannot be opened" warnings
- When an installer for another platform is needed, the AI explains that it has to be packaged on a computer of that platform

## 5. How the AI behaves

- The AI does most things on its own and asks the user only when necessary
- Always plain language, no jargon; technical details go under "Details"
- Permissions:
  - Reading and writing inside the project folder, building, running: done automatically
  - Downloading and installing, accessing places outside the project, deleting things: ask first in plain language
- The AI answers in **the language of the user's latest input**; when that cannot be determined, it uses the interface language (the interface supports Traditional Chinese and English first)
- When using a cloud AI, show roughly how many tokens / how much money has been used; a daily limit can be set
- Each project's `.agentknife/` folder holds:
  - `PROJECT.md`: requirements and decisions in plain language
  - `plan/STATUS.md`: current progress
  - `plan/tasks.md`: task list
  - The chat history

  When a project is reopened, the AI can carry on; when the chat gets too long, it is condensed automatically

## 6. What the user's app looks like

- Technology: Tauri 2 + Leptos + Tailwind (all Rust, no Node)
- AgentKnife provides a starter template and a set of common components (buttons, forms, lists, dialogs, navigation…)
- The tech stack is an extensible "stack profile". Other technologies such as egui can be added later, so AgentKnife can build large projects like [wordcraft](https://github.com/storytold/wordcraft)

## 7. Privacy

- No usage data is collected by default
- Crash reports are sent only with the user's explicit consent

## 8. Phases

| Phase | Contents |
|---|---|
| Phase 1 (MVP) | The three desktop platforms, AI connection, tool install, Tauri+Leptos template and components, live preview and point and pick, chat, timeline, packaging and install instructions, Traditional Chinese/English |
| Phase 2 | Android, AgentKnife auto-update, more AI providers |
| Phase 3 | iOS (only possible on a Mac), large-project mode (several AIs developing at once), the egui stack, signing and publishing, a plugin system |

## 9. MVP acceptance scenario

A person who has never written code, on a **clean Windows 11 computer**:

1. Installs AgentKnife
2. Connects an AI
3. Lets AgentKnife install the development tools
4. Using only chat, makes a "to-do list app" that can add and delete items, with the data saved on the computer
5. Midway, clicks a button in the preview and asks the AI to change its color
6. Gets the installer, and following the AI's instructions installs and runs it successfully

macOS and Linux must pass the same scenario.
