<h1 align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset=".github/assets/Typomend-lockup-reverse.svg">
    <img alt="Typomend" src=".github/assets/Typomend-lockup-color.svg" width="360">
  </picture>
</h1>

<p align="center"><strong>Keep typing. Mend in place.</strong></p>
<p align="center"><a href="https://typomend.github.io/">typomend.github.io</a></p>

**English** | [繁體中文](README.zh-TW.md)

Typomend is an autocorrect layer that sits after your input method. It notices the wrong homophone the IME just committed and fixes it in place, in any app, on your own computer.

You keep Microsoft Bopomofo, or whichever IME you already use. There is no new input method to learn, no key to press and nothing sent to the cloud: you type 我今天去打籃求, and a moment later it reads 我今天去打籃球.

> **Status:** early side project. Traditional Chinese comes first, Simplified Chinese works too, and the masked language model is optional.

## Why Typomend?

Every Chinese IME picks the wrong homophone now and then: 在 for 再, 的 for 得, 因該 for 應該. The usual fixes all ask you to change how you work:

| Approach | What it costs you |
|----------|-------------------|
| Switch to a smarter IME | New habits, a new dictionary, and an install that a work computer may not allow |
| Proofread afterwards | Copy, paste into a checker, read the suggestions, paste back |
| Press a key to check before sending | You have to remember, every time, and the text often goes to a cloud API |
| **Typomend** | **Nothing. Keep typing; the typo is gone before you look back** |

Typomend works on the text after the IME commits it, so it does not care which IME produced it, and the IME keeps doing what it already does well.

## Highlights

- **Keeps your IME.** Typomend never talks to the input method. It reads the focused text field through UI Automation, the accessibility API that screen readers use, and corrects what was committed.
- **Fully automatic.** No hotkey, no pop-up to confirm. Text you are still composing is never touched.
- **On-device, private by design.** Rules and the masked language model run locally. Logs never contain what you type, and private mode keeps no logs or history at all.
- **Two engines, layered.** A rule list for the typos you know about, plus an optional local masked language model for the ones you do not. A masked language model fills in a hidden character from its context, so Typomend asks it how likely each homophone is; it only scores candidates and never writes text. The default model fixed 81% of Traditional Chinese homophone typos in a typing simulation without changing a single correct sentence.
- **Measured, not promised.** The **Masked LM** tab benchmarks the model on your own machine and shows its fix rate, the share of correct text it changed and the time per check.
- **Works where you type, including terminals.** Browsers, Notepad, File Explorer, VS Code, Windows Terminal, PowerShell, and AI coding agents such as Claude Code running in them. Each app can have its own correction mode.
- **No administrator rights.** One executable that runs as your user and starts with Windows from your own account.

## Quick reference

| I want to | Do this |
|-----------|---------|
| Run it | Download `typomend.exe` from the [latest release](../../releases/latest) and start it |
| Open the settings | Left-click the tray icon |
| Pause corrections | Right-click the tray icon, then **Pause corrections** |
| Start with Windows | Tray menu or **Settings** tab, **Start with Windows** |
| Fix typos no rule lists | **Rules** tab, choose **Rule list and masked language model**, then download a model on the **Masked LM** tab |
| Add my own corrections | **Rules** tab, **Edit rules**, then save the file |
| See how well a model does on my computer | **Masked LM** tab, **Accuracy benchmark…** |
| Stop it for one app | **Apps** tab, select the app, then set **Correction mode** to **Off** |
| Quit | Tray menu, **Quit**. Closing the window only hides it |

## How does it work?

Typomend watches the focused text field through [UI Automation](https://learn.microsoft.com/windows/win32/winauto/entry-uiauto-win32):

1. UIA TextEdit events report every keystroke of an IME composition, so Typomend knows when you are still composing and leaves that text alone.
2. Once the composition is committed and the keyboard has been idle briefly, Typomend checks the text just before the caret.
3. It selects the typo and replaces it, then reads the text back to confirm.

Only the text just before the caret is read, and edits are learned about from UI Automation events, so a long document costs no more than a short one. Browsers answer UI Automation more slowly the longer a text field is; a field too slow to change safely, around 300,000 characters in Chrome or Edge, is left alone. A correction is also dropped when you start typing again before it is written.

How a field is handled depends on what it exposes, not on which app it belongs to:

| Field | Examples | How the typo is replaced | Verified |
|-------|----------|--------------------------|----------|
| Text is readable | Notepad, Chrome, Edge, File Explorer search | Select through UIA, then `EM_REPLACESEL` or simulated typing | Yes |
| Text is hidden, composition is reported | VS Code with accessibility support off | Arrow keys relative to the caret ("blind mode") | No |
| Command line | Windows Terminal, the classic console, and the shells or apps running in them | Arrow keys and Backspace from the cursor, one step at a time ("terminal mode") | Yes |
| Password fields, elevated apps | | Left alone | |

## Correction engines

The **Rules** tab chooses the engine:

| Engine | Fixes | Cost |
|--------|-------|------|
| Rule list | Exactly the typos listed in `rules.txt` | Nothing noticeable, even with 200,000 rules |
| Rule list and masked language model | The listed typos, plus homophone typos no rule lists, such as 因問 for 因為 or 打籃求 for 打籃球 | A model download of a few hundred MB and tens of milliseconds per check on the CPU |

### How does the masked language model decide?

The model is a judge, not a writer. For each recently typed character, Typomend lists its homophones, asks a BERT model how likely each one is in context, and replaces the character only when one homophone is many times likelier than what was typed. **Every correction is therefore a homophone or near-homophone of what you typed**, never a rewrite, and one character changes at a time. Candidates stay in the script of the text, so 為 is never replaced with 为.

The model needs context after a typo to judge it well, so the last two characters typed are checked with a ten times stricter threshold until more text follows.

Results for the default model, macbert4csc in accurate mode, typing the bundled test sets two characters at a time:

| Test set | Typos fixed | Correct sentences changed |
|----------|------------:|--------------------------:|
| Traditional Chinese | 81% | 0% |
| Simplified Chinese | 83% | 0% |
| Chinese mixed with English and numbers | 70% | 2% |

The **Masked LM** tab offers other BERT models and three modes from about 25 ms to about 53 ms per check. Every model, mode and test set is compared in [`crates/typomend-scorer/README.md`](crates/typomend-scorer/README.md).

## Correction mode per app

The **Apps** tab lists the apps you have typed in. Select one to choose how it is corrected:

| Mode | What it does |
|------|--------------|
| Automatic | The default: the method follows from what the field exposes, as in the table above; terminals get terminal mode |
| Standard | Only fields whose text is readable; the typo is selected, replaced and read back. Terminals are skipped |
| Blind | Arrow keys from the caret, without reading the text, even where it is readable |
| Terminal | For command lines: the text before the cursor is read, the typo is replaced with arrow keys and Backspace, and the line is read back. Useful for other apps that only work this way |
| Off | Never correct this app |

Selecting text in a terminal starts a mark mode instead of moving the shell's cursor, which is why terminals are corrected with keys instead. This works wherever the line is edited with arrow keys and Backspace: PowerShell, cmd and Claude Code, in Windows Terminal and the classic console. Every step is read back before the next is sent, and the cursor is checked before anything is deleted, so in an app where arrow keys mean something else the correction stops without touching the text. Password fields and elevated apps are skipped whatever the mode.

## Rules

The first run creates `rules.txt` with a built-in list of 273 common typos. Rules live in a plain UTF-8 text file, so you can edit them, or paste in thousands at once, with Notepad, VS Code or any other editor. **Typomend applies the file the moment you save it**; no restart is needed.

```text
# Lines starting with # are ignored; comment a rule out to switch it off.
因該 = 應該
在見 = 再見
```

- One rule per line, `typo = correction`. Spaces around `=` are ignored.
- When the same typo appears twice, the later line wins.
- Lines Typomend cannot read are skipped, and the **Rules** tab shows the first one by line number.

All rules are compiled into a single [Aho-Corasick](https://en.wikipedia.org/wiki/Aho%E2%80%93Corasick_algorithm) automaton, so lookups take the same time however long the list is. With 200,000 rules, a saved file is in effect about a quarter of a second later.

Because the file is plain text, a team can keep one list of its product names, people's names and jargon, and everyone's computer corrects them the same way.

## Download

Download `typomend.exe` from the [latest release](../../releases/latest) and run it. Each release also publishes `typomend.exe.sha256` so you can verify the download.

Run with `--background` to start hidden in the notification area; this is how the **Start with Windows** entry launches it.

## Configuration and privacy

| What | Where |
|------|-------|
| Settings | `%APPDATA%\typomend\config.toml` |
| Rules | `%APPDATA%\typomend\rules.txt` |
| Masked language models | `D:\Typomend\models`, or `C:\Typomend\models` without a D: drive, or any folder you choose |
| Logs | `%LOCALAPPDATA%\typomend\logs` |
| Correction history | Memory only, gone when Typomend quits |

**Logs never contain what you type.** They record app names, support levels and outcomes only. Text is checked on your computer; the only network traffic is a model download you start yourself.

Turn on **Private mode** from the tray menu or the **Settings** tab to stop writing log files and stop keeping correction history; **Delete log files** in **Settings** removes the ones already written.

## Project layout

| Path | Contents |
|------|----------|
| `crates/typomend` | The tray application: window, settings, i18n |
| `crates/typomend-engine` | Rule engine, free of platform code |
| `crates/typomend-scorer` | Masked language model engine, model downloads and benchmarks |
| `crates/typomend-observer` | UI Automation observer that detects committed text and writes corrections |
| `xtask/` | Project automation, run with `cargo xtask <task>` |
| `tools/` | End-to-end test scripts, a UI Automation probe and the script that regenerates the scorer's data files |

## Contributing

Issues and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under the [Mozilla Public License 2.0](LICENSE).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project shall be licensed under the same terms.
