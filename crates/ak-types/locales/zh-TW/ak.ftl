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
timeline-bundle = 打包了 App
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

# Bundling
bundle-building = 正在建置完成版 App（第一次要幾分鐘）…
bundle-checking = 正在確認裡面沒有預覽專用的部分…
bundle-packing = 正在把 App 打包成一個檔案…
bundle-done = App 打包好了：{ $file }
bundle-open-folder = 在資料夾中顯示

## 作業系統名稱
os-windows = Windows
os-macos = macOS
os-linux = Linux

## 打包後的安裝說明（$file：打包出來的檔名；$app：App 的名稱）
bundle-guide-win-extract = 在「{ $file }」上按右鍵，選「解壓縮全部…」，再按「解壓縮」。
bundle-guide-win-move = 會出現一個叫「{ $app }」的資料夾。你可以把它搬到任何地方，例如「文件」。裡面的東西請放在一起。
bundle-guide-win-run = 打開這個資料夾，按兩下「{ $app }」就能啟動 App。
bundle-guide-win-smartscreen = 如果 Windows 顯示「Windows 已保護您的電腦」，請按「其他資訊」，再按「仍要執行」。沒有知名發行者簽章的 App 都會出現這個畫面；這是你自己做的 App。
bundle-guide-win-shortcut = 想以後從桌面啟動：在「{ $app }」上按右鍵，選「顯示其他選項」→「傳送到」→「桌面 (建立捷徑)」。
bundle-guide-mac-open = 按兩下「{ $file }」，會開出一個裝著 App 的視窗。
bundle-guide-mac-drag = 把「{ $app }」拖到那個視窗裡的「應用程式」資料夾。
bundle-guide-mac-eject = 關掉視窗，在 Finder 側邊欄把「{ $app }」退出。
bundle-guide-mac-first-open = 第一次打開時：打開「應用程式」，按住 Control 鍵再點「{ $app }」，選「打開」，然後再按一次「打開」。macOS 會問這個，是因為 App 沒有知名開發者的簽章；這是你自己做的 App。
bundle-guide-mac-settings = 如果 macOS 說無法打開，而且沒有「打開」按鈕：打開「系統設定」→「隱私權與安全性」，往下捲，在「{ $app }」旁邊按「強制打開」，再輸入密碼確認。
bundle-guide-linux-extract = 在「{ $file }」上按右鍵，選「在此解壓縮」，會出現一個叫「{ $app }」的資料夾。
bundle-guide-linux-libraries = 這個 App 需要 WebKitGTK 4.1 函式庫，這台電腦已經有了。在另一台 Ubuntu 或 Debian 電腦上，請在終端機執行這個指令（其他 Linux 的套件名稱是「webkit2gtk4.1」）：
bundle-guide-linux-run = 打開資料夾，按兩下「{ $app }」。如果檔案管理員把它當文字檔打開，請按右鍵選「當作程式執行」。
bundle-guide-open = 打開「{ $file }」就能啟動 App。
bundle-guide-share-windows = 想把 App 給別人：把「{ $file }」傳給對方，照同樣的步驟做就行。只能在 Windows 電腦上用。
bundle-guide-share-macos = 想把 App 給別人：把「{ $file }」傳給對方，照同樣的步驟做就行。只能在 Mac 上用。
bundle-guide-share-linux = 想把 App 給別人：把「{ $file }」傳給對方，照同樣的步驟做就行。只能在 Linux 電腦上用。
# $host、$requested：作業系統名稱（os-*）
bundle-other-os = 這台電腦是 { $host }，所以 AgentKnife 在這裡只能做出 { $host } 版。想要 { $requested } 版的話，請把這個專案資料夾複製到一台 { $requested } 電腦，在那裡用 AgentKnife 打開再打包一次。

## AgentKnife 視窗（shell）
shell-no-project = 還沒有打開專案
shell-mode = 畫面
shell-mode-simple = 簡單
shell-mode-simple-hint = 只顯示預覽和對話
shell-mode-full = 完整
shell-mode-full-hint = 也顯示檔案、版本紀錄和程式碼
shell-files = 檔案
shell-files-empty = 打開專案後，檔案會列在這裡。
shell-timeline = 版本紀錄
shell-timeline-empty = 每次修改都會記在這裡，隨時可以回到之前的版本。
shell-tab-preview = 預覽
shell-tab-code = 程式碼
shell-tab-log = 記錄
log-sources = 顯示哪些輸出
log-source-all = 全部
log-source-screens = 畫面
log-source-backend = 背景程式
log-source-window = 真正的視窗
log-source-bundle = 打包
log-source-ai = AI 步驟
log-copy = 全部複製
log-copied = 已複製
log-clear = 清除
log-empty = 還沒有任何輸出。App 建置、背景程式、真正的視窗、打包和 AI 步驟印出的內容都會顯示在這裡。
build-log-all = 完整記錄
build-log-shrink = 縮小
build-log-expand = 展開
build-log-waiting = 等待建置輸出…
build-log-window = 正在建置 App 自己的視窗…
shell-preview-empty = 你的 App 會顯示在這裡
shell-code-empty = 在左邊選一個檔案，就能看到它的程式碼。
shell-chat = 與 AI 對話
shell-chat-placeholder = 說說你想做什麼…
shell-resize-files = 調整檔案面板寬度
shell-resize-chat = 調整對話面板寬度
shell-loading = 載入中…
shell-failed = AgentKnife 的背景程式沒有回應。請重新開啟 AgentKnife；如果一直發生，下面的詳細資訊可以幫忙找出原因。
shell-projects = ‹ 專案
shell-demo = 示範
shell-demo-hint = 在瀏覽器裡執行，沒有連到 AgentKnife：不會儲存任何東西。
shell-ai = { $provider } · { $model }
shell-ai-none = 連接 AI
shell-ai-change = 更換 AI

## 首頁
home-title = 你的專案
home-new = 新專案
home-open-folder = 打開資料夾…
home-new-name = App 名稱
home-new-where = 會存在 { $folder }。
home-create = 建立
home-recent = 最近打開
home-empty = 還沒有專案。按「新專案」開始做你的第一個 App。
home-opened-today = 今天打開過
home-opened-yesterday = 昨天打開過
# $days（數字）至少 2
home-opened-days = { $days } 天前打開過
home-missing = 找不到資料夾
home-delete = 刪除
home-delete-title = 要刪除「{ $name }」嗎？
home-delete-about = { $os ->
    [macos] 「只從清單移除」會把資料夾留在這台電腦上，之後可以用「打開資料夾…」加回來。移到垃圾桶會連資料夾一起丟掉，需要的話還能從垃圾桶救回。
   *[other] 「只從清單移除」會把資料夾留在這台電腦上，之後可以用「打開資料夾…」加回來。移到資源回收筒會連資料夾一起丟掉，需要的話還能從資源回收筒救回。
}
home-delete-forget = 只從清單移除
home-delete-trash = { $os ->
    [macos] 連資料夾一起移到垃圾桶
   *[other] 連資料夾一起移到資源回收筒
}
home-error = 沒有成功。

## 連接 AI 精靈
connect-title = 連接 AI
connect-intro = AgentKnife 需要一個 AI 來幫你做 App。選一個吧，之後隨時可以換。
connect-cloud = 網路上的服務
connect-cloud-about = 最聰明的模型。需要帳號和 API 金鑰，用多少就付多少給該服務。
connect-local = 這台電腦上
connect-local-about = 免費又私密，但需要夠快的電腦，以及一個執行模型的程式。
connect-preset-anthropic = Claude（Anthropic）
connect-preset-anthropic-about = 推薦。很擅長做 App。
connect-preset-openai = OpenAI
connect-preset-openai-about = OpenAI 的 GPT 模型。
connect-preset-openrouter = OpenRouter
connect-preset-openrouter-about = 一把金鑰就能用很多家公司的模型。
connect-preset-ollama = Ollama
connect-preset-ollama-about = 在你電腦上執行模型的免費程式。
connect-preset-lmstudio = LM Studio
connect-preset-lmstudio-about = 下載並執行模型的免費 App。
connect-preset-openai-compatible = 其他程式
connect-preset-openai-compatible-choice = 其他程式（自己填網址）
connect-preset-openai-compatible-about = llama.cpp、vLLM、Jan 等和 OpenAI 用法相同的程式：填它的網址，需要金鑰才填。別台電腦或網路上服務的網址也可以。
connect-key = API 金鑰
connect-key-optional = API 金鑰（選填，程式有要求才需要）
connect-key-safe = 會存在電腦的系統鑰匙圈，不會寫進任何檔案。
connect-in-use = 使用中
connect-set-up = 設定過
connect-current = 目前使用：{ $name } · { $model }
connect-key-saved = 已存好金鑰，不用再輸入（要換新的才填）
connect-address = 網址
connect-local-running = 請確認 { $name } 已經在這台電腦上執行。
connect-other-running = 請確認那個程式已經在執行。網址是別台電腦或網路上的服務時，請確認這台電腦連得到它。
connect-get-key = 取得 API 金鑰
connect-get-program = 下載程式
connect-back = 上一步
connect-test = 測試連線
connect-testing = 測試中…
connect-ok = 連上了。
connect-model = 模型
connect-checking = 正在確認這個模型能做什麼…
connect-check-tools = 這個模型可以做 App（能使用工具）。
connect-check-no-tools = 這個模型不能使用工具，所以沒辦法修改 App 的檔案。可以的話請換一個模型。
connect-check-use-anyway = 還是要用
connect-check-no-vision = 它看不到圖片，所以會用文字描述而不是截圖來檢查預覽。
connect-save = 連接
connect-error-unreachable = 那個網址沒有回應。如果是這台電腦上的程式，請確認它正在執行；否則請檢查網路連線。
connect-error-wrong-key = API 金鑰沒有被接受。請到該服務的網站重新複製一次。
connect-error-wrong-address = 有回應，但不是 AI 服務。請檢查網址（通常以 /v1 結尾）。
connect-error-no-models = 連上了，但還沒有任何模型。請先在程式裡下載一個模型。
connect-error-rate-limited = 服務太忙了，請過一分鐘再試。
connect-error-other = 發生問題了，詳細資訊可能有幫助。

## 對話面板
chat-empty = 告訴 AI 你想做什麼或改什麼。也可以先在預覽裡點一下要改的地方。
# $what：使用者在預覽裡選的元素
chat-selected = 已選取：{ $what }
chat-selected-remove = 取消選取
chat-thinking = AI 的思考過程
chat-thinking-now = 思考中…
chat-compacted = AI 把之前的對話整理成摘要以騰出空間。這裡的內容都還在。
chat-allow = 允許
chat-deny = 不允許
chat-allowed = 你允許了這個步驟。
chat-denied = 你沒有允許這個步驟。
# $estimate：AI 自己的估計（「大約 3 分鐘」）
chat-plan-estimate = 預估：{ $estimate }
chat-plan-feedback = 計畫要怎麼改？
chat-plan-started = 你開始了這個計畫。
chat-plan-revised = 你要求修改這個計畫。
chat-plan-cancelled = 你取消了這個計畫。
chat-answered = 你的回答：{ $answer }
chat-other-answer = 或自己輸入答案
chat-send = 送出
chat-stop = 停止
chat-stopping = 正在停止…
chat-continue = 繼續
chat-step-limit = 我已經做了很多步，先停下來讓你看看。要繼續嗎？
chat-max-tokens = 我的回答太長被截斷了。要繼續嗎？
chat-refused = AI 拒絕了這個要求。請換個方式問問看。
# $spent、$cap：美元
chat-over-budget = 今天先停在這裡：每日上限 US${ $cap } 已經用了 US${ $spent }。
chat-cancelled = 已停止。
chat-failed = AI 服務出了問題。
# $request：使用者之前的要求
chat-interrupted = 上次做到一半中斷了（正在：「{ $request }」）。要接著做，還是回到上一個檢查點？
chat-interrupted-continue = 接著做
chat-interrupted-go-back = 回到上一個檢查點
# $session、$today：美元
chat-usage = 這次 US${ $session } · 今天 US${ $today }
chat-usage-at-least = 這次至少 US${ $session } · 今天至少 US${ $today }
chat-expired = 不需要再回答了：AI 在你回答之前就停下來了。

## 預覽面板
preview-device = 畫面大小
preview-device-computer = 電腦
preview-device-tablet = 平板
preview-device-phone = 手機
preview-pick = 點選要改的地方
preview-pick-hint = 在預覽裡點一個元素，告訴 AI 要改哪裡
preview-picking = 請點一下 App 裡你想改的地方。按 Esc 結束。
preview-reload = 重新整理
preview-run-app = 用真正的視窗執行
preview-run-app-hint = 用 App 自己的視窗開啟，和安裝後執行的樣子一樣
preview-opening-window = 正在用 App 自己的視窗開啟…第一次會花幾分鐘。
preview-window-failed = App 沒辦法用自己的視窗開啟。可以請 AI 修正，並把詳細資料給它看。
preview-frame-title = 你的 App 預覽
preview-off = 打開專案後，預覽就會啟動。
preview-first-build = 正在準備預覽…
preview-first-build-hint = 第一次要幾分鐘，因為所有東西都要先建置一次。之後的修改幾秒內就會出現。
preview-rebuilding = 更新中…
preview-error-screens = App 的畫面目前無法建置。
preview-error-backend = App 的背景程式出錯停止了。
preview-error-hint = AI 修改時會順便修好。如果是你自己改的，可以請 AI 修正。
preview-error-ai-working = AI 正在處理。
preview-fix = 請 AI 修正
# 以使用者的身分傳給 AI
preview-fix-message = 預覽顯示錯誤：App 無法建置了，請幫我修正。

## 檔案、版本紀錄和程式碼
files-unsaved = 尚未儲存
files-save = 儲存
files-save-hint = 儲存檔案（Ctrl+S）；預覽會跟著更新
files-error = 沒有成功。
files-changed-outside = AI 修改了這個檔案。重新載入會丟掉你還沒儲存的修改。
files-reload = 重新載入
files-keep-mine = 保留我的
files-binary = 這個檔案不是文字（例如圖片），沒辦法在這裡編輯。
timeline-mark = 標記這一版
timeline-mark-name = 這一版的名稱
timeline-mark-save = 儲存
timeline-current = 目前在這一版
timeline-restore-button = 回到這一版
timeline-restore-confirm = 要回到這一版嗎？目前的內容會先暫存起來，之後還找得回來。
timeline-restore-yes = 回到這一版
time-just-now = 剛剛
# $n（數字）
time-minutes-ago = { $n } 分鐘前
time-hours-ago = { $n } 小時前
time-days-ago = { $n } 天前

## 對話中的工具檢查和打包
chat-check-tools = 檢查工具
chat-bundle = 打包 App
chat-bundle-hint = 做出可以在這台電腦安裝、也能分享給別人的檔案
doctor-title = 開發工具
doctor-checking = 正在檢查裝了哪些工具…
doctor-isolated-note = 這些工具會裝在 AgentKnife 自己的資料夾，不會動到電腦的其他地方。
doctor-install-button = 安裝缺少的工具
doctor-system-intro = 有幾樣東西必須裝在電腦本身。做法如下：
doctor-copy = 複製
doctor-copied = 已複製
doctor-confirm-yes = 好，開始
doctor-error = 檢查沒有成功。
bundle-title = 打包 App
bundle-steps-title = 安裝方式
bundle-other-os-question = 需要其他系統的版本？
bundle-error-build-failed = 完成版的 App 建置失敗。通常 AI 可以修好。
bundle-error-preview-code = 完成版裡還有預覽專用的部分。AI 可以修好。
bundle-error-unsupported = AgentKnife 還不能在這種電腦上打包 App。
bundle-error-app-info = App 的設定裡缺少名稱或版本。
bundle-error-cancelled = 打包已停止。
bundle-error-other = 打包沒有成功。
# 以使用者的身分傳給 AI
bundle-fix-message = 打包 App 失敗了，因為它無法建置。請幫我修正。

## AgentKnife 代使用者傳給 AI 的要求（對話中的按鈕）
agent-continue-request = 請繼續。
agent-try-another-request = 這樣行不通，請換個做法試試。

## 設定
settings-title = 設定
settings-save = 儲存
settings-saved = 已儲存。
settings-language = 語言
settings-language-system = 跟電腦一樣
# 語言名稱用該語言本身書寫。
language-en = English
language-zh-TW = 繁體中文
settings-ai = AI
settings-permissions = AI 可以自己做的事
settings-permissions-standard = 一般
settings-permissions-standard-about = 專案內的修改會直接進行；下載、刪除檔案和專案外的事會先問你。
settings-permissions-ask = 每個修改都先問我
settings-permissions-ask-about = 每次改檔案、執行指令都會等你同意。
settings-read = AI 可以直接讀取的資料夾
settings-read-about = AI 讀這些資料夾（包含裡面所有子資料夾）的檔案時，不會每次都問你。它只會讀，不會修改。適合放你想讓 AI 參考的文件或其他程式。
settings-read-none = 還沒有加入資料夾。
settings-read-add = 加入資料夾…
settings-read-remove = 移除
settings-plans = 修改前先給計畫
settings-plans-about = 計畫會列出步驟、時間和花費，等你按「開始」才動手。
settings-plans-big = 比較大的修改（由 AI 判斷）
settings-plans-always = 每次修改前
settings-plans-never = 不用
settings-fix = 自動修正
settings-fix-about = App 無法建置時，AI 會自己修正最多這麼多次，還不行才停下來問你。
settings-fix-unit = 次
settings-history = 版本紀錄
settings-history-about = 最近的修改會逐筆保留這麼多筆，更早的會併入所屬的完成任務；任務、打包過的版本和你標記的版本永遠保留。
settings-history-unit = 筆最近的修改
settings-cap = 每日花費上限
settings-cap-about = 給要收費的 AI 服務用。用到 80% 時會提醒你，超過前會先問你。留空就是不設上限。
settings-cap-currency = US$
settings-cap-none = 不設上限
settings-privacy = 隱私
settings-privacy-text = AgentKnife 不收集任何使用資料，也不會傳送關於你的任何資訊。你的專案、對話和 API 金鑰都留在這台電腦上（金鑰存在系統鑰匙圈），只有你傳給所選 AI 服務的內容例外。
settings-crash = 當機報告
settings-crash-about = 如果 AgentKnife 當機，會在這台電腦上存一份報告。報告絕不會自動傳送；想回報問題時，可以複製報告自己分享。
settings-crash-none = 沒有當機報告。
settings-crash-copy = 複製

## 滑鼠停在按鈕上時顯示的說明（title）。

common-cancel-tip = 關掉這裡，什麼都不改
common-ok-tip = 關掉這則訊息
shell-projects-tip = 回到專案列表（這個專案會停在現在的位置）
settings-tip = 語言、AI、AI 可以讀的資料夾、當機報告
shell-tab-preview-tip = 你的 App 現在的樣子
shell-tab-code-tip = 打開的檔案的程式碼，可以看或自己修改
shell-tab-log-tip = 建置、你的 App 和 AI 步驟印出的所有內容
chat-check-tools-tip = 檢查這台電腦有沒有做 App 需要的工具，缺的會幫你裝
chat-allow-tip = 讓 AI 做這件事
chat-deny-tip = 不讓 AI 做這件事，它會換別的方法
plan-start-tip = AI 開始照這個計畫做
plan-revise-tip = 開始前告訴 AI 計畫要改哪裡
plan-cancel-tip = 放棄這個計畫，什麼都不會改
plan-send-revision-tip = 送出你對計畫的修改，AI 會重寫一份
chat-option-tip = 用這個選項回答
chat-other-send-tip = 用你輸入的內容回答
chat-continue-tip = 讓 AI 從暫停的地方繼續
stuck-try-another-tip = AI 換一種做法再試
stuck-roll-back-tip = 復原 AI 沒做完的修改，回到上一個存好的版本
chat-retry-tip = 把同樣的訊息再送給 AI 一次
chat-interrupted-continue-tip = AI 從中斷的地方接著做完上次的要求
chat-send-tip = 把訊息送給 AI（Enter）
chat-stop-tip = 馬上停下 AI，已經改好的部分會留著
doctor-copy-tip = 複製這行指令，可以貼到終端機
doctor-action-tip = 會先說明要做什麼，你按「好，開始」之前不會執行
doctor-confirm-yes-tip = 現在執行（Windows 或 macOS 可能會問你是否允許）
doctor-install-tip = 把缺的工具下載到 AgentKnife 自己的資料夾，不會動到電腦其他地方
doctor-check-again-tip = 你自己裝好東西之後，再檢查一次
preview-fix-tip = 把錯誤交給 AI，請它修好
bundle-other-os-tip = 在這種電腦上怎麼安裝
bundle-open-folder-tip = 打開這個檔案所在的資料夾
connect-preset-tip = 用這個 AI
connect-back-tip = 回到上一步
connect-test-tip = 試試看位址和金鑰，並列出可以用的模型
connect-save-tip = 之後都用這個 AI
files-row-tip = 在「程式碼」分頁打開（資料夾會展開或收起）
timeline-mark-tip = 幫現在的版本取個名字，之後比較好找
timeline-mark-save-tip = 儲存這個名字
timeline-restore-tip = 讓 App 回到這個版本的樣子（會先問你）
timeline-restore-yes-tip = 回到這個版本；比較新的版本還會留在清單裡
files-reload-tip = 顯示 AI 改過的檔案，你還沒存的修改會不見
files-keep-mine-tip = 保留你打的內容；存檔時會蓋掉 AI 的修改
home-open-project-tip = 打開這個專案
home-delete-tip = 把這個專案從清單移除（會先問你）
home-delete-forget-tip = 只從這個清單拿掉，資料夾留在原地
home-delete-trash-tip = { $os ->
    [macos] 從清單拿掉，資料夾移到垃圾桶，之後還能從那裡救回來
   *[other] 從清單拿掉，資料夾移到資源回收筒，之後還能從那裡救回來
}
home-open-folder-tip = 打開你已經有的 App 資料夾
home-new-tip = 開始做一個新的 App
home-create-tip = 用這個名字建立專案
log-filter-tip = 只顯示這類的記錄
log-copy-tip = 複製目前顯示的記錄，例如求助時可以貼給別人
log-clear-tip = 清空記錄（其他東西都不會變）
build-log-all-tip = 打開「記錄」分頁看全部
build-log-toggle-tip = 把這個視窗縮小或放大；建置完成後會自己關掉
preview-device-tip = 用這個螢幕大小看預覽
preview-reload-tip = 把預覽從頭重新載入（不會重新建置；修改本來就會自動出現）
settings-close-tip = 回到剛才的畫面
settings-change-ai-tip = 換一個 AI 服務或模型
settings-read-remove-tip = AI 之後不能再讀這個資料夾
settings-read-add-tip = 選一個 AI 不用每次問就可以讀的資料夾
settings-crash-copy-tip = 複製這份報告，回報問題時可以附上
settings-crash-folder-tip = 打開放當機報告的資料夾
settings-save-tip = 儲存這些設定
