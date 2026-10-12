// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The settings window: a modeless Win32 property sheet with one page per
//! topic.
//!
//! Page layouts come from `resources/typomend.rc`; every caption is filled in
//! at run time so the sheet follows the language chosen in the settings. The
//! sheet, its pages and this module's state all live on the UI thread, which
//! is why plain thread-locals are enough.
//!
//! A modeless property sheet never destroys itself. The message loop hands
//! every message to [`handle_message`], which routes keyboard navigation to
//! the sheet and destroys it once the user has closed it.
//!
//! The Win32 constants below are written out instead of imported from the
//! `windows` crate, which wraps them in differing typed newtypes. Each keeps
//! its name from the Windows SDK headers.
//!
//! Changes made in the sheet apply at once, so there is no OK or Apply step.
//! The OK button is turned into a Close button for that reason.

mod bench_window;
mod chart;
mod controls;
mod ids;
mod pages;
mod try_window;
mod window;

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::MapWindowPoints;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    DRAWITEMSTRUCT, NMHDR, PROPSHEETHEADERW_V2, PROPSHEETHEADERW_V2_0, PROPSHEETHEADERW_V2_1,
    PROPSHEETHEADERW_V2_2, PROPSHEETPAGEW, PROPSHEETPAGEW_0, PropertySheetW, TCITEMHEADERA_MASK,
    TCITEMW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, GetWindowLongPtrW, GetWindowRect, ICON_BIG, MSG, SWP_NOSIZE, SWP_NOZORDER,
    SetWindowLongPtrW, SetWindowPos, WINDOW_LONG_PTR_INDEX, WM_COMMAND, WM_DESTROY, WM_DRAWITEM,
    WM_INITDIALOG, WM_NOTIFY, WM_SETICON,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use crate::state::AppState;
use crate::{fl, icon, platform, tray};

// Property sheet, list view and tab control values from the Windows SDK
// headers, spelled out for the reason given in the `controls` module.
const PSP_USETITLE: u32 = 0x0008;
const PSH_USEHICON: u32 = 0x0002;
const PSH_PROPSHEETPAGE: u32 = 0x0008;
const PSH_NOAPPLYNOW: u32 = 0x0080;
const PSH_MODELESS: u32 = 0x0400;
const PSH_NOCONTEXTHELP: u32 = 0x0200_0000;
const PSM_GETTABCONTROL: u32 = 0x0400 + 116;
const PSM_ISDIALOGMESSAGE: u32 = 0x0400 + 117;
const PSM_GETCURRENTPAGEHWND: u32 = 0x0400 + 118;
const PSN_SETACTIVE: u32 = -200i32 as u32;
const LVN_GETDISPINFOW: u32 = -177i32 as u32;
const TCM_SETITEMW: u32 = 0x1300 + 61;
const TCIF_TEXT: u32 = 0x0001;
const IDOK: i32 = 1;
const IDCANCEL: i32 = 2;

// winuser.h places DWLP_USER after the two pointer-sized slots that a dialog
// keeps for its message result and its dialog procedure.
const DWLP_USER: WINDOW_LONG_PTR_INDEX = WINDOW_LONG_PTR_INDEX(2 * size_of::<isize>() as i32);

/// A page of the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Page {
    Overview,
    Rules,
    /// The Masked language model page.
    Model,
    /// The Usage page, which charts Typomend's own resource use.
    Usage,
    Apps,
    History,
    Settings,
    About,
}

impl Page {
    /// Every page, in the order of the tabs. A page's position here is the
    /// index it stores in its window.
    const ALL: [Self; 8] = [
        Self::Overview,
        Self::Rules,
        Self::Model,
        Self::Usage,
        Self::Apps,
        Self::History,
        Self::Settings,
        Self::About,
    ];

    /// Returns the resource id of the dialog template of the page.
    fn template(self) -> u16 {
        match self {
            Self::Overview => ids::IDD_OVERVIEW,
            Self::Rules => ids::IDD_RULES,
            Self::Model => ids::IDD_MODEL,
            Self::Usage => ids::IDD_USAGE,
            Self::Apps => ids::IDD_APPS,
            Self::History => ids::IDD_HISTORY,
            Self::Settings => ids::IDD_SETTINGS,
            Self::About => ids::IDD_ABOUT,
        }
    }

    /// Returns the tab caption of the page in the current language.
    fn title(self) -> String {
        match self {
            Self::Overview => fl!("tab-overview"),
            Self::Rules => fl!("tab-rules"),
            Self::Model => fl!("tab-model"),
            Self::Usage => fl!("tab-usage"),
            Self::Apps => fl!("tab-apps"),
            Self::History => fl!("tab-history"),
            Self::Settings => fl!("tab-settings"),
            Self::About => fl!("tab-about"),
        }
    }

    /// Returns the page at `index` in [`Page::ALL`], or `None` when `index` is
    /// out of range.
    fn from_index(index: isize) -> Option<Self> {
        Self::ALL.get(usize::try_from(index).ok()?).copied()
    }
}

thread_local! {
    // Shared state, set by `init`.
    static STATE: RefCell<Option<Arc<AppState>>> = const { RefCell::new(None) };
    // Handle of the open property sheet, or 0 while none is open.
    static SHEET: Cell<isize> = const { Cell::new(0) };
    // The pages created so far, with their window handles.
    static PAGES: RefCell<Vec<(Page, isize)>> = const { RefCell::new(Vec::new()) };
    // Set while the program itself changes controls, so the notifications
    // those changes raise are not mistaken for user input.
    static UPDATING: Cell<bool> = const { Cell::new(false) };
}

/// Hands the shared state to the dialog. Must be called on the UI thread
/// before [`show`].
pub fn init(state: Arc<AppState>) {
    STATE.with(|s| *s.borrow_mut() = Some(state));
}

/// Returns the shared state handed over by [`init`].
///
/// # Panics
///
/// Panics if [`init`] has not been called on this thread.
fn state() -> Arc<AppState> {
    STATE
        .with(|s| s.borrow().clone())
        .expect("dialog::init was called")
}

/// Returns the open property sheet, if any.
fn sheet() -> Option<HWND> {
    let raw = SHEET.get();
    (raw != 0).then_some(HWND(raw as *mut _))
}

/// Runs `f` with `UPDATING` set, so the notifications its control changes
/// raise are not taken for user input.
fn quietly(f: impl FnOnce()) {
    let was = UPDATING.replace(true);
    f();
    UPDATING.set(was);
}

/// Opens the settings window, or brings it to the front if it is open.
///
/// Logs an error and opens nothing if the property sheet cannot be created.
pub fn show() {
    reap_closed_sheet();
    if let Some(sheet) = sheet() {
        platform::bring_to_front(sheet);
        return;
    }
    // SAFETY: GetModuleHandleW(None) takes no pointer and returns the handle
    // of the executable, which stays loaded for the life of the process.
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }
        .map(Into::into)
        .unwrap_or_default();
    let titles: Vec<HSTRING> = Page::ALL.iter().map(|p| HSTRING::from(p.title())).collect();
    let mut pages: Vec<PROPSHEETPAGEW> = Page::ALL
        .iter()
        .zip(&titles)
        .enumerate()
        .map(|(i, (page, title))| PROPSHEETPAGEW {
            dwSize: size_of::<PROPSHEETPAGEW>() as u32,
            dwFlags: PSP_USETITLE,
            hInstance: instance,
            // MAKEINTRESOURCE: the template is named by its integer id,
            // passed in place of a string pointer.
            Anonymous1: PROPSHEETPAGEW_0 {
                pszTemplate: PCWSTR(usize::from(page.template()) as *const u16),
            },
            pszTitle: PCWSTR(title.as_ptr()),
            pfnDlgProc: Some(page_proc),
            lParam: LPARAM(i as isize),
            ..Default::default()
        })
        .collect();
    let caption = HSTRING::from(platform::WINDOW_TITLE);
    let mut header = PROPSHEETHEADERW_V2 {
        dwSize: size_of::<PROPSHEETHEADERW_V2>() as u32,
        dwFlags: PSH_PROPSHEETPAGE
            | PSH_MODELESS
            | PSH_NOAPPLYNOW
            | PSH_NOCONTEXTHELP
            | PSH_USEHICON,
        hInstance: instance,
        Anonymous1: PROPSHEETHEADERW_V2_0 {
            hIcon: icon::app(false),
        },
        pszCaption: PCWSTR(caption.as_ptr()),
        nPages: pages.len() as u32,
        Anonymous2: PROPSHEETHEADERW_V2_1 { nStartPage: 0 },
        Anonymous3: PROPSHEETHEADERW_V2_2 {
            ppsp: pages.as_mut_ptr(),
        },
        ..Default::default()
    };
    // SAFETY: `header`, `pages`, `titles` and `caption` live until the call
    // returns, and PropertySheetW copies what it keeps of them. The page
    // template ids and the dialog procedure refer to resources and code of
    // this executable, which stays loaded.
    let raw = unsafe { PropertySheetW(&mut header) };
    if raw <= 0 {
        tracing::error!("PropertySheetW failed");
        return;
    }
    SHEET.set(raw);
    let sheet = HWND(raw as *mut _);
    // The header icon only covers the title bar; Alt+Tab and the taskbar
    // want the large one.
    controls::send(
        sheet,
        WM_SETICON,
        ICON_BIG as usize,
        icon::app(true).0 as isize,
    );
    convert_to_close_button(sheet);
    platform::bring_to_front(sheet);
}

/// Turns the OK button into a Close button in Cancel's place, on the right.
///
/// Changes apply as soon as they are made, so the sheet needs no other
/// button.
fn convert_to_close_button(sheet: HWND) {
    let ok = controls::item(sheet, IDOK);
    let cancel = controls::item(sheet, IDCANCEL);
    let mut rect = RECT::default();
    // SAFETY: the handles belong to the sheet this thread just created, and a
    // missing button only makes a call fail. `rect` and `origin` are valid for
    // the calls that write them.
    unsafe {
        if GetWindowRect(cancel, &mut rect).is_ok() {
            let mut origin = [POINT {
                x: rect.left,
                y: rect.top,
            }];
            MapWindowPoints(None, Some(sheet), &mut origin);
            let _ = SetWindowPos(
                ok,
                None,
                origin[0].x,
                origin[0].y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER,
            );
        }
    }
    controls::show(sheet, IDCANCEL, false);
    controls::set_text(sheet, IDOK, &fl!("dialog-close"));
}

/// Gives the settings window and the tool windows opened from it a chance to
/// handle `msg`, for keyboard navigation. Returns whether one did, in which
/// case the message must not be translated or dispatched.
pub fn handle_message(msg: &MSG) -> bool {
    if try_window::handle_message(msg) || bench_window::handle_message(msg) {
        return true;
    }
    let Some(sheet) = sheet() else {
        return false;
    };
    let handled = controls::send(sheet, PSM_ISDIALOGMESSAGE, 0, msg as *const MSG as isize) != 0;
    reap_closed_sheet();
    handled
}

/// Destroys the sheet once the user has closed it, which a modeless sheet
/// signals only by reporting no current page.
///
/// Checked after every message, and again before the sheet is used, because
/// a close that happens inside a cross-thread SendMessage is followed by no
/// message at all.
fn reap_closed_sheet() {
    let Some(sheet) = sheet() else { return };
    if controls::send(sheet, PSM_GETCURRENTPAGEHWND, 0, 0) == 0 {
        // SAFETY: `sheet` is the property sheet this thread created, which is
        // the thread DestroyWindow requires. SHEET is cleared right after, so
        // the sheet is destroyed only once.
        unsafe {
            let _ = DestroyWindow(sheet);
        }
        SHEET.set(0);
        PAGES.with(|p| p.borrow_mut().clear());
    }
}

/// Updates the open pages and tool windows with the latest state.
pub fn refresh() {
    try_window::refresh();
    bench_window::refresh();
    reap_closed_sheet();
    for (page, hwnd) in open_pages() {
        quietly(|| pages::refresh(page, hwnd, false));
    }
}

/// Updates the Usage page with the latest resource usage sample.
pub fn refresh_monitor() {
    reap_closed_sheet();
    for (page, hwnd) in open_pages() {
        if page == Page::Usage {
            quietly(|| pages::refresh_usage(hwnd));
        }
    }
}

/// Re-applies every caption of the settings window, the tool windows and the
/// tray menu after the language changed.
pub fn relabel() {
    try_window::relabel();
    bench_window::relabel();
    reap_closed_sheet();
    if let Some(sheet) = sheet() {
        let tab = HWND(controls::send(sheet, PSM_GETTABCONTROL, 0, 0) as *mut _);
        for (i, page) in Page::ALL.iter().enumerate() {
            let mut title: Vec<u16> = page.title().encode_utf16().chain([0]).collect();
            let item = TCITEMW {
                mask: TCITEMHEADERA_MASK(TCIF_TEXT),
                pszText: PWSTR(title.as_mut_ptr()),
                ..Default::default()
            };
            controls::send(tab, TCM_SETITEMW, i, &item as *const _ as isize);
        }
        controls::set_text(sheet, IDOK, &fl!("dialog-close"));
    }
    for (page, hwnd) in open_pages() {
        quietly(|| {
            pages::texts(page, hwnd);
            pages::refresh(page, hwnd, true);
        });
    }
    tray::sync(&state());
}

/// Returns the pages created so far with their window handles.
///
/// Returns a copy, so callers may run code that creates or destroys pages
/// while iterating.
fn open_pages() -> Vec<(Page, HWND)> {
    PAGES.with(|p| {
        p.borrow()
            .iter()
            .map(|&(page, raw)| (page, HWND(raw as *mut _)))
            .collect()
    })
}

/// Returns the page whose window is `hwnd`, from the index it stored when it
/// was initialized.
fn page_of(hwnd: HWND) -> Option<Page> {
    // SAFETY: GetWindowLongPtrW has no pointer arguments, and DWLP_USER is a
    // slot every dialog window has. A stale handle only makes it return 0.
    Page::from_index(unsafe { GetWindowLongPtrW(hwnd, DWLP_USER) })
}

/// Dialog procedure of every property page.
///
/// Looks up the page from the index stored in the window and forwards each
/// message to [`pages`].
///
/// # Safety
///
/// Windows calls it with the arguments of a dialog message, whose `lparam`
/// points to the structure that `msg` defines, if any.
unsafe extern "system" fn page_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            // SAFETY: for a property page, the lparam of WM_INITDIALOG points
            // to the sheet's copy of its PROPSHEETPAGEW, whose lParam holds
            // the page index set in `show`.
            let index = unsafe { (*(lparam.0 as *const PROPSHEETPAGEW)).lParam.0 };
            let Some(page) = Page::from_index(index) else {
                return 0;
            };
            // SAFETY: SetWindowLongPtrW has no pointer arguments, and
            // DWLP_USER is a slot every dialog window reserves for its owner.
            unsafe { SetWindowLongPtrW(hwnd, DWLP_USER, index) };
            PAGES.with(|p| p.borrow_mut().push((page, hwnd.0 as isize)));
            quietly(|| {
                pages::init(page, hwnd);
                pages::texts(page, hwnd);
                pages::refresh(page, hwnd, true);
            });
            1
        }
        WM_COMMAND => {
            if let Some(page) = page_of(hwnd)
                && !UPDATING.get()
            {
                let id = (wparam.0 & 0xFFFF) as i32;
                let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
                pages::command(page, hwnd, id, code);
            }
            0
        }
        WM_NOTIFY => {
            let Some(page) = page_of(hwnd) else { return 0 };
            // SAFETY: for WM_NOTIFY, `lparam` always points to an NMHDR that
            // is valid for the duration of the message. It may be the head of
            // a larger structure, which pages::notify reads by code.
            let header = unsafe { &*(lparam.0 as *const NMHDR) };
            if header.code == LVN_GETDISPINFOW {
                pages::display_info(page, lparam);
            } else if header.code == PSN_SETACTIVE {
                quietly(|| pages::refresh(page, hwnd, true));
            } else if !UPDATING.get() {
                pages::notify(page, hwnd, header, lparam);
            }
            0
        }
        WM_DRAWITEM => {
            let Some(page) = page_of(hwnd) else { return 0 };
            // SAFETY: for WM_DRAWITEM, `lparam` always points to a
            // DRAWITEMSTRUCT that is valid for the duration of the message.
            let item = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
            isize::from(pages::draw_item(page, hwnd, item))
        }
        WM_DESTROY => {
            if let Some(page) = page_of(hwnd) {
                pages::destroy(page, hwnd);
            }
            PAGES.with(|p| p.borrow_mut().retain(|&(_, raw)| raw != hwnd.0 as isize));
            0
        }
        _ => 0,
    }
}
