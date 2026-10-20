## AgentKnife 固定介面文字（繁體中文）。
## 這裡的每個訊息都必須也存在於 en/ak.ftl（由 `cargo xtask ci` 檢查）。

app-name = AgentKnife

## 常用動作

common-ok = 確定
common-cancel = 取消
common-continue = 繼續
common-start = 開始
common-details = 詳細資料
common-retry = 再試一次

## 工具檢查進度（確定性步驟）

# $tool（字串）正在安裝的工具；$step / $total（數字）進度
doctor-installing = 正在安裝 { $tool }（{ $step }/{ $total }）…
doctor-install-download = 正在下載 { $tool }（{ $step }/{ $total }）…
doctor-install-verify = 正在檢查 { $tool } 的下載內容…
doctor-install-unpack = 正在解開 { $tool }…
doctor-install-setup = 正在設定 { $tool }（{ $step }/{ $total }）…
doctor-install-compile = 正在從原始碼建置 { $tool }（{ $step }/{ $total }），可能需要幾分鐘…
doctor-install-done = { $tool } 已安裝。
doctor-install-failed = 無法安裝 { $tool }。
doctor-ready = 需要的工具都裝好了。

## Agent 修改後的建置檢查（$attempt、$max：數字）

agent-build-checking = 正在確認 App 還能正常建置…
agent-build-ok = App 可以正常建置與執行。
agent-build-fixing = App 無法建置，正在修正（第 { $attempt } 次，最多 { $max } 次）…

## Agent 步驟確認（$detail：原始指令或路徑）

confirm-download = 這一步會從網路下載或安裝軟體：{ $detail }
confirm-delete = 這一步會刪除檔案或丟棄修改：{ $detail }
confirm-outside-project = 這一步會動到專案資料夾以外的檔案：{ $detail }
confirm-elevated = 這一步需要系統管理員權限：{ $detail }
confirm-unrecognized = AgentKnife 無法判斷這一步會做什麼：{ $detail }
confirm-change = 你設定了「每次修改都要確認」。要允許這一步嗎？{ $detail }
denied-path-escape = 已阻擋：這在你的專案資料夾以外：{ $detail }
denied-protected-path = 已阻擋：這是 AgentKnife 自己的歷史紀錄，不能直接修改：{ $detail }
denied-dangerous = 已阻擋：這可能會損壞你的電腦：{ $detail }

## 時間軸項目（檢查點）

timeline-initial = 起點
timeline-turn = 修改
timeline-task = 完成一個任務
timeline-bundle = 產生了安裝檔
timeline-marked = 標記的版本
timeline-abandoned = 暫存起來（可以找回來）
timeline-restore = 回到之前的版本

## 計畫與卡住時

plan-title = 這是我的計畫
plan-start = 開始
plan-revise = 修改計畫
plan-cancel = 取消
# $attempts（數字）已自動修正的次數
stuck-title = 我已經自動修正 { $attempts } 次，還是失敗。
stuck-try-another = 換個做法試試
stuck-roll-back = 回到上一個檢查點
stuck-show-details = 顯示技術細節

## 用量與花費（$detail：「US$1.23 / US$1.00」）

confirm-daily-cap = 今天的 AI 花費已經達到你設定的每日上限（{ $detail }）。今天要繼續使用嗎？
# $spent、$cap（數字，美元）
budget-warning = 今天已經用了 { $spent }，每日上限是 { $cap }（美元）。
usage-free = 免費（在你的電腦上執行）
usage-unknown = 無法得知這個 AI 服務的費用

## 開發工具檢查（doctor）

doctor-item-rust = Rust
doctor-item-wasm32-target = Rust 網頁版（WebAssembly）
doctor-item-trunk = Trunk（建置 App 畫面）
doctor-item-wasm-bindgen = wasm-bindgen
doctor-item-tauri-cli = Tauri 工具（打包 App）
doctor-item-tailwind = Tailwind CSS（樣式）
doctor-item-msvc-build-tools = Microsoft C++ 建置工具
doctor-item-windows-sdk = Windows SDK
doctor-item-webview2 = Microsoft Edge WebView2
doctor-item-xcode-clt = Xcode 命令列工具
doctor-item-linux-webkit2gtk = WebKitGTK（webkit2gtk-4.1）
doctor-item-linux-javascriptcore = JavaScriptCore（javascriptcoregtk-4.1）
doctor-item-linux-libsoup = libsoup 3
doctor-item-linux-appindicator = Ayatana AppIndicator
doctor-item-linux-librsvg = librsvg
doctor-item-linux-c-compiler = C 編譯器（build-essential）
doctor-status-ok = 已就緒
doctor-status-missing = 尚未安裝
doctor-status-wrong-version = 需要不同的版本
doctor-status-error = 無法檢查
doctor-item-linux-packages = Linux 系統函式庫

## 系統相依項目的引導（每行一個步驟）
doctor-guide-check-again = 完成後，按「再檢查一次」。
doctor-guide-admin-prompt = Windows 會詢問是否允許變更你的電腦，請按「是」。
doctor-guide-msvc-what = Windows 版的 App 要用 Microsoft 免費的 C++ 建置工具來建置。整台電腦只需安裝一次（約 5 GB）。
doctor-guide-msvc-winget = 按「安裝」，AgentKnife 會啟動 Microsoft 官方安裝程式。它會顯示自己的進度視窗，大約需要 10–30 分鐘。
doctor-guide-msvc-download = 打開 Microsoft 的下載頁面，按「Download Build Tools」，然後執行下載的檔案。
doctor-guide-msvc-workload = 在安裝程式中勾選「使用 C++ 的桌面開發」（Desktop development with C++），然後按「安裝」。
doctor-guide-sdk-open-installer = C++ 建置工具已經有了，但缺少 Windows SDK。按「開啟安裝程式」打開 Visual Studio Installer。
doctor-guide-sdk-modify = 在「Build Tools」旁按「修改」，勾選「使用 C++ 的桌面開發」（右側要包含「Windows 11 SDK」），再按「修改」。
doctor-guide-webview2-what = App 用 Microsoft Edge WebView2 顯示畫面。Windows 11 內建，但 Windows 10 有時沒有。
doctor-guide-webview2-winget = 按「安裝」，AgentKnife 會啟動 Microsoft 官方的 WebView2 安裝程式，大約一分鐘。
doctor-guide-webview2-download = 打開 Microsoft 的下載頁面，下載「Evergreen Bootstrapper」並執行它。
doctor-guide-xcode-what = macOS 版的 App 要用 Apple 免費的命令列工具來建置（約 2 GB）。
doctor-guide-xcode-dialog = 按「安裝」。Apple 會跳出視窗，在那裡也按「安裝」並同意授權條款。
doctor-guide-xcode-wait = 下載大約需要 10–30 分鐘，視網路速度而定。
doctor-guide-linux-what = Linux 版的 App 需要幾個系統函式庫。安裝它們需要管理員密碼，所以請你自己執行一行指令。
doctor-guide-linux-terminal = 打開「終端機」（Terminal）。
doctor-guide-linux-run = 複製這行指令，貼到終端機，然後按 Enter：
doctor-guide-linux-password = 被要求時輸入你的密碼（輸入時不會顯示任何字），按 Enter。如果詢問是否繼續，輸入「y」。
doctor-guide-linux-unknown = AgentKnife 不認得這個 Linux 的套件管理工具。Tauri 的頁面列出了各發行版的指令。

## 官方安裝程式的按鈕與確認
doctor-check-again = 再檢查一次
doctor-action-winget-build-tools = 安裝
doctor-action-winget-webview2 = 安裝
doctor-action-open-vs-installer = 開啟安裝程式
doctor-action-xcode-install = 安裝
doctor-action-open-page = 打開下載頁面
doctor-confirm-winget-build-tools = 要安裝 Microsoft C++ 建置工具嗎？AgentKnife 會透過 Windows 套件管理員（winget）執行 Microsoft 官方安裝程式，並代你接受 Microsoft 的授權條款。會下載約 5 GB，Windows 會要求管理員權限。
doctor-confirm-winget-webview2 = 要安裝 Microsoft Edge WebView2 嗎？AgentKnife 會透過 Windows 套件管理員（winget）執行 Microsoft 官方安裝程式，並代你接受 Microsoft 的授權條款。
doctor-confirm-open-vs-installer = 要打開 Visual Studio Installer 嗎？Windows 可能會要求管理員權限。
doctor-confirm-xcode-install = 要安裝 Apple 的命令列工具嗎？macOS 會顯示自己的視窗來確認並下載。
doctor-confirm-open-page = 要在瀏覽器打開官方下載頁面嗎？
