# AgentKnife

[English](README.md) | 繁體中文

AgentKnife 是給 AI agent 用的瑞士刀。沒有程式基礎的人，只要對 AI 說幾句想要什麼，AI 就會用這把刀幫他們做出程式。它是一個桌面版的開發工具（IDE）：在對話框描述你的想法，在即時預覽裡看著它出現，最後拿到一個安裝檔。

預覽裡跑的是真正的 App，不是一張示意圖：點一下任何地方，再跟 AI 說要怎麼改（見[所見即所得](#所見即所得wysiwyg)）。

每一輪修改都會存進版本紀錄，隨時可以回到之前任何一步，也可以把想保留的版本標記起來。

全部都用 Rust。AgentKnife 做出來的 App，後端用 [Tauri 2](https://v2.tauri.app/)（原生視窗、檔案與系統存取），前端用編譯成 WebAssembly 的 [Leptos](https://leptos.dev/)，樣式用 [Tailwind CSS](https://tailwindcss.com/)。AgentKnife 本身也是用同一套做的。

適合用來快速做 demo 和原型、試做 UI 介面，也能用在教學上：學生請 AI 做一個演算法，就能在預覽裡看著它一步一步執行。

## 所見即所得（WYSIWYG）

What You See Is What You Get：預覽視窗裡跑的就是你的 App 本身，你在預覽裡看到的，就是最後做出來的樣子。

想改東西有兩種方式：

- **點選它**：按「點選要改的地方」，在預覽裡點你想改的元件（按鈕、清單、面板），再告訴 AI 要怎麼改。
- **直接說**：用平常說話的方式描述你想改什麼，例如「在右上角加一個深色模式的開關」或「標題字放大一點」。

每次修改後，AgentKnife 會重新編譯 App，需要等一下。編譯完成後，預覽視窗就會顯示結果，這也就是 App 最終的樣子和用起來的感覺。

## 範例

三個範例都是用在同一台電腦上執行的本機模型（qwen3.8-flash）做的，沒有用到雲端 AI。

### 照片編輯器介面

![用 AgentKnife 做的科幻風照片編輯器介面](images/photo-editor.jpg)

AI agent 從這一段簡短的需求做出這個介面：

> Create a futuristic, high-tech, sci-fi inspired "Photo Editor UI Only". This is a purely client-side mock UI with no backend logic.

### 路徑搜尋：A\* 與 Dijkstra

![用 AgentKnife 做的路徑搜尋視覺化，在同一張格子圖上比較 A* 與 Dijkstra](images/path-finding.jpg)

一個看得見執行過程的演算法。AI agent 從這一段簡短的需求做出來：

> Create a pathfinding visualizer: a grid where I can draw walls and drag the start and goal, then watch A\* and Dijkstra explore side by side, step by step, with a speed slider and a count of visited cells.

### 3D 方塊城堡

![用 AgentKnife 做的 3D 方塊城堡，櫻花花瓣緩緩飄落](images/voxel-castle.jpg)

一個有動畫的 3D 場景。AI agent 從這一段簡短的需求做出來：

> Create a 3D voxel castle scene: a small Japanese-style castle built from colorful voxel blocks on a hill, surrounded by cherry blossom trees, with pink petals gently falling and drifting in the wind. I can rotate and zoom the camera with the mouse, and there is a slider for the wind strength and a button to switch between day and night.

第一段需求只是起點。想法有變的時候，隨時都可以請 AI 修改：路徑搜尋範例裡的縮放按鈕，就是後來用「Add zoom-in and zoom-out buttons」加上去的。

## 在 Windows 上安裝

macOS / Linux 版之後推出。

1. 從 [Releases](../../releases/latest) 頁面下載 `AgentKnife-1.0.0-windows-x64.zip`。
2. 解壓縮到你想放的資料夾，`agentknife.exe` 和 `agentknife-mcp.exe` 要放在一起。
3. 執行 `agentknife.exe`。程式沒有簽章，如果 Windows 跳出「Windows 已保護您的電腦」，請按「其他資訊」，再按「仍要執行」。

第一次開啟時，AgentKnife 會請你先連結一個 AI，然後自己準備好需要的工具，其他東西都不用自己動手裝。

### 會下載哪些東西

下載總量約 2–2.5 GB，大部分是 Visual C++ Build Tools。開第一個專案前約需 7–8 GB 硬碟空間，第一個專案建置完約 10.5–11.5 GB。AgentKnife 本身約 35 MB。每個套件和它的大小（英文）：[What gets downloaded](docs/install-size.md)。

## 連結 AI

AgentKnife 本身不附 AI。第一次開啟時，它會請你用下面三種方式之一連結一個 AI。之後隨時可以換，之前的對話會接著用。

### 1. 網路上的 AI 服務（API key）

最聰明的模型。你需要該服務的帳號和 API key，用多少付多少。

- Claude（Anthropic）：推薦，很會寫 App
- OpenAI
- OpenRouter：一把 key 就能用很多家公司的模型
- Google Gemini：用 Google AI Studio 取得 key，有免費額度

Key 存在電腦的系統鑰匙圈裡，不會存成檔案。使用雲端 AI 時會顯示大約用了多少 token 和花費，也可以設定每日上限。

### 2. 在自己電腦上跑的 AI

免費又不外流資料，但需要一台夠快的電腦，以及一個執行模型的程式。

- Ollama
- LM Studio
- 其他跟 OpenAI API 用法相同的程式（llama.cpp、vLLM、Jan 等）：填入它的位址；只有在它需要時才填 key。位址在別台電腦或網路上也可以。

AgentKnife 會檢查模型能不能使用工具，效果可能不好時會提醒你。

### 3. 用你已經在用的 AI 工具（MCP，進階）

如果你已經在用 Claude Code、Codex 或 Antigravity，它們可以透過 MCP 操作 AgentKnife，不需要另外的 key。你在那個工具的視窗裡跟 AI 對話，AgentKnife 會顯示它的每一步和即時預覽。你需要會用終端機。

1. 在連結 AI 的頁面打開「用你已經在用的 AI 工具（進階）」，開啟「允許其他 AI 工具操作 AgentKnife」，再選你的工具。
2. 執行一次它顯示的指令（旁邊有「複製」按鈕），例如：

   ```
   claude mcp add agentknife -- "<AgentKnife 資料夾>\agentknife-mcp.exe"
   codex mcp add agentknife -- "<AgentKnife 資料夾>\agentknife-mcp.exe"
   agy mcp add agentknife "<AgentKnife 資料夾>\agentknife-mcp.exe"
   ```

3. 在專案資料夾啟動那個工具（預設是 `%USERPROFILE%\AgentKnife\Projects`，除非你選了別的位置）。第一次連線時，AgentKnife 會問你要不要允許。

其他支援 MCP 的工具也可以使用同一個 `agentknife-mcp.exe`。關掉那個開關，所有工具就會一起斷線。

## 打包你的 App

App 做好之後，按「打包 App」。在 Windows 上會得到一個 `.zip`：解壓縮到任何地方，執行裡面的程式就好，自己的電腦或別人的電腦都可以。App 沒有簽章，第一次執行時 Windows 可能會跳出「Windows 已保護您的電腦」，請按「其他資訊」，再按「仍要執行」。AI 也會一步步說明。

## 從原始碼建置

需要先裝好 [Rust](https://rustup.rs/)，以及含「使用 C++ 的桌面開發」工作負載的 Visual Studio Build Tools。然後執行：

```
git clone <這個 repository>
cd agentknife
cargo run -p ak-doctor --example doctor_install
cargo xtask build
```

`doctor_install` 會把 AgentKnife 建置時要用的工具裝到 `%APPDATA%\AgentKnife\toolchain`（跟第一次開啟時裝的是同一套）。`cargo xtask build` 會在 `target\release` 產生 `agentknife.exe` 和 `agentknife-mcp.exe`。`cargo xtask dist` 還會把它們打包成 `target\dist` 裡的 release zip。

## 隱私

AgentKnife 不收集任何使用資料。你的專案、對話和 API key 都留在你的電腦上，只有你傳給所選 AI 服務的內容會送出去。

## 授權

你可以任選 [Apache License 2.0](LICENSE-APACHE) 或 [MIT license](LICENSE-MIT) 其中一種授權使用。

AgentKnife 放進你 App 裡的程式碼（`templates/`，以及從 `crates/` 複製過去的 `ak-bridge`、`ak-ui`）改用 [MIT No Attribution](templates/LICENSE) 授權。你做出來的 App 屬於你，可以分享或販售，不必附上任何 AgentKnife 的授權聲明。

## 支持這個專案

如果 AgentKnife 對你有幫助，可以透過以下方式支持它的開發：

* [Buy Me a Bubble Tea](https://madedit.bobaboba.me/)
* [Ko-fi](https://ko-fi.com/madedit)
* [PayPal](https://paypal.me/madedit)
