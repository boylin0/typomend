# Copyright boylin0 and the Typomend contributors.
# SPDX-License-Identifier: MPL-2.0
#
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

### Traditional Chinese as used in Taiwan. Keep the messages in the order and
### groups of the English file.

## Tray

tray-tooltip = Typomend
tray-tooltip-paused = Typomend（已暫停）
tray-tooltip-private = Typomend（不記錄模式）
menu-open = 開啟 Typomend
menu-pause = 暫停修正
menu-resume = 繼續修正
menu-private = 不記錄模式
menu-autostart = 開機時啟動
menu-quit = 結束

## Window

dialog-close = 關閉
already-running = Typomend 已經在執行中，圖示在系統匣。按「確定」會開啟它的設定視窗。

## Tabs

tab-overview = 總覽
tab-rules = 規則
tab-model = 遮罩語言模型
tab-usage = 用量
tab-apps = 應用程式
tab-history = 紀錄
tab-settings = 設定
tab-about = 關於

## Overview

overview-enabled = 自動修正錯字
overview-status-on = Typomend 正在留意你輸入的文字。
overview-status-off = 修正已暫停。
overview-focus = 目前的程式
overview-focus-none = 還沒有選取任何文字欄位。
overview-count = 啟動以來的修正次數：{ $count }

## Support levels

support-readable = 支援
support-blind = 支援（盲改模式）
support-terminal = 支援（終端機模式）
support-unsupported = 不支援
support-excluded-password = 略過：密碼欄位
support-excluded-disabled = 已由你關閉
support-excluded-terminal = 略過：標準模式無法修正終端機
support-excluded-elevated = 略過：以系統管理員身分執行
support-excluded-blind-off = 略過：盲改模式已關閉

## Rules

rules-engine = 修正引擎
rules-engine-rules = 規則清單
rules-engine-mlm = 規則清單＋遮罩語言模型
rules-file = 規則檔：{ $path }
rules-status = 已在 { $time } 載入 { $count } 條規則。存檔後會立即套用。
rules-status-missing = 找不到規則檔。按「編輯規則」會建立一個。
rules-status-error = 已略過第 { $line } 行：{ $reason }。共有 { $count } 行有問題。
rules-error-separator = 錯字和修正文字之間沒有「=」
rules-error-empty = 錯字或修正文字是空的
rules-edit = 編輯規則
rules-open-folder = 開啟資料夾
rules-from = 錯字
rules-to = 修正為
rules-file-header-format = Typomend 規則清單：一行一條，格式為「錯字 = 修正文字」。
rules-file-header-apply = 以 # 開頭的行會被忽略。存檔後 Typomend 會立即套用。

## Masked language model

lm-intro = 用本機的遮罩語言模型依上下文找出選錯的同音字。模型不隨 Typomend 附帶，請在這裡下載。
lm-model = 模型
lm-mode = 模式
lm-mode-accurate = 精準：逐字檢查新輸入的字
lm-mode-balanced = 平衡：先快速篩選，再確認
lm-mode-fast = 快速：只跑一次快速檢查
lm-not-installed = - 尚未下載
lm-download = 下載
lm-cancel = 取消下載
lm-delete = 刪除模型
lm-dir = 存放位置：{ $path }
lm-change-dir = 變更位置...
lm-open-folder = 開啟資料夾
lm-note = 模型下載自 Hugging Face，各有其授權條款。變更位置不會搬移已下載的模型。
lm-status-off = 已下載。到「規則」分頁選擇「規則清單＋遮罩語言模型」即可啟用。
lm-status-loading = 正在載入模型...
lm-status-ready = 模型已就緒。規則清單仍會優先套用。
lm-status-missing = 尚未下載（約 { $size } MB）。按「下載」取得。
lm-status-failed = 無法載入模型：{ $error }
lm-status-connecting = 正在連線下載 { $model }...
lm-status-downloading = 正在下載 { $model }：{ $done } / { $total } MB（{ $percent }%）
    速度 { $speed } MB/s，{ $threads } 條連線
lm-status-download-failed = 下載失敗：{ $error }
    再按「下載」會從中斷處繼續。
lm-confirm-delete = 要刪除 { $model } 嗎？之後要使用需重新下載。
lm-delete-failed = 無法刪除模型：{ $error }
lm-try = 測試…
lm-bench = 準確度評測…
lm-bench-stop = 停止評測
lm-status-benchmarking = 正在評測 { $model }（{ $set }）：{ $done } / { $total } 組
lm-expected = 預期效果（{ $mode }，加入模型時實測）：
lm-measured-here = 這台電腦的實測結果（{ $mode }）：
lm-expected-none = 這個模型還沒有評測數據。下載後按「準確度評測…」可以自己測。
lm-model-summary = 繁 { $tw }% · 簡 { $cn }% · 混 { $mixed }%
lm-mode-name-accurate = 精準模式
lm-mode-name-balanced = 平衡模式
lm-mode-name-fast = 快速模式
lm-bench-result-line = { $set }：修正 { $fixed }% 錯字，誤改 { $fp }% 正確句，每次檢查 { $ms } ms
lm-bench-failed = 無法評測：{ $error }
lm-set-zh-tw = 繁體中文
lm-set-zh-cn = 簡體中文
lm-set-zh-tw-mixed = 中英混打

## Benchmark

bench-title = 準確度評測
bench-intro = 用目前選擇的模型和模式，模擬在內建的三組測試句上打字，算出修正了多少錯字、又改錯了多少正確的句子。評測會用上所有處理器核心，精準模式在 16 核心的電腦上約需 25 秒。
bench-model = 模型：{ $model }，{ $mode }
bench-start = 開始評測
bench-hint = 按「開始評測」。結果也會顯示在「遮罩語言模型」分頁的預期效果。
bench-not-installed = 這個模型還沒下載，下載後才能評測。
bench-done = 評測完成。結果也已顯示在「遮罩語言模型」分頁。
bench-set = 測試集
bench-fixed = 修正錯字
bench-changed = 誤改正確句
bench-ms = 每次檢查
bench-reference = 參考值（加入模型時）

## Try

try-title = 測試 Typomend
try-intro = 輸入一段文字，看 Typomend 會怎麼修正，以及遮罩語言模型對每個字的評分。在這個視窗打字時不會自動修正。
try-run = 檢查
try-hint = 輸入文字後按 Enter 或「檢查」。
try-running = 檢查中…
try-failed = 無法檢查：{ $error }
try-committed = 一次送出：{ $text }
try-typed = 逐字輸入：{ $text }
try-unchanged = { $text }（不修正）
try-with-fixes = { $text }（{ $fixes }）
try-explained = 耗時 { $ms } ms。比值是同音字比原字可能幾倍，要達到「門檻」才會修正。打字時只檢查最後 8 個字。
try-no-model = 耗時 { $ms } ms。沒有載入可以評分的遮罩語言模型，只套用規則。
try-char = 字
try-best = 最可能
try-ratio = 比值
try-required = 門檻
try-verdict = 結果
try-passes = 達到
try-fails = 未達
try-passes-ended = 達到：視為句末時 { $ratio }
try-fails-ended = 未達：視為句末時 { $ratio }

## Apps

apps-intro = Typomend 啟動後你輸入過文字的程式，以及你設定過模式的程式。選取一個程式即可變更修正方式。
apps-empty = 在任何程式裡打字，它就會出現在這裡。
apps-select-hint = 選取一個程式來變更它的修正方式。
apps-not-seen = 這次啟動後還沒用過
apps-pending = 下次使用這個程式時套用
apps-name = 程式
apps-mode = 模式
apps-mode-label = 修正模式：
apps-support = 狀態
mode-auto = 自動
mode-standard = 標準
mode-blind = 盲改
mode-terminal = 終端機
mode-off = 關閉
mode-auto-hint = 依文字欄位提供的資訊自動選擇做法。終端機會以終端機模式修正。
mode-standard-hint = 只修正讀得到內文的欄位：選取錯字、取代，再讀回確認。終端機會被略過。
mode-blind-hint = 從游標位置用方向鍵修正，不讀取內文，因此無法驗證結果。適用於會隱藏內文的編輯器。
mode-terminal-hint = 用於命令列：讀取游標前的文字，用方向鍵和 Backspace 逐步修正並讀回確認。任何一步沒有生效就會停止，不再送出後續按鍵。
mode-off-hint = 不修正這個程式。

## History

history-intro = 最近的修正。紀錄只存在記憶體中，結束 Typomend 後就會清除。
history-empty = 還沒有任何修正。
history-private = 不記錄模式已開啟，不會保留修正紀錄。
history-clear = 清除
history-time = 時間
history-app = 程式
history-change = 修正內容
history-result = 結果
outcome-verified = 完成
outcome-unverified = 已送出，無法驗證
outcome-failed = 失敗
outcome-skipped = 略過，你還在打字
outcome-too-slow = 略過，這個欄位回應太慢

## Usage

usage-enabled = 以圖表顯示資源用量
usage-hint = 每秒取樣一次，只在這個視窗開啟時進行。關閉後完全不取樣。
usage-model-none = 目前沒有載入遮罩語言模型。
usage-model-loading = 正在載入遮罩語言模型...
usage-model-memory = { $model } 載入時佔用約 { $size } MB 記憶體，在 Typomend 內以 CPU 執行。
usage-model-memory-unknown = { $model } 在 Typomend 內以 CPU 執行。
usage-gpu-memory = GPU 專用記憶體：Typomend { $typomend } MB
usage-gpu-none = 這台電腦沒有提供 GPU 用量資訊。
usage-cpu-title = CPU（刻度 { $max }%）
usage-memory-title = 記憶體（刻度 { $max } MB）
usage-gpu-title = GPU（刻度 { $max }%）

## Settings

settings-language = 語言
settings-language-system = 跟隨系統
settings-autostart = 開機時啟動
settings-idle = 最後一次按鍵後的等待時間（毫秒）
settings-idle-hint = 鍵盤閒置這麼久之後才會送出修正，避免跟你正在打的字混在一起。
settings-caret-window = 游標前的搜尋範圍（字元）
settings-blind = 允許盲改模式
settings-blind-hint = 用於會隱藏內文的編輯器，例如關閉協助工具支援的 VS Code。修正會用方向鍵送出，無法驗證結果。
settings-private = 不記錄模式
settings-private-hint = 不寫入記錄檔，也不保留修正紀錄。
settings-delete-logs = 刪除記錄檔
settings-logs-deleted = 已刪除 { $count } 個記錄檔。
settings-logs-delete-failed = 無法刪除記錄檔：{ $error }
settings-open-logs = 開啟記錄檔資料夾

## About

about-tagline = 照常輸入，原地修正。
about-description = 在任何程式裡，於輸入法確認送出文字後修正錯字，不需要更換輸入法。
about-version = 版本 { $version }
about-license = 授權：{ $license }
about-source = 原始碼
