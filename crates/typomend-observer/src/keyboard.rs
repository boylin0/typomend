// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Records when keys are pressed, using a low-level keyboard hook that only
//! observes and always passes the key on.
//!
//! Some editors, VS Code with accessibility support off being the known case,
//! raise a UIA Composition event for every keystroke of a composition but
//! nothing at all when it is committed. Enter is the usual commit key, so
//! seeing Enter while a composition is pending is the only commit signal left.
//!
//! A WH_KEYBOARD_LL callback runs on the thread that installed the hook and
//! needs a message loop there, hence the dedicated thread. The callback has no
//! user data pointer, so its state lives in statics.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_RETURN;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG, SetWindowsHookExW,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
};

use crate::input::SELF_INPUT_TAG;

/// The instant that stamps count from.
static EPOCH: OnceLock<Instant> = OnceLock::new();
/// When the user last pressed a key, as a [`stamp`].
static LAST_KEY: AtomicU64 = AtomicU64::new(0);
/// When the user last pressed Enter, as a [`stamp`].
static LAST_ENTER: AtomicU64 = AtomicU64::new(0);

/// Installs the hook on a background thread. Later calls do nothing.
pub(crate) fn start() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        EPOCH.get_or_init(Instant::now);
        std::thread::Builder::new()
            .name("typomend-keyboard".into())
            .spawn(run)
            .expect("spawning the keyboard thread");
    });
}

/// Installs the hook and pumps this thread's messages, which the hook needs,
/// for the rest of the program.
fn run() {
    // SAFETY: the hook procedure is a plain extern "system" function that
    // lives for the whole program.
    let hook = unsafe {
        let module = GetModuleHandleW(None).ok().map(Into::into);
        SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module, 0)
    };
    if let Err(e) = hook {
        tracing::error!("installing the keyboard hook failed: {e}");
        return;
    }
    let mut msg = MSG::default();
    // SAFETY: standard message loop on the thread that owns the hook.
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {}
}

/// Records the time of every key the user presses, then passes the key on.
///
/// # Safety
///
/// Only Windows may call this, as the WH_KEYBOARD_LL hook procedure, so that
/// `lparam` points to a valid KBDLLHOOKSTRUCT whenever `code` is HC_ACTION.
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
        // SAFETY: for WH_KEYBOARD_LL, lparam points to a KBDLLHOOKSTRUCT.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if info.dwExtraInfo != SELF_INPUT_TAG {
            let now = stamp();
            LAST_KEY.store(now, Ordering::Release);
            if info.vkCode == u32::from(VK_RETURN.0) {
                LAST_ENTER.store(now, Ordering::Release);
            }
        }
    }
    // Every hook must pass the event on, or other hooks and the target
    // window never see it.
    // SAFETY: forwards the arguments Windows passed in, unchanged.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Returns the time since [`EPOCH`] in nanoseconds, plus one so that a
/// stored zero means never.
fn stamp() -> u64 {
    EPOCH.get().map_or(1, |e| e.elapsed().as_nanos() as u64 + 1)
}

/// Turns a [`stamp`] back into an instant, or `None` for zero.
fn instant(stamp: u64) -> Option<Instant> {
    let epoch = *EPOCH.get()?;
    (stamp != 0).then(|| epoch + Duration::from_nanos(stamp - 1))
}

/// Returns when the user last pressed Enter, or `None` if they have not.
pub(crate) fn last_enter() -> Option<Instant> {
    instant(LAST_ENTER.load(Ordering::Acquire))
}

/// Returns when the user last pressed any key, Enter included.
pub(crate) fn last_key() -> Option<Instant> {
    instant(LAST_KEY.load(Ordering::Acquire))
}
