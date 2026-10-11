// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Small wrappers over the messages that drive standard Win32 controls.
//!
//! Message numbers are spelled out here rather than imported because the
//! `windows` crate exposes them under assorted typed wrappers; each constant
//! carries its name from the Windows SDK headers.
//!
//! Every wrapper takes the dialog and a control id, so callers never hold a
//! control handle across a page being destroyed. A control that does not exist
//! resolves to a null handle, which turns each call into a harmless no-op.

use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::UI::Controls::{LVCOLUMNW, LVITEMW};
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetDlgItem, GetWindowTextLengthW, GetWindowTextW, SW_HIDE, SW_SHOW,
    SendMessageW, SetDlgItemTextW, ShowWindow,
};
use windows::core::{HSTRING, PWSTR};

// Button, combo box and up-down control messages.
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const BST_CHECKED: usize = 1;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_RESETCONTENT: u32 = 0x014B;
const CB_SETCURSEL: u32 = 0x014E;
const CB_SETDROPPEDWIDTH: u32 = 0x0160;
const UDM_SETRANGE32: u32 = 0x0400 + 111;
const UDM_SETPOS32: u32 = 0x0400 + 113;
const UDM_GETPOS32: u32 = 0x0400 + 114;

// List view messages, styles and flags.
const LVM_GETITEMCOUNT: u32 = 0x1004;
const LVM_DELETEALLITEMS: u32 = 0x1009;
const LVM_GETNEXTITEM: u32 = 0x100C;
const LVM_ENSUREVISIBLE: u32 = 0x1013;
const LVM_SETCOLUMNWIDTH: u32 = 0x101E;
const LVM_SETITEMSTATE: u32 = 0x102B;
const LVM_SETITEMCOUNT: u32 = 0x102F;
const LVM_SETEXTENDEDLISTVIEWSTYLE: u32 = 0x1036;
const LVM_INSERTITEMW: u32 = 0x104D;
const LVM_SETCOLUMNW: u32 = 0x1060;
const LVM_INSERTCOLUMNW: u32 = 0x1061;
const LVM_GETITEMTEXTW: u32 = 0x1073;
const LVM_SETITEMTEXTW: u32 = 0x1074;
const LVS_EX_FULLROWSELECT: isize = 0x0020;
const LVS_EX_DOUBLEBUFFER: isize = 0x0001_0000;
const LVIF_TEXT: u32 = 0x0001;
const LVIF_STATE: u32 = 0x0008;
const LVCF_TEXT: u32 = 0x0004;
const LVCF_WIDTH: u32 = 0x0002;
const LVSCW_AUTOSIZE_USEHEADER: isize = -2;
const LVNI_SELECTED: isize = 0x0002;

/// List view item state of a selected row.
pub const LVIS_SELECTED: u32 = 0x0002;
const LVIS_FOCUSED: u32 = 0x0001;

/// Returns control `id` of `dialog`, or a null handle when there is none.
pub fn item(dialog: HWND, id: i32) -> HWND {
    // SAFETY: GetDlgItem has no pointer arguments, and an invalid window or
    // an unknown id makes it fail, which maps to a null handle.
    unsafe { GetDlgItem(Some(dialog), id) }.unwrap_or_default()
}

/// Sends `msg` to `hwnd` and returns the result.
///
/// `wparam` and `lparam` may carry pointers to locals of the caller: the
/// call is synchronous, so they only need to stay valid until it returns.
pub fn send(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: SendMessageW returns only after the message is handled, and
    // every caller passes either plain values or pointers to data that
    // outlives the call.
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(wparam)), Some(LPARAM(lparam))) }.0
}

/// Sets the text of control `id` of `dialog`.
pub fn set_text(dialog: HWND, id: i32, text: &str) {
    // SAFETY: the HSTRING is null-terminated and outlives the synchronous
    // call, and an invalid window only makes the call fail.
    unsafe {
        let _ = SetDlgItemTextW(dialog, id, &HSTRING::from(text));
    }
}

/// Returns the text of control `id` of `dialog`, empty when there is none.
pub fn text(dialog: HWND, id: i32) -> String {
    let control = item(dialog, id);
    // SAFETY: the buffer has room for the text plus its terminator, and
    // GetWindowTextW never writes past the length of the slice it is given.
    unsafe {
        let mut buf = vec![0u16; GetWindowTextLengthW(control).max(0) as usize + 1];
        let n = GetWindowTextW(control, &mut buf).max(0) as usize;
        String::from_utf16_lossy(&buf[..n])
    }
}

/// Shows or hides control `id` of `dialog`.
pub fn show(dialog: HWND, id: i32, visible: bool) {
    // SAFETY: ShowWindow has no pointer arguments, and a null or stale handle
    // only makes it fail.
    unsafe {
        let _ = ShowWindow(item(dialog, id), if visible { SW_SHOW } else { SW_HIDE });
    }
}

/// Enables or disables control `id` of `dialog`.
pub fn enable(dialog: HWND, id: i32, enabled: bool) {
    // SAFETY: EnableWindow has no pointer arguments, and a null or stale
    // handle only makes it fail.
    unsafe {
        let _ = EnableWindow(item(dialog, id), enabled);
    }
}

/// Returns whether check box or radio button `id` is checked.
pub fn checked(dialog: HWND, id: i32) -> bool {
    send(item(dialog, id), BM_GETCHECK, 0, 0) as usize == BST_CHECKED
}

/// Checks or clears check box or radio button `id`.
pub fn set_checked(dialog: HWND, id: i32, checked: bool) {
    send(item(dialog, id), BM_SETCHECK, usize::from(checked), 0);
}

/// Replaces the items of combo box `id` with `items` and selects item
/// `selected`.
pub fn combo_fill(dialog: HWND, id: i32, items: &[String], selected: usize) {
    let combo = item(dialog, id);
    send(combo, CB_RESETCONTENT, 0, 0);
    for text in items {
        let wide = HSTRING::from(text.as_str());
        send(combo, CB_ADDSTRING, 0, wide.as_ptr() as isize);
    }
    send(combo, CB_SETCURSEL, selected, 0);
}

/// Makes the drop-down list of a combo box `percent` percent as wide as the
/// box, so long items are not cut off.
pub fn combo_widen_list(dialog: HWND, id: i32, percent: i32) {
    let combo = item(dialog, id);
    let mut rect = RECT::default();
    // SAFETY: `rect` is a valid out-pointer for the call, and a null handle
    // only makes it fail and leaves `rect` zeroed.
    unsafe {
        let _ = GetClientRect(combo, &mut rect);
    }
    let width = (rect.right - rect.left) * percent / 100;
    send(combo, CB_SETDROPPEDWIDTH, width.max(0) as usize, 0);
}

/// Returns the index of the selected item of combo box `id`, or `None` when
/// nothing is selected.
pub fn combo_selected(dialog: HWND, id: i32) -> Option<usize> {
    usize::try_from(send(item(dialog, id), CB_GETCURSEL, 0, 0)).ok()
}

/// Selects item `index`, or clears the selection when it is `None`.
pub fn combo_select(dialog: HWND, id: i32, index: Option<usize>) {
    send(
        item(dialog, id),
        CB_SETCURSEL,
        index.unwrap_or(usize::MAX),
        0,
    );
}

/// Sets the range and the value of up-down control `id`.
///
/// The control must have a buddy edit, which shows the value.
pub fn spin_setup(dialog: HWND, id: i32, min: i32, max: i32, value: i32) {
    let spin = item(dialog, id);
    send(spin, UDM_SETRANGE32, min as usize, max as isize);
    send(spin, UDM_SETPOS32, 0, value as isize);
}

/// Returns the value of up-down control `id`.
///
/// Returns `None` while the buddy edit holds text that is not a number in the
/// range, such as when the user has cleared it to type a new one.
pub fn spin_value(dialog: HWND, id: i32) -> Option<i32> {
    let mut error = 0i32;
    let value = send(
        item(dialog, id),
        UDM_GETPOS32,
        0,
        &mut error as *mut i32 as isize,
    );
    (error == 0).then_some(value as i32)
}

/// A report-mode list view.
///
/// Wraps only the handle, so it is `Copy` and does not own the control.
#[derive(Debug, Clone, Copy)]
pub struct ListView(pub HWND);

impl ListView {
    /// Returns the list view `id` of `dialog`.
    pub fn new(dialog: HWND, id: i32) -> Self {
        Self(item(dialog, id))
    }

    /// Turns on full-row selection and double buffering, and adds `columns`,
    /// each a title and a weight. The weights share out the width of the
    /// control.
    pub fn setup(self, columns: &[(String, u32)]) {
        let style = LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER;
        send(self.0, LVM_SETEXTENDEDLISTVIEWSTYLE, style as usize, style);

        let mut rect = RECT::default();
        // SAFETY: `rect` is a valid out-pointer for the call, and a null
        // handle only makes it fail and leaves `rect` zeroed.
        unsafe {
            let _ = GetClientRect(self.0, &mut rect);
        }
        let total: u32 = columns.iter().map(|(_, w)| w).sum();
        let width = rect.right - rect.left;
        for (i, (text, weight)) in columns.iter().enumerate() {
            let mut wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
            let column = LVCOLUMNW {
                mask: windows::Win32::UI::Controls::LVCOLUMNW_MASK(LVCF_TEXT | LVCF_WIDTH),
                cx: width * *weight as i32 / total.max(1) as i32,
                pszText: PWSTR(wide.as_mut_ptr()),
                ..Default::default()
            };
            send(self.0, LVM_INSERTCOLUMNW, i, &column as *const _ as isize);
        }
        // The last column takes whatever width is left, so no horizontal
        // scroll bar appears.
        send(
            self.0,
            LVM_SETCOLUMNWIDTH,
            columns.len().saturating_sub(1),
            LVSCW_AUTOSIZE_USEHEADER,
        );
    }

    /// Renames the columns, in order, keeping their widths.
    pub fn set_column_titles(self, titles: &[String]) {
        for (i, text) in titles.iter().enumerate() {
            let mut wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
            let column = LVCOLUMNW {
                mask: windows::Win32::UI::Controls::LVCOLUMNW_MASK(LVCF_TEXT),
                pszText: PWSTR(wide.as_mut_ptr()),
                ..Default::default()
            };
            send(self.0, LVM_SETCOLUMNW, i, &column as *const _ as isize);
        }
    }

    /// Sets the row count of an LVS_OWNERDATA list view and repaints it.
    ///
    /// The list view then asks for each visible cell through
    /// LVN_GETDISPINFO instead of storing the text.
    pub fn set_virtual_len(self, len: usize) {
        send(self.0, LVM_SETITEMCOUNT, len, 0);
        // SAFETY: no rectangle is passed, so the whole client area is
        // invalidated, and a null handle only makes the call fail.
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(self.0), None, true);
        }
    }

    /// Removes every row.
    pub fn clear(self) {
        send(self.0, LVM_DELETEALLITEMS, 0, 0);
    }

    /// Returns the number of rows.
    pub fn len(self) -> usize {
        send(self.0, LVM_GETITEMCOUNT, 0, 0).max(0) as usize
    }

    /// Appends a row and returns its index.
    pub fn push(self, cells: &[String]) -> usize {
        let index = self.len() as i32;
        let mut first: Vec<u16> = cells
            .first()
            .map_or(String::new(), Clone::clone)
            .encode_utf16()
            .chain([0])
            .collect();
        let row = LVITEMW {
            mask: windows::Win32::UI::Controls::LIST_VIEW_ITEM_FLAGS(LVIF_TEXT),
            iItem: index,
            pszText: PWSTR(first.as_mut_ptr()),
            ..Default::default()
        };
        let index = send(self.0, LVM_INSERTITEMW, 0, &row as *const _ as isize).max(0) as usize;
        for (column, text) in cells.iter().enumerate().skip(1) {
            let mut wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
            let cell = LVITEMW {
                iSubItem: column as i32,
                pszText: PWSTR(wide.as_mut_ptr()),
                ..Default::default()
            };
            send(self.0, LVM_SETITEMTEXTW, index, &cell as *const _ as isize);
        }
        index
    }

    /// Returns the selected row of a single-selection list view, or `None`
    /// when no row is selected.
    pub fn selected(self) -> Option<usize> {
        usize::try_from(send(self.0, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED)).ok()
    }

    /// Selects and focuses row `index` and scrolls it into view.
    pub fn select(self, index: usize) {
        let flags = LVIS_SELECTED | LVIS_FOCUSED;
        let state = LVITEMW {
            mask: windows::Win32::UI::Controls::LIST_VIEW_ITEM_FLAGS(LVIF_STATE),
            state: windows::Win32::UI::Controls::LIST_VIEW_ITEM_STATE_FLAGS(flags),
            stateMask: windows::Win32::UI::Controls::LIST_VIEW_ITEM_STATE_FLAGS(flags),
            ..Default::default()
        };
        send(self.0, LVM_SETITEMSTATE, index, &state as *const _ as isize);
        send(self.0, LVM_ENSUREVISIBLE, index, 0);
    }

    /// Returns the text of the cell at row `index` and `column`, cut off
    /// after 511 UTF-16 code units.
    pub fn text(self, index: usize, column: usize) -> String {
        let mut buf = [0u16; 512];
        let cell = LVITEMW {
            iSubItem: column as i32,
            pszText: PWSTR(buf.as_mut_ptr()),
            cchTextMax: buf.len() as i32,
            ..Default::default()
        };
        let n = send(self.0, LVM_GETITEMTEXTW, index, &cell as *const _ as isize).max(0) as usize;
        String::from_utf16_lossy(&buf[..n.min(buf.len())])
    }
}
