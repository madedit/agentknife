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
doctor-ready = 需要的工具都裝好了。

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
