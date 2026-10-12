# Changelog

All notable changes to this project are documented in this file. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Releases ship as a zip archive named for the version and the platform it runs on, `typomend-<version>-x86_64-pc-windows-msvc.zip` for 64-bit Windows on an Intel or AMD processor, with the license and the readme next to `typomend.exe`. The SHA-256 checksum is now of the archive, in `typomend-<version>-x86_64-pc-windows-msvc.zip.sha256`.

## [0.0.1] - 2026-10-11

First public release.

### Added

- Tray application that fixes typos after the input method commits them, in any Windows app, without replacing the input method. It reads the focused text field through UI Automation and picks how to correct it from what the field exposes, not from a list of apps. The release build is a single executable of about 5 MB that needs no administrator rights.
- Rule engine: `rules.txt` is a plain text file you edit with any editor, applied as soon as it is saved. Rules are matched with an Aho-Corasick automaton, so a list of 200,000 rules takes effect about 0.25 s after saving.
- Masked language model engine for homophone typos no rule lists, such as 因問 for 因為 or 打籃求 for 打籃球. A local BERT masked language model fills in a hidden character from its context, so Typomend asks it how likely each homophone of a recently typed character is and replaces one only when it is many times likelier than what was typed. The model only scores candidates and never writes text. Candidates stay in the script of the text, so 為 is never replaced with 为. The default model, macbert4csc, fixed 81% of Traditional, 83% of Simplified and 70% of mixed Chinese and English typos in a typing simulation, while changing at most 2% of correct sentences.
- The last two characters typed are checked with a ten times stricter threshold until more text follows. One that falls short is checked again as if the text ended there, so 臭豆府 at the end of 今天要去吃臭豆府 is fixed without waiting for more text.
- Text committed at once with up to three typos, such as 臭斗府 for 臭豆腐, has every typo corrected.
- Masked LM tab: choose a model and a mode, accurate, balanced or fast, and see each model's measured fix rate per language before you download it. Downloads come from Hugging Face over 4 connections, resume after a failure or cancel, and go to `D:\Typomend\models`, `C:\Typomend\models` on a machine without a D: drive, or any folder you choose.
- Accuracy benchmark window: types the bundled Traditional Chinese, Simplified Chinese and mixed Chinese and English test sets through the chosen model and mode on all processor cores, and shows the typos fixed, the correct text changed and the time per check. Accurate mode takes about 25 seconds on a 16-core computer. Your results replace the published ones on the Masked LM tab. Results per model and language are in `crates/typomend-scorer/README.md`.
- Try window: shows what Typomend makes of some text, committed at once and typed a character at a time, with each character's likeliest homophone, how many times likelier it is than what was typed and the ratio it must reach.
- Correction mode per app on the Apps tab: automatic, standard, blind, terminal or off.
- Terminal mode corrects command lines in Windows Terminal and the classic console, and is what automatic mode uses for terminals. The typo is replaced with arrow keys and Backspace, and every step is read back before the next one is sent.
- Blind mode for editors that hide their text but report IME composition, such as VS Code with accessibility support off, and for controls that report the end of a composition themselves, such as Notepad.
- Long documents cost no more than short ones: edits are learned from UI Automation events, and only the text just before the caret is read. With 100,000 characters in an Edge text area, Edge stays under 1% of a CPU core while idle and a correction takes about 0.8 s.
- A correction is dropped when you start typing again before it is written, and in fields that answer too slowly to change safely. The history shows why.
- Keys that make a correction go out 30 ms apart, because Windows 11 Notepad handles a burst of them out of order.
- A field Chrome or Edge has not exposed yet when it gets focus is checked again for a few seconds, so the first field typed in after the browser starts is corrected too.
- Password fields and elevated apps are always skipped.
- Usage tab: charts of the CPU, memory and GPU Typomend used over the last two minutes, and the memory the masked language model took when it loaded. Sampling is off by default and runs once a second, only while the settings window is open.
- Private mode, from the tray menu or the Settings tab: no log files are written and no correction history is kept. Settings can also delete existing log files.
- English and Traditional Chinese user interface, following the system language by default. Typomend still corrects Simplified Chinese text.
- Start with Windows and a single running instance. Starting Typomend again opens the running instance's settings window.
- Settings in `%APPDATA%\typomend\config.toml`. Thresholds follow the recommendation for the chosen model and mode unless `custom_threshold` is set, and `log` chooses which log lines are written, such as `typomend_observer=debug,info`; `TYPOMEND_LOG` overrides it.
- Program icon, a monochrome tray icon that follows the taskbar's light or dark theme and dims while paused, and version information shown on the About tab, in Task Manager and in File Explorer.
- Each release publishes a SHA-256 checksum, `typomend.exe.sha256`, next to `typomend.exe`.
- Licensed under MPL-2.0, with the license notice in every source file and a link to the license on the About tab.

[Unreleased]: https://github.com/boylin0/typomend/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/boylin0/typomend/releases/tag/v0.0.1
