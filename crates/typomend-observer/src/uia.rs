// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Thin helpers over the UI Automation client API.
//!
//! Each function wraps a few raw UIA calls so the observer does not repeat the
//! `unsafe` blocks. Ranges are built relative to the caret rather than the
//! document, so the cost of a call does not grow with the length of the text.
//! Offsets are counted in UTF-16 code units, which is what UIA uses; see
//! [`utf16_len`].

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
    IUIAutomationTextRange, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start,
    TextUnit_Character, TextUnit_Line, UIA_TextEditPatternId, UIA_TextPatternId,
};
use windows::core::{IUnknown, Result};

/// Creates the UI Automation client.
///
/// COM must already be initialized on the calling thread.
pub(crate) fn create() -> Result<IUIAutomation> {
    // SAFETY: the caller has initialized COM on this thread, and the class id
    // and context are valid constants.
    unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }
}

/// Cached facts about the focused element.
pub(crate) struct Focused {
    /// The focused element.
    pub element: IUIAutomationElement,
    /// Id of the process that owns the element.
    pub pid: u32,
    /// Native window of the element, which is invalid when it has none of its
    /// own.
    pub hwnd: HWND,
    /// UIA class name of the element.
    pub class: String,
    /// UIA name of the element.
    pub name: String,
}

impl Focused {
    /// Reads the focused element and its properties.
    pub fn get(uia: &IUIAutomation) -> Result<Self> {
        // SAFETY: `element` is the live interface returned by UIA, and the
        // calls on it only read properties.
        unsafe {
            let element = uia.GetFocusedElement()?;
            Ok(Self {
                pid: element.CurrentProcessId()? as u32,
                hwnd: HWND(element.CurrentNativeWindowHandle()?.0),
                class: element.CurrentClassName()?.to_string(),
                name: element.CurrentName()?.to_string(),
                element,
            })
        }
    }

    /// Returns whether the element is a password field.
    ///
    /// A failed read counts as not a password field.
    pub fn is_password(&self) -> bool {
        // SAFETY: `self.element` is a live interface and the call only reads a
        // property.
        unsafe { self.element.CurrentIsPassword() }.is_ok_and(|b| b.as_bool())
    }

    /// Returns the element's TextPattern, or `None` when it has none.
    pub fn text_pattern(&self) -> Option<IUIAutomationTextPattern> {
        // SAFETY: `self.element` is a live interface, and an unsupported
        // pattern comes back as an error, which maps to `None`.
        unsafe {
            self.element
                .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        }
        .ok()
    }

    /// Returns whether the element supports TextEditPattern, which raises
    /// composition events even when TextPattern gives no readable text.
    pub fn has_text_edit_pattern(&self) -> bool {
        // SAFETY: `self.element` is a live interface, and an unsupported
        // pattern comes back as an error.
        unsafe {
            self.element
                .GetCurrentPatternAs::<IUnknown>(UIA_TextEditPatternId)
        }
        .is_ok()
    }
}

/// Returns whether `a` and `b` are the same UI element.
pub(crate) fn same_element(
    uia: &IUIAutomation,
    a: &IUIAutomationElement,
    b: &IUIAutomationElement,
) -> bool {
    // SAFETY: `a` and `b` are live interface pointers.
    unsafe { uia.CompareElements(a, b) }.is_ok_and(|b| b.as_bool())
}

/// Returns the whole text of `range`.
pub(crate) fn text(range: &IUIAutomationTextRange) -> Result<String> {
    // SAFETY: `range` is a live interface, and a maximum length of -1 asks for
    // the whole range.
    Ok(unsafe { range.GetText(-1) }?.to_string())
}

/// Reads the whole document. Its cost grows with the document, in this
/// process and in the target, so it is only used where nothing cheaper works.
pub(crate) fn document_text(pattern: &IUIAutomationTextPattern) -> Result<String> {
    // SAFETY: `pattern` is a live interface and the call only reads.
    text(&unsafe { pattern.DocumentRange() }?)
}

/// Reads at most `max` characters from the start of the document.
pub(crate) fn document_prefix(pattern: &IUIAutomationTextPattern, max: usize) -> Result<String> {
    let max = i32::try_from(max).unwrap_or(i32::MAX);
    // SAFETY: `pattern` is a live interface and `max` is a valid length limit.
    Ok(unsafe { pattern.DocumentRange()?.GetText(max) }?.to_string())
}

/// Returns the first selection, which is a degenerate range at the caret when
/// nothing is selected.
pub(crate) fn first_selection(
    pattern: &IUIAutomationTextPattern,
) -> Result<Option<IUIAutomationTextRange>> {
    // SAFETY: `pattern` is a live interface, and the element index is checked
    // against the length first.
    unsafe {
        let selections = pattern.GetSelection()?;
        if selections.Length()? == 0 {
            return Ok(None);
        }
        Ok(Some(selections.GetElement(0)?))
    }
}

/// Returns whether `range` ends where the document ends.
pub(crate) fn ends_at_document_end(
    pattern: &IUIAutomationTextPattern,
    range: &IUIAutomationTextRange,
) -> Result<bool> {
    // SAFETY: `pattern` and `range` are live interfaces and the calls only
    // read.
    unsafe {
        let doc = pattern.DocumentRange()?;
        let order = range.CompareEndpoints(
            TextPatternRangeEndpoint_End,
            &doc,
            TextPatternRangeEndpoint_End,
        )?;
        Ok(order >= 0)
    }
}

/// Returns an empty range at the start of `range`, which is the caret when
/// `range` is the selection.
pub(crate) fn caret_of(range: &IUIAutomationTextRange) -> Result<IUIAutomationTextRange> {
    // SAFETY: `range` is a live interface, and the endpoint is moved on a
    // clone so `range` itself is unchanged.
    unsafe {
        let caret = range.Clone()?;
        caret.MoveEndpointByRange(
            TextPatternRangeEndpoint_End,
            range,
            TextPatternRangeEndpoint_Start,
        )?;
        Ok(caret)
    }
}

/// Returns up to `max` characters before `caret`. Fewer come back near the
/// start of the document.
pub(crate) fn text_before_caret(caret: &IUIAutomationTextRange, max: usize) -> Result<String> {
    // SAFETY: `caret` is a live interface, and the endpoint is moved on a
    // clone so `caret` itself is unchanged.
    unsafe {
        let head = caret.Clone()?;
        head.MoveEndpointByUnit(
            TextPatternRangeEndpoint_Start,
            TextUnit_Character,
            -i32::try_from(max).unwrap_or(i32::MAX),
        )?;
        text(&head)
    }
}

/// Returns the range of `len` characters that ends `gap` characters before
/// `caret`.
///
/// Every range is built relative to the caret, so its cost does not depend
/// on how far into the document the caret is. UIA character units match
/// UTF-16 code units for the text this is used on; callers verify the range
/// text before writing because the two can disagree, for example on
/// surrogate pairs.
pub(crate) fn range_before(
    caret: &IUIAutomationTextRange,
    gap: usize,
    len: usize,
) -> Result<IUIAutomationTextRange> {
    // SAFETY: `caret` is a live interface, and the endpoints are moved on a
    // clone so `caret` itself is unchanged.
    unsafe {
        let range = caret.Clone()?;
        range.MoveEndpointByUnit(
            TextPatternRangeEndpoint_Start,
            TextUnit_Character,
            -((gap + len) as i32),
        )?;
        if gap > 0 {
            range.MoveEndpointByUnit(
                TextPatternRangeEndpoint_End,
                TextUnit_Character,
                -(gap as i32),
            )?;
        }
        Ok(range)
    }
}

/// Returns an empty range `count` characters after `caret`.
pub(crate) fn caret_after(
    caret: &IUIAutomationTextRange,
    count: usize,
) -> Result<IUIAutomationTextRange> {
    // SAFETY: `caret` is a live interface, and the move is done on a clone.
    // Moving an empty range keeps it empty.
    unsafe {
        let moved = caret.Clone()?;
        moved.Move(TextUnit_Character, count as i32)?;
        Ok(moved)
    }
}

/// Returns the line the caret is on, from its start up to the caret.
///
/// Terminals report every screen row as a line, and a row that wraps onto the
/// next one has no line break at its end. Such rows are joined, up to
/// `max_rows` of them, so a long command reads as one line.
pub(crate) fn line_before_caret(
    pattern: &IUIAutomationTextPattern,
    max_rows: usize,
) -> Result<Option<String>> {
    let Some(caret) = first_selection(pattern)? else {
        return Ok(None);
    };
    // SAFETY: `caret` and `pattern` are live interfaces, and all moves are
    // done on clones, so the selection itself is unchanged.
    unsafe {
        let line = caret.Clone()?;
        line.ExpandToEnclosingUnit(TextUnit_Line)?;
        let row = line.Clone()?;
        for _ in 1..max_rows {
            if row.Move(TextUnit_Line, -1)? == 0 || text(&row)?.ends_with('\n') {
                break;
            }
            line.MoveEndpointByRange(
                TextPatternRangeEndpoint_Start,
                &row,
                TextPatternRangeEndpoint_Start,
            )?;
        }
        line.MoveEndpointByRange(
            TextPatternRangeEndpoint_End,
            &caret,
            TextPatternRangeEndpoint_Start,
        )?;
        text(&line).map(Some)
    }
}

/// Returns the text of the first selection.
pub(crate) fn selection_text(pattern: &IUIAutomationTextPattern) -> Result<String> {
    // SAFETY: `pattern` is a live interface, and the element index is checked
    // against the length first.
    unsafe {
        let selections = pattern.GetSelection()?;
        if selections.Length()? == 0 {
            return Ok(String::new());
        }
        text(&selections.GetElement(0)?)
    }
}

/// Selects `range` in its control, which moves the caret there.
pub(crate) fn select(range: &IUIAutomationTextRange) -> Result<()> {
    // SAFETY: `range` is a live interface.
    unsafe { range.Select() }
}

/// Removes every event handler this client registered.
pub(crate) fn remove_all_handlers(uia: &IUIAutomation) {
    // SAFETY: `uia` is a live interface.
    if let Err(e) = unsafe { uia.RemoveAllEventHandlers() } {
        tracing::debug!("RemoveAllEventHandlers failed: {e}");
    }
}

/// Returns the length of `s` in UTF-16 code units, the unit UIA offsets are
/// expressed in.
pub(crate) fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}
