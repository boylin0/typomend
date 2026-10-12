# Copyright boylin0 and the Typomend contributors.
# SPDX-License-Identifier: MPL-2.0
#
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

### English is the source language. A message missing from a translation
### falls back to the one here.

## Tray

tray-tooltip = Typomend
tray-tooltip-paused = Typomend (paused)
tray-tooltip-private = Typomend (private mode)
menu-open = Open Typomend
menu-pause = Pause corrections
menu-resume = Resume corrections
menu-private = Private mode
menu-autostart = Start with Windows
menu-quit = Quit

## Window

dialog-close = Close
# Shown in a message box when Typomend is started while it is already running.
already-running = Typomend is already running. Its icon is in the notification area; choose OK to open its settings.

## Tabs

tab-overview = Overview
tab-rules = Rules
tab-model = Masked LM
tab-usage = Usage
tab-apps = Apps
tab-history = History
tab-settings = Settings
tab-about = About

## Overview

overview-enabled = Correct typos automatically
overview-status-on = Typomend is watching what you type.
overview-status-off = Corrections are paused.
overview-focus = Current app
overview-focus-none = No text field has been focused yet.
overview-count = Corrections since start: { $count }

## Support levels

support-readable = Supported
support-blind = Supported (blind mode)
support-terminal = Supported (terminal mode)
support-unsupported = Not supported
support-excluded-password = Skipped: password field
support-excluded-disabled = Turned off by you
support-excluded-terminal = Skipped: standard mode cannot correct a terminal
support-excluded-elevated = Skipped: runs as administrator
support-excluded-blind-off = Skipped: blind mode is off

## Rules

rules-engine = Correction engine
rules-engine-rules = Rule list
rules-engine-mlm = Rule list and masked language model
rules-file = Rule file: { $path }
# Variables:
#   $count (Number) - number of rules read from the file
#   $time (String) - time of day the file was read, as HH:MM:SS
rules-status = { $count } rules loaded at { $time }. Saving the file applies it right away.
rules-status-missing = The rule file does not exist. Edit rules creates it.
# Variables:
#   $line (Number) - line number of the first line that was skipped
#   $reason (String) - why it was skipped: rules-error-separator or rules-error-empty
#   $count (Number) - number of lines skipped
rules-status-error = Line { $line } was skipped: { $reason }. { $count } lines have problems.
rules-error-separator = no "=" between the typo and the correction
rules-error-empty = the typo or the correction is empty
rules-edit = Edit rules
rules-open-folder = Open folder
rules-from = Typo
rules-to = Correction
# The two header lines are written as comment lines at the top of a new rule
# file, so each must stay on a single line.
rules-file-header-format = Typomend rules: one per line, written as "typo = correction".
rules-file-header-apply = Lines starting with # are ignored. Typomend applies the file as soon as it is saved.

## Masked language model

lm-intro = Uses a local masked language model to find homophones picked by mistake, judged by context. Models do not ship with Typomend; download one here.
lm-model = Model
lm-mode = Mode
lm-mode-accurate = Accurate: checks every new character
lm-mode-balanced = Balanced: quick pass, then confirms
lm-mode-fast = Fast: one quick pass only
# Appended to the name of a model in the model list.
lm-not-installed = - not downloaded
lm-download = Download
lm-cancel = Cancel download
lm-delete = Delete model
lm-dir = Stored in: { $path }
lm-change-dir = Change location...
lm-open-folder = Open folder
lm-note = Models come from Hugging Face under their own licenses. Changing the location does not move models already downloaded.
lm-status-off = Downloaded. Choose "Rule list and masked language model" on the Rules page to use it.
lm-status-loading = Loading the model...
lm-status-ready = Model ready. Rules still take precedence.
lm-status-missing = Not downloaded yet (about { $size } MB). Press Download to get it.
lm-status-failed = The model could not be loaded: { $error }
lm-status-connecting = Connecting to download { $model }...
# Variables:
#   $model (String) - id of the model, such as macbert4csc-base-chinese
#   $done (String) - megabytes downloaded so far
#   $total (String) - size of the download in megabytes
#   $percent (String) - share of the download done, from 0 to 100
#   $speed (String) - download speed in megabytes per second
#   $threads (String) - number of connections in use
lm-status-downloading = Downloading { $model }: { $done } / { $total } MB ({ $percent }%)
    { $speed } MB/s over { $threads } connections
lm-status-download-failed = Download failed: { $error }
    Press Download to resume where it stopped.
lm-confirm-delete = Delete { $model }? It has to be downloaded again to be used.
lm-delete-failed = The model could not be deleted: { $error }
lm-try = Try...
lm-bench = Accuracy benchmark...
lm-bench-stop = Stop benchmark
# Variables:
#   $model (String) - id of the model being benchmarked
#   $set (String) - name of the test set being typed, one of the lm-set-* messages
#   $done (String) - sentence pairs typed so far
#   $total (String) - sentence pairs in the test set
lm-status-benchmarking = Benchmarking { $model } on { $set }: { $done } / { $total } pairs
# Followed by one lm-bench-result-line per test set.
# Variables:
#   $mode (String) - name of the mode, one of the lm-mode-name-* messages
lm-expected = Expected in { $mode }, measured when the model was added:
# Followed by one lm-bench-result-line per test set.
# Variables:
#   $mode (String) - name of the mode, one of the lm-mode-name-* messages
lm-measured-here = Measured on this computer in { $mode }:
lm-expected-none = No measurements for this model yet. Download it and press Accuracy benchmark to measure it.
# Shown after each model in the model list.
# Variables:
#   $tw (String) - percent of Traditional Chinese typos the model fixed
#   $cn (String) - percent of Simplified Chinese typos the model fixed
#   $mixed (String) - percent of typos the model fixed in Chinese mixed with English
lm-model-summary = Trad { $tw }% · Simp { $cn }% · Mixed { $mixed }%
# Mode names, used inside lm-expected, lm-measured-here and bench-model.
lm-mode-name-accurate = accurate mode
lm-mode-name-balanced = balanced mode
lm-mode-name-fast = fast mode
# Variables:
#   $set (String) - name of the test set, one of the lm-set-* messages
#   $fixed (String) - percent of typos fixed
#   $fp (String) - percent of correct sentences changed by mistake
#   $ms (String) - average milliseconds per check
lm-bench-result-line = { $set }: { $fixed }% of typos fixed, { $fp }% of correct text changed, { $ms } ms per check
lm-bench-failed = The benchmark could not run: { $error }
# Names of the bundled test sets.
lm-set-zh-tw = Traditional Chinese
lm-set-zh-cn = Simplified Chinese
lm-set-zh-tw-mixed = Chinese with English

## Benchmark

bench-title = Accuracy benchmark
bench-intro = Types the three bundled test sets through the chosen model and mode, and counts the typos fixed and the correct sentences changed. It uses every processor core; in accurate mode it takes about 25 seconds on a 16-core computer.
# Variables:
#   $model (String) - id of the model
#   $mode (String) - name of the mode, one of the lm-mode-name-* messages
bench-model = Model: { $model }, { $mode }
bench-start = Start
bench-hint = Press Start. The results also replace the expected figures on the Masked LM page.
bench-not-installed = Download this model before benchmarking it.
bench-done = Done. The results are also shown on the Masked LM page.
bench-set = Test set
bench-fixed = Typos fixed
bench-changed = Correct text changed
bench-ms = Per check
bench-reference = Reference (when added)

## Try

try-title = Try Typomend
try-intro = Type some text to see how Typomend would correct it and how the masked language model scores each character. Text typed in this window is not corrected automatically.
try-run = Check
try-hint = Type some text, then press Enter or Check.
try-running = Checking...
try-failed = Could not check: { $error }
try-committed = Committed at once: { $text }
try-typed = Typed one character at a time: { $text }
try-unchanged = { $text } (left as typed)
# Variables:
#   $text (String) - the text after the corrections
#   $fixes (String) - the corrections made, each as "typo → correction",
#       separated by slashes
try-with-fixes = { $text } ({ $fixes })
try-explained = Took { $ms } ms. Ratio: how many times likelier the homophone is than the typed character; it must reach the required ratio. Only the last 8 characters typed are checked while typing.
try-no-model = Took { $ms } ms. No masked language model that scores characters is loaded, so only the rules apply.
try-char = Character
try-best = Likeliest
try-ratio = Ratio
try-required = Required
try-verdict = Result
try-passes = Above
try-fails = Below
# Results for one of the last characters, which is also scored as if the text
# ended right after it.
# Variables:
#   $ratio (String) - the ratio the character reaches when the text ends there
try-passes-ended = Above: { $ratio } as the end of the text
# Variables:
#   $ratio (String) - the ratio the character reaches when the text ends there
try-fails-ended = Below: { $ratio } as the end of the text

## Apps

apps-intro = Apps you have typed in since Typomend started, and apps you have set a mode for. Select one to change how it is corrected.
apps-empty = Type in any app and it will show up here.
apps-select-hint = Select an app to change how it is corrected.
apps-not-seen = Not used since Typomend started
apps-pending = Applies the next time you use this app
apps-name = App
apps-mode = Mode
apps-mode-label = Correction mode:
apps-support = Status
mode-auto = Automatic
mode-standard = Standard
mode-blind = Blind
mode-terminal = Terminal
mode-off = Off
mode-auto-hint = Typomend picks a method from what the text field exposes. Terminals are corrected as in terminal mode.
mode-standard-hint = Only fields whose text Typomend can read: the typo is selected, replaced and read back. Terminals are skipped.
mode-blind-hint = The typo is fixed with arrow keys from the caret, without reading the text, so the result cannot be checked. For editors that hide their text.
mode-terminal-hint = For command lines: the text before the cursor is read, fixed with arrow keys and Backspace one step at a time, and read back. If a step does not land, the rest is not sent.
mode-off-hint = Typomend never corrects this app.

## History

history-intro = Recent corrections. History is kept in memory only and is lost when Typomend quits.
history-empty = No corrections yet.
history-private = Private mode is on, so no corrections are recorded.
history-clear = Clear
history-time = Time
history-app = App
history-change = Change
history-result = Result
outcome-verified = Done
outcome-unverified = Sent, cannot be verified
outcome-failed = Failed
outcome-skipped = Skipped, you kept typing
outcome-too-slow = Skipped, this field responds too slowly

## Usage

usage-enabled = Chart resource usage
usage-hint = Samples once a second, only while this window is open. Turn it off and nothing is sampled.
usage-model-none = No masked language model is loaded.
usage-model-loading = Loading the masked language model...
# Variables:
#   $model (String) - id of the loaded model
#   $size (String) - megabytes of memory the model took when it loaded
usage-model-memory = { $model } took about { $size } MB of memory when it loaded and runs on the CPU, inside Typomend.
usage-model-memory-unknown = { $model } runs on the CPU, inside Typomend.
# Variables:
#   $typomend (String) - megabytes of dedicated GPU memory Typomend uses
usage-gpu-memory = Dedicated GPU memory: Typomend { $typomend } MB
usage-gpu-none = This computer does not report GPU usage.
# Chart titles.
# Variables:
#   $max (String) - the value at the top of the chart's scale
usage-cpu-title = CPU (scale { $max }%)
usage-memory-title = Memory (scale { $max } MB)
usage-gpu-title = GPU (scale { $max }%)

## Settings

settings-language = Language
settings-language-system = System default
settings-autostart = Start with Windows
settings-idle = Wait after the last key press (ms)
settings-idle-hint = Corrections are typed only after the keyboard has been idle this long, so they never mix with your own keys.
settings-caret-window = Search range before the caret (characters)
settings-blind = Allow blind mode
settings-blind-hint = For editors that hide their text, such as VS Code with accessibility support off. Corrections are typed with arrow keys and cannot be verified.
settings-private = Private mode
settings-private-hint = Writes no log files and keeps no correction history.
settings-delete-logs = Delete log files
settings-logs-deleted = Deleted { $count } log files.
settings-logs-delete-failed = Could not delete the log files: { $error }
settings-open-logs = Open log folder

## About

about-tagline = Keep typing. Mend in place.
about-description = Fixes typos after your input method commits them, in any app, without replacing your input method.
about-version = Version { $version }
# Variables:
#   $license (String) - the license name, already marked up as a link
about-license = License: { $license }
# Shown as a link to the source repository.
about-source = Source code
