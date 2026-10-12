// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Follows IME composition on one element through UIA TextEdit events.
//!
//! TextEditPattern.GetActiveComposition() is not used because Chromium keeps
//! returning the last composition string after the IME commits it, and VS
//! Code returns garbage cut out of its placeholder text.
//!
//! Notepad and XAML raise TextEditChangeType_CompositionFinalized on commit;
//! Chromium never does. What Chromium does do is raise a Composition event
//! right before every Text_TextChanged caused by the IME. A TextChanged that
//! arrives without a fresh Composition event in front of it is therefore the
//! commit, provided the user pressed a key after the last composition update.
//! VS Code raises neither on commit; see the keyboard module for that case.

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use windows::Win32::System::Com::SAFEARRAY;
use windows::Win32::System::Ole::{SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound};
use windows::Win32::UI::Accessibility::{
    IUIAutomation, IUIAutomation3, IUIAutomationCacheRequest, IUIAutomationElement,
    IUIAutomationEventHandler, IUIAutomationEventHandler_Impl,
    IUIAutomationTextEditTextChangedEventHandler,
    IUIAutomationTextEditTextChangedEventHandler_Impl, TextEditChangeType,
    TextEditChangeType_Composition, TextEditChangeType_CompositionFinalized, TreeScope_Element,
    TreeScope_Subtree, UIA_EVENT_ID, UIA_Text_TextChangedEventId,
};
use windows::core::{BSTR, Interface, Ref, Result, implement};

use crate::keyboard;

/// Longest gap between a Composition event and the TextChanged event it
/// causes. Chromium raises the pair 1 to 2 ms apart, which leaves room for
/// scheduling jitter.
const PAIR_WINDOW: Duration = Duration::from_millis(50);

/// State shared between a [`Tracker`] and the UIA event handlers that feed it.
#[derive(Debug, Default)]
struct State {
    /// The text being composed, or `None` when the IME is idle.
    composition: Option<String>,
    /// The last composition string, kept after the control reports it
    /// committed, until [`Tracker::mark_committed`] says it was dealt with.
    uncommitted: Option<String>,
    /// When the last Composition event arrived.
    last_update: Option<Instant>,
    /// Whether the element has raised any TextEdit event.
    reports_composition: bool,
    /// Number of Text_TextChanged events seen.
    text_changes: u64,
}

/// Composition state of the element the tracker was attached to.
#[derive(Debug, Clone, Default)]
pub(crate) struct Tracker {
    state: Arc<Mutex<State>>,
}

impl Tracker {
    /// Registers event handlers on `element`. They stay registered until
    /// `IUIAutomation::RemoveAllEventHandlers` is called.
    pub fn attach(uia: &IUIAutomation, element: &IUIAutomationElement) -> Result<Self> {
        let tracker = Self::default();
        let handler = Handler {
            state: tracker.state.clone(),
        };
        let text_edit: IUIAutomationTextEditTextChangedEventHandler = handler.into();
        let changed: IUIAutomationEventHandler = text_edit.cast()?;
        let uia3: IUIAutomation3 = uia.cast()?;
        // SAFETY: `element` and the handlers are live COM interface pointers,
        // and UIA takes its own reference to each handler.
        unsafe {
            for kind in [
                TextEditChangeType_Composition,
                TextEditChangeType_CompositionFinalized,
            ] {
                // XAML text boxes raise TextEdit events with themselves as the
                // sender, yet a TreeScope_Element registration never receives
                // them. Subtree scope includes the element and does.
                uia3.AddTextEditTextChangedEventHandler(
                    element,
                    TreeScope_Subtree,
                    kind,
                    None::<&IUIAutomationCacheRequest>,
                    &text_edit,
                )?;
            }
            uia.AddAutomationEventHandler(
                UIA_Text_TextChangedEventId,
                element,
                TreeScope_Element,
                None::<&IUIAutomationCacheRequest>,
                &changed,
            )?;
        }
        Ok(tracker)
    }

    /// The text being composed, or `None` when the IME is idle.
    pub fn composition(&self) -> Option<String> {
        self.state.lock().composition.clone()
    }

    /// The last composition string, whether or not it has been committed
    /// since. Blind mode corrects it once it has seen the commit itself.
    pub fn uncommitted(&self) -> Option<String> {
        self.state.lock().uncommitted.clone()
    }

    /// Whether the element has raised any TextEdit event. Some controls,
    /// such as the Chrome and Edge address bars, never do, so their
    /// composition state is unknown.
    pub fn reports_composition(&self) -> bool {
        self.state.lock().reports_composition
    }

    /// How many times the element has reported that its text changed. Zero
    /// can also mean the element never reports it.
    pub fn text_changes(&self) -> u64 {
        self.state.lock().text_changes
    }

    /// When the last composition update arrived.
    pub fn last_update(&self) -> Option<Instant> {
        self.state.lock().last_update
    }

    /// Forgets the pending composition once the caller has decided, from a
    /// signal outside UIA, that it was committed.
    pub fn mark_committed(&self) {
        let mut state = self.state.lock();
        state.composition = None;
        state.uncommitted = None;
    }
}

/// UIA event handler that writes what it sees into a [`Tracker`]'s state.
///
/// UIA calls the handlers from its own threads, so the state sits behind a
/// mutex.
#[implement(
    IUIAutomationTextEditTextChangedEventHandler,
    IUIAutomationEventHandler
)]
struct Handler {
    state: Arc<Mutex<State>>,
}

impl IUIAutomationTextEditTextChangedEventHandler_Impl for Handler_Impl {
    fn HandleTextEditTextChangedEvent(
        &self,
        _sender: Ref<'_, IUIAutomationElement>,
        kind: TextEditChangeType,
        strings: *const SAFEARRAY,
    ) -> Result<()> {
        let mut state = self.state.lock();
        tracing::trace!(kind = kind.0, null = strings.is_null(), "text edit event");
        state.reports_composition = true;
        if kind == TextEditChangeType_Composition {
            // An empty composition string means the user deleted every pending
            // key, which leaves nothing to wait for.
            let text = first_string(strings).unwrap_or_default();
            state.composition = (!text.is_empty()).then_some(text);
            state.uncommitted = state.composition.clone();
            state.last_update = Some(Instant::now());
        } else {
            state.composition = None;
        }
        Ok(())
    }
}

impl IUIAutomationEventHandler_Impl for Handler_Impl {
    fn HandleAutomationEvent(
        &self,
        _sender: Ref<'_, IUIAutomationElement>,
        _event: UIA_EVENT_ID,
    ) -> Result<()> {
        let mut state = self.state.lock();
        tracing::trace!("text changed event");
        state.text_changes += 1;
        let Some(last_update) = state.last_update else {
            return Ok(());
        };
        let paired = last_update.elapsed() <= PAIR_WINDOW;
        // Programs also change text on their own while a composition is open;
        // the File Explorer search box does so to run its live search. Every
        // key typed into a composition produces a fresh Composition event, so
        // only a key pressed after the last update, such as Enter, can be the
        // commit. Treating a programmatic change as the commit would correct
        // text that is still being composed and break the composition.
        let key_since_update = keyboard::last_key().is_some_and(|k| k > last_update);
        if state.composition.is_some() && !paired && key_since_update {
            state.composition = None;
        }
        Ok(())
    }
}

/// Reads the first BSTR of a one-dimensional SAFEARRAY.
fn first_string(array: *const SAFEARRAY) -> Option<String> {
    if array.is_null() {
        return None;
    }
    // SAFETY: UIA passes a valid one-dimensional SAFEARRAY of BSTR that stays
    // alive for the duration of the callback.
    unsafe {
        let lower = SafeArrayGetLBound(array, 1).ok()?;
        let upper = SafeArrayGetUBound(array, 1).ok()?;
        if upper < lower {
            return None;
        }
        let mut value = BSTR::default();
        SafeArrayGetElement(array, &lower, &mut value as *mut BSTR as *mut _).ok()?;
        Some(value.to_string())
    }
}
