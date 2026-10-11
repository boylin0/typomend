// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The observer thread: follows the focus and corrects committed text.
//!
//! Every [`POLL`] interval the thread looks up the focused element. A newly
//! focused element is probed once to decide its [`Support`] and gets a
//! [`Tracker`] that follows its IME composition. On later ticks the handler
//! for that support level decides when the user has finished an edit and
//! asks the corrector about the text just before the caret.
//!
//! Nothing is overwritten unverified. The range about to be replaced must
//! hold exactly the typo the corrector saw, the replacement is read back
//! where the control allows it, and a correction the user overtakes by
//! typing again is dropped instead of racing the user for the selection.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::Accessibility::{
    IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern, IUIAutomationTextRange,
};

use crate::event::{CorrectionRecord, Event, ExcludeReason, Outcome, Support};
use crate::settings::{AppMode, Settings, Shared};
use crate::terminal::{self, Line};
use crate::tracker::Tracker;
use crate::uia::{self, Focused};
use crate::{ime, input, keyboard, process};

/// Time between two ticks of the observer thread.
const POLL: Duration = Duration::from_millis(150);

/// Longest time from the last composition update to Enter for blind mode to
/// take Enter as the commit. A later Enter is an ordinary newline: the
/// composition was probably committed some other way, such as a mouse click,
/// and the caret may have moved since.
const BLIND_COMMIT_WINDOW: Duration = Duration::from_secs(5);

/// How many times [`wait_until`] checks its condition. Chromium updates its
/// accessibility tree asynchronously, so a selection or a replacement can
/// take a few frames to show through UIA.
const SETTLE_ATTEMPTS: usize = 10;

/// Time between two checks of [`wait_until`].
const SETTLE_INTERVAL: Duration = Duration::from_millis(50);

/// Calls `check` until it returns `true`, at most [`SETTLE_ATTEMPTS`] times
/// and [`SETTLE_INTERVAL`] apart. Returns whether it did.
pub(crate) fn wait_until(mut check: impl FnMut() -> bool) -> bool {
    for attempt in 0..SETTLE_ATTEMPTS {
        if check() {
            return true;
        }
        if attempt + 1 < SETTLE_ATTEMPTS {
            std::thread::sleep(SETTLE_INTERVAL);
        }
    }
    false
}

/// Window classes of terminals. Selecting text in a terminal starts a mark
/// mode instead of moving the input cursor, so terminals are corrected with
/// keys relative to the cursor, as in [`AppMode::Terminal`].
const TERMINAL_CLASSES: &[&str] = &["TermControl", "ConsoleWindowClass", "PseudoConsoleWindow"];

/// Executable names of terminals, lower case and without extension; see
/// [`TERMINAL_CLASSES`].
const TERMINAL_PROCESSES: &[&str] = &["windowsterminal", "conhost", "openconsole"];

/// Classes of controls that may hide their text from UIA. VS Code's editor
/// with accessibility support off exposes a fixed hint as both its name and
/// its text instead of the document.
const HIDDEN_TEXT_CLASSES: &[&str] = &["native-edit-context"];

/// Characters read before the caret beyond the caret window, so a typo that
/// ends inside the window but starts before it is still seen whole.
const LOOKBACK: usize = 64;

/// How many more times a control that looked unusable is probed. Chromium
/// builds its accessibility tree only once a UIA client asks for it, so the
/// field that has focus at that moment may expose no text yet.
const REPROBES: u8 = 4;

/// Time between two probes of the same control.
const REPROBE_INTERVAL: Duration = Duration::from_millis(750);

/// Longest a field may take to answer one text call and still be corrected.
///
/// Between selecting a typo and replacing it, any key the user presses
/// overwrites the selection, and that window spans a few calls. Chromium
/// takes time in proportion to the length of the document for every text
/// call, about 50 ms per call at 300,000 characters.
const SLOW_FIELD: Duration = Duration::from_millis(50);

/// How long after a correction has been read back its own text-changed
/// events may still arrive. Chromium raises them asynchronously.
const OWN_EDIT_SETTLE: Duration = Duration::from_millis(500);

/// The most corrections made to the text of one edit.
///
/// A commit can hold more than one typo, as 臭斗府 does for 臭豆腐, while a
/// [`Corrector`](typomend_engine::Corrector) returns one at a time, so the
/// text is checked again after each correction. The limit stops a corrector
/// that keeps changing its mind.
pub const MAX_FIXES_PER_EDIT: u8 = 3;

/// A running observer.
///
/// The observer works on a thread of its own; see [`Observer::spawn`].
/// Dropping the handle stops the thread and waits for it to finish.
#[derive(Debug)]
pub struct Observer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Observer {
    /// Starts watching the desktop on a new thread.
    ///
    /// Settings and the corrector are read from `shared` on every tick, so
    /// changes to them take effect while the observer runs. `sink` receives
    /// every [`Event`] on the observer thread and must not block.
    ///
    /// # Panics
    ///
    /// Panics if the operating system cannot create the thread.
    pub fn spawn(shared: Arc<Shared>, sink: impl Fn(Event) + Send + 'static) -> Self {
        keyboard::start();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("typomend-observer".into())
                .spawn(move || run(&shared, &stop, &sink))
                .expect("spawning the observer thread")
        };
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Runs the observer thread, one tick every [`POLL`], until `stop` is set.
fn run(shared: &Shared, stop: &AtomicBool, sink: &dyn Fn(Event)) {
    // SAFETY: this is a new thread that has not initialized COM yet, and a
    // successful call is balanced by the CoUninitialize at the end.
    if let Err(e) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
        tracing::error!("CoInitializeEx failed: {e}");
        return;
    }
    match uia::create() {
        Ok(uia) => {
            let mut state = State::new(uia);
            while !stop.load(Ordering::Acquire) {
                std::thread::sleep(POLL);
                if let Err(e) = state.tick(shared, sink) {
                    // The focused element routinely disappears between calls,
                    // for example when a tab is closed. Start over next tick.
                    tracing::trace!("tick failed: {e}");
                    state.reset();
                }
            }
            uia::remove_all_handlers(&state.uia);
        }
        Err(e) => tracing::error!("creating the UI Automation client failed: {e}"),
    }
    // SAFETY: balances the successful CoInitializeEx above, and every COM
    // object this thread created was dropped with `state`.
    unsafe { CoUninitialize() };
}

/// What the observer knows about the focused element.
struct Watch {
    element: IUIAutomationElement,
    /// Executable name of the process that owns the element.
    process: String,
    /// Native window of the element, which is invalid when it has none of its
    /// own.
    hwnd: HWND,
    /// How the control is handled.
    support: Support,
    /// The element's TextPattern, if it has one.
    pattern: Option<IUIAutomationTextPattern>,
    /// Follows the IME composition; `None` for controls that are not
    /// corrected or when attaching it failed.
    tracker: Option<Tracker>,
    /// Text-changed events seen so far, for controls that raise them.
    seen_changes: u64,
    /// Changes before this moment are the observer's own edit.
    own_edit_until: Option<Instant>,
    /// The whole document as last read, for controls that never raise
    /// text-changed events. Read only after a key press, so a large document
    /// costs nothing while nobody types.
    last_text: Option<String>,
    /// When [`text_changed`] last read the document.
    read_at: Instant,
    /// The text changed and has not been checked for typos since.
    pending: bool,
    /// Corrections made since the user last changed the text.
    fixes: u8,
    /// What blind mode left before the caret after its last correction.
    blind_next: Option<BlindNext>,
    /// The composition as last logged.
    last_composition: Option<String>,
    /// The command line, for [`Support::Terminal`].
    line: Option<Line>,
    /// How many more times [`State::reprobe`] may look at the control.
    probes_left: u8,
    /// Earliest moment [`State::reprobe`] may look at the control again.
    next_probe: Instant,
}

/// The state of the observer thread from one tick to the next.
struct State {
    uia: IUIAutomation,
    /// The focused element, or `None` until the next tick after a reset.
    watch: Option<Watch>,
    /// The settings generation the watch was made for. A newer one makes
    /// the observer look at the focused control afresh.
    generation: u64,
    /// Process names by process id, looked up once per process.
    names: HashMap<u32, String>,
    /// This process, whose own windows are never corrected.
    self_pid: u32,
    /// Whether this process runs elevated. Windows blocks input sent from a
    /// process to one running at a higher integrity level.
    self_elevated: bool,
}

impl State {
    fn new(uia: IUIAutomation) -> Self {
        Self {
            uia,
            watch: None,
            generation: u64::MAX,
            names: HashMap::new(),
            self_pid: std::process::id(),
            self_elevated: process::self_elevated(),
        }
    }

    /// Forgets the focused element and the event handlers registered on it.
    fn reset(&mut self) {
        if self.watch.take().is_some_and(|w| w.tracker.is_some()) {
            uia::remove_all_handlers(&self.uia);
        }
    }

    /// Runs one step: starts watching a newly focused element, or moves the
    /// correction of the focused one forward.
    fn tick(&mut self, shared: &Shared, sink: &dyn Fn(Event)) -> windows::core::Result<()> {
        let settings = shared.settings();
        let generation = shared.generation();
        if !settings.enabled {
            self.reset();
            return Ok(());
        }
        if generation != self.generation {
            self.generation = generation;
            self.reset();
        }

        let focused = Focused::get(&self.uia)?;
        if focused.pid == self.self_pid {
            self.reset();
            return Ok(());
        }
        let same = self
            .watch
            .as_ref()
            .is_some_and(|w| uia::same_element(&self.uia, &w.element, &focused.element));
        if !same {
            self.reset();
            let watch = self.watch_new(focused, &settings);
            sink(Event::Focus {
                process: watch.process.clone(),
                support: watch.support,
            });
            tracing::debug!(process = %watch.process, support = ?watch.support, "focus");
            self.watch = Some(watch);
            return Ok(());
        }

        if self.reprobe(&focused, &settings) {
            // Start over with a fresh watch next tick.
            self.reset();
            return Ok(());
        }

        let corrector = shared.corrector();
        let Some(watch) = self.watch.as_mut() else {
            return Ok(());
        };
        let tracker = watch.tracker.clone();
        let composition = tracker.as_ref().and_then(Tracker::composition);
        if composition != watch.last_composition {
            match &composition {
                Some(text) => tracing::trace!("composing: {text}"),
                None => tracing::trace!("composition ended"),
            }
            watch.last_composition = composition.clone();
        }

        let record = match (watch.support, &tracker) {
            // Terminals draw the composition over the buffer, but a control
            // put in terminal mode may show it inline as Notepad does, and
            // keys sent into it would go to the IME.
            (Support::Terminal, _) if composition.is_some() => None,
            (Support::Terminal, _) => terminal_tick(watch, &settings, corrector.as_ref()),
            (Support::Readable, Some(tracker)) => {
                readable_tick(watch, tracker, &settings, corrector.as_ref())?
            }
            (Support::Blind, Some(tracker)) => blind_tick(watch, tracker, corrector.as_ref()),
            _ => None,
        };
        if let Some(record) = record {
            tracing::info!(process = %record.process, outcome = ?record.outcome, "correction");
            sink(Event::Correction(record));
        }
        Ok(())
    }

    /// Starts watching a newly focused element.
    fn watch_new(&mut self, focused: Focused, settings: &Settings) -> Watch {
        let process = self
            .names
            .entry(focused.pid)
            .or_insert_with(|| process::name(focused.pid).unwrap_or_default())
            .clone();
        let (pattern, support) = self.probe(&focused, &process, settings);

        let tracker = match support {
            Support::Readable | Support::Blind | Support::Terminal => {
                match Tracker::attach(&self.uia, &focused.element) {
                    Ok(t) => Some(t),
                    Err(e) => {
                        tracing::debug!("attaching the tracker failed: {e}");
                        None
                    }
                }
            }
            _ => None,
        };
        let line = match (support, &pattern) {
            (Support::Terminal, Some(pattern)) => Some(Line::read(pattern)),
            _ => None,
        };
        Watch {
            element: focused.element,
            process,
            hwnd: focused.hwnd,
            support,
            pattern,
            tracker,
            seen_changes: 0,
            own_edit_until: None,
            last_text: None,
            read_at: Instant::now(),
            pending: false,
            fixes: 0,
            blind_next: None,
            last_composition: None,
            line,
            probes_left: REPROBES,
            next_probe: Instant::now() + REPROBE_INTERVAL,
        }
    }

    /// Finds out how the focused control can be handled. Only as much of its
    /// text is read as the decision needs, however long the document is.
    fn probe(
        &self,
        focused: &Focused,
        process: &str,
        settings: &Settings,
    ) -> (Option<IUIAutomationTextPattern>, Support) {
        let pattern = focused.text_pattern();
        // One character more than the name tells a document that merely
        // repeats the name apart from one that starts with it.
        let max = uia::utf16_len(&focused.name) + 1;
        let text = pattern
            .as_ref()
            .and_then(|p| uia::document_prefix(p, max).ok());
        let support = self.classify(focused, process, text.as_deref(), settings);
        (pattern, support)
    }

    /// Looks again at a control that seemed unusable. Returns whether it can
    /// now be handled differently.
    fn reprobe(&mut self, focused: &Focused, settings: &Settings) -> bool {
        let Some(watch) = self.watch.as_mut() else {
            return false;
        };
        let unusable = matches!(watch.support, Support::Blind | Support::Unsupported);
        if !unusable || watch.probes_left == 0 || Instant::now() < watch.next_probe {
            return false;
        }
        watch.probes_left -= 1;
        watch.next_probe = Instant::now() + REPROBE_INTERVAL;
        let (before, process) = (watch.support, watch.process.clone());
        let (_, support) = self.probe(focused, &process, settings);
        if support != before {
            tracing::debug!(process = %process, from = ?before, to = ?support, "reprobe");
        }
        support != before
    }

    /// Decides how the focused control is handled, from the mode chosen for
    /// its program and from what it exposes. `text` is the start of its
    /// document, or `None` when it has no readable text.
    fn classify(
        &self,
        focused: &Focused,
        process: &str,
        text: Option<&str>,
        settings: &Settings,
    ) -> Support {
        let lower = process.to_ascii_lowercase();
        let mode = settings.app_mode(&lower);
        if focused.is_password() {
            return Support::Excluded(ExcludeReason::Password);
        }
        if mode == AppMode::Off {
            return Support::Excluded(ExcludeReason::DisabledByUser);
        }
        let terminal = TERMINAL_CLASSES.contains(&focused.class.as_str())
            || TERMINAL_PROCESSES.contains(&lower.as_str());
        if terminal && mode == AppMode::Standard {
            return Support::Excluded(ExcludeReason::Terminal);
        }
        if !self.self_elevated && process::is_elevated(focused.pid) {
            return Support::Excluded(ExcludeReason::Elevated);
        }

        let hidden = HIDDEN_TEXT_CLASSES.contains(&focused.class.as_str())
            && text == Some(focused.name.as_str());
        let readable = text.is_some() && !hidden;
        let blind = focused.has_text_edit_pattern();
        match mode {
            AppMode::Terminal if readable => Support::Terminal,
            AppMode::Auto if terminal && readable => Support::Terminal,
            AppMode::Blind if blind => Support::Blind,
            AppMode::Auto | AppMode::Standard if readable => Support::Readable,
            AppMode::Auto if blind => {
                if settings.blind_mode {
                    Support::Blind
                } else {
                    Support::Excluded(ExcludeReason::BlindModeOff)
                }
            }
            _ => Support::Unsupported,
        }
    }
}

/// Moves the correction of a control with readable text forward. Returns
/// the correction attempted on this tick, if any.
fn readable_tick(
    watch: &mut Watch,
    tracker: &Tracker,
    settings: &Settings,
    corrector: &dyn typomend_engine::Corrector,
) -> windows::core::Result<Option<CorrectionRecord>> {
    let Some(pattern) = watch.pattern.clone() else {
        return Ok(None);
    };
    if text_changed(watch, tracker, &pattern)? {
        watch.pending = true;
        watch.fixes = 0;
        // Wait for one quiet interval so a burst of committed characters is
        // treated as a single edit.
        return Ok(None);
    }
    // Nothing to check, or the user is still composing.
    if !watch.pending || tracker.composition().is_some() {
        return Ok(None);
    }
    // Typed replacements interleave with keys the user presses at the same
    // time. Wait until the keyboard has been quiet for a moment.
    if input::idle_ms() < settings.idle_ms {
        return Ok(None);
    }
    // Without TextEdit events the composition cannot be seen, and an open
    // Bopomofo composition is shown inline as ordinary text. Correcting it
    // would break the composition, and the Enter meant to commit it would then
    // submit the field instead. While the IME is in native mode, only Enter as
    // the most recent key shows that the composition has been committed.
    if !tracker.reports_composition() && ime::native_mode() {
        let last_key_was_enter =
            keyboard::last_key().is_some_and(|k| Some(k) == keyboard::last_enter());
        if !last_key_was_enter {
            return Ok(None);
        }
    }
    watch.pending = false;
    let record = correct_readable(watch, &pattern, settings, corrector)?;
    if let Some(record) = &record {
        watch.own_edit_until = Some(Instant::now() + OWN_EDIT_SETTLE);
        if watch.last_text.is_some() {
            watch.last_text = Some(uia::document_text(&pattern)?);
        }
        watch.fixes += 1;
        watch.pending = record.outcome == Outcome::Verified && watch.fixes < MAX_FIXES_PER_EDIT;
    }
    Ok(record)
}

/// Returns whether the document changed since the last call.
///
/// Reading the document to find out costs time in proportion to its length,
/// in this process and in the target, which a browser feels badly. Controls
/// that raise text-changed events are therefore never read for it. The rest
/// are read only after a key press, so an idle document costs nothing.
fn text_changed(
    watch: &mut Watch,
    tracker: &Tracker,
    pattern: &IUIAutomationTextPattern,
) -> windows::core::Result<bool> {
    let changes = tracker.text_changes();
    if changes > 0 {
        let changed = changes != watch.seen_changes;
        watch.seen_changes = changes;
        let own = watch.own_edit_until.is_some_and(|t| Instant::now() < t);
        return Ok(changed && !own);
    }
    if !keyboard::last_key().is_some_and(|k| k > watch.read_at) {
        return Ok(false);
    }
    watch.read_at = Instant::now();
    let current = uia::document_text(pattern)?;
    // The first read only sets the baseline. Treating it as a change would
    // correct a typo the user merely moved the caret next to.
    let changed = watch.last_text.as_ref().is_some_and(|t| *t != current);
    watch.last_text = Some(current);
    Ok(changed)
}

/// Corrects the latest typo before the caret of a readable control by
/// selecting it, writing the replacement and reading it back.
///
/// Returns `None` when there is nothing to correct or the control does not
/// hold the text the corrector saw, and a record of the attempt otherwise.
fn correct_readable(
    watch: &Watch,
    pattern: &IUIAutomationTextPattern,
    settings: &Settings,
    corrector: &dyn typomend_engine::Corrector,
) -> windows::core::Result<Option<CorrectionRecord>> {
    // Each UIA call can take hundreds of milliseconds in a long browser
    // document, so the user may start typing again while this runs. A key
    // pressed then would land on the selected typo, so a correction that has
    // been overtaken is dropped before anything is selected or written.
    let started = Instant::now();
    let overtaken = || keyboard::last_key().is_some_and(|k| k > started);
    let Some(selection) = uia::first_selection(pattern)? else {
        return Ok(None);
    };
    let round_trip = started.elapsed();
    // Only the text just before the caret is read, and every range below is
    // built relative to the caret, so a correction costs the same at the end
    // of a long document as in a short one.
    let caret = uia::caret_of(&selection)?;
    let head = uia::text_before_caret(&caret, settings.caret_window + LOOKBACK)?;
    let Some(fix) = corrector.last_correction(&head, settings.caret_window) else {
        return Ok(None);
    };
    // The text between the end of the typo and the caret, which the
    // correction must leave alone, in UTF-16 units.
    let after = uia::utf16_len(&head[fix.end()..]);
    let from_len = uia::utf16_len(&fix.from);
    let to_len = uia::utf16_len(&fix.to);
    if after > settings.caret_window {
        return Ok(None);
    }
    let record = |outcome| CorrectionRecord {
        process: watch.process.clone(),
        from: fix.from.clone(),
        to: fix.to.clone(),
        outcome,
    };
    if round_trip > SLOW_FIELD {
        tracing::debug!(
            ms = round_trip.as_millis() as u64,
            "field too slow to correct"
        );
        return Ok(Some(record(Outcome::TooSlow)));
    }

    // Right after a commit the caret is collapsed, unless the control has
    // appended an inline autocompletion and selected it, as the Chrome and
    // Edge address bars do from browsing history. That suggestion is not
    // something the user typed. Moving the selection to the typo would accept
    // it, so reject it with Delete first, as the user would. A selection that
    // stops short of the end was made some other way and is left alone.
    if !uia::text(&selection)?.is_empty() {
        if !uia::ends_at_document_end(pattern, &selection)? {
            return Ok(None);
        }
        if !input::tap_key(input::KEY_DELETE, 1)
            || !wait_until(|| uia::selection_text(pattern).is_ok_and(|t| t.is_empty()))
        {
            return Ok(Some(record(Outcome::Failed)));
        }
    }

    let target = uia::range_before(&caret, after, from_len)?;
    // Never overwrite text that was not verified, and never type into a
    // control whose UIA selection is not its real editing selection.
    if uia::text(&target)? != fix.from {
        tracing::debug!("range text mismatch {after} before the caret");
        return Ok(None);
    }
    if overtaken() {
        return Ok(Some(record(Outcome::Skipped)));
    }
    uia::select(&target)?;
    if !wait_until(|| uia::selection_text(pattern).is_ok_and(|t| t == fix.from)) {
        tracing::debug!("selection did not follow TextRange.Select()");
        return Ok(Some(record(Outcome::Failed)));
    }
    if overtaken() {
        // Put the caret back where it was before the typo was selected.
        uia::select(&uia::caret_after(
            &uia::caret_of(&target)?,
            from_len + after,
        )?)?;
        return Ok(Some(record(Outcome::Skipped)));
    }

    if !write_replacement(watch, &fix.to) {
        return Ok(Some(record(Outcome::Failed)));
    }

    // Writing leaves the caret right after the replacement.
    let mut written = None;
    let verified = wait_until(|| {
        written = replaced_before_caret(pattern, &fix.to, to_len)
            .ok()
            .flatten();
        written.is_some()
    });

    // Put the caret back where the user left it, past the text that followed
    // the typo. When the typo ended at the caret, typing already left the
    // caret there; moving it again would accept any new inline autocompletion
    // the control has selected after it.
    if let Some(caret) = written
        && after > 0
    {
        uia::select(&uia::caret_after(&caret, after)?)?;
    }

    Ok(Some(record(if verified {
        Outcome::Verified
    } else {
        Outcome::Failed
    })))
}

/// Returns the caret if `to`, `to_len` UTF-16 units long, sits right before
/// it.
fn replaced_before_caret(
    pattern: &IUIAutomationTextPattern,
    to: &str,
    to_len: usize,
) -> windows::core::Result<Option<IUIAutomationTextRange>> {
    let Some(selection) = uia::first_selection(pattern)? else {
        return Ok(None);
    };
    let caret = uia::caret_of(&selection)?;
    let replaced = uia::text(&uia::range_before(&caret, 0, to_len)?)? == to;
    Ok(replaced.then_some(caret))
}

/// Replaces the selected typo with `to`. Returns whether the text was sent.
///
/// Edit and RichEdit controls take the text directly, which never passes
/// through the IME. Everything else gets typed input, which keeps the
/// change undoable in the target control.
fn write_replacement(watch: &Watch, to: &str) -> bool {
    let class = input::class_name(watch.hwnd);
    if !watch.hwnd.is_invalid() && input::supports_replace_selection(&class) {
        return input::replace_selection(watch.hwnd, to);
    }
    input::type_text(to)
}

/// Moves the correction of a command line forward; see [`terminal::tick`].
fn terminal_tick(
    watch: &mut Watch,
    settings: &Settings,
    corrector: &dyn typomend_engine::Corrector,
) -> Option<CorrectionRecord> {
    let (Some(line), Some(pattern)) = (watch.line.as_mut(), watch.pattern.as_ref()) else {
        return None;
    };
    terminal::tick(line, pattern, &watch.process, settings, corrector)
}

/// Moves the correction of a control in blind mode forward. Returns the
/// correction attempted on this tick, if any.
///
/// The committed text is the last composition string and the caret sits
/// right after it, so the typo is reached with arrow keys relative to the
/// caret instead of through TextPattern. Controls such as Notepad report
/// the commit themselves, which ends the composition, so the string is taken
/// from what the tracker kept of it.
fn blind_tick(
    watch: &mut Watch,
    tracker: &Tracker,
    corrector: &dyn typomend_engine::Corrector,
) -> Option<CorrectionRecord> {
    // A key pressed since the last correction may have moved the caret, so
    // the text it left is only worth checking again while nothing was typed.
    let (composition, since, fixes) = match watch.blind_next.take() {
        Some(next) if keyboard::last_key().is_none_or(|k| k < next.at) => {
            (next.text, next.at, next.fixes)
        }
        _ => {
            let (composition, enter) = blind_commit(tracker)?;
            (composition, enter, 0)
        }
    };

    let fix = corrector.last_correction(&composition, composition.chars().count())?;
    let mut record = CorrectionRecord {
        process: watch.process.clone(),
        from: fix.from.clone(),
        to: fix.to.clone(),
        outcome: Outcome::Skipped,
    };
    // The keys below count from where the commit left the caret, which a key
    // pressed since may have moved.
    if keyboard::last_key().is_some_and(|k| k > since) {
        return Some(record);
    }

    let after = uia::utf16_len(&composition[fix.end()..]);
    let ok = input::tap_key(input::KEY_LEFT, after)
        && input::tap_key(input::KEY_BACK, uia::utf16_len(&fix.from))
        && input::type_text(&fix.to)
        && input::tap_key(input::KEY_RIGHT, after);
    if ok && fixes + 1 < MAX_FIXES_PER_EDIT {
        watch.blind_next = Some(BlindNext {
            text: format!(
                "{}{}{}",
                &composition[..fix.start],
                fix.to,
                &composition[fix.end()..]
            ),
            at: Instant::now(),
            fixes: fixes + 1,
        });
    }
    record.outcome = if ok {
        Outcome::Unverified
    } else {
        Outcome::Failed
    };
    Some(record)
}

/// The text before the caret after a blind correction, and when it was
/// made.
struct BlindNext {
    text: String,
    at: Instant,
    /// Corrections made to the commit so far.
    fixes: u8,
}

/// Returns the composition just committed with Enter and when Enter was
/// pressed, once the IME has had time to show that Enter committed it.
fn blind_commit(tracker: &Tracker) -> Option<(String, Instant)> {
    let composition = tracker.uncommitted()?;
    let enter = keyboard::last_enter()?;
    let last_update = tracker.last_update()?;
    if enter <= last_update {
        return None;
    }
    // Give the IME time to raise another Composition event, which would mean
    // Enter did not commit, for example because it picked a candidate.
    if enter.elapsed() < Duration::from_millis(300) {
        return None;
    }
    // Whether or not this Enter counts as the commit, the composition is
    // over, so it is forgotten rather than looked at again on every tick.
    tracker.mark_committed();
    if enter - last_update > BLIND_COMMIT_WINDOW {
        return None;
    }
    Some((composition, enter))
}
