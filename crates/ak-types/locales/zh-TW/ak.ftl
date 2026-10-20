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
