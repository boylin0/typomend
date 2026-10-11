// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Writing text into other programs.
//!
//! Keys go out through `SendInput`, one group at a time and [`KEY_PACE`]
//! apart, each tagged with [`SELF_INPUT_TAG`] so the keyboard hook can tell
//! them from the user's. Edit and RichEdit controls take a replacement
//! directly through `EM_REPLACESEL` instead.

use std::time::Duration;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetLastInputInfo, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, LASTINPUTINFO, SendInput,
    VIRTUAL_KEY, VK_BACK, VK_DELETE, VK_LEFT, VK_RIGHT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, SMTO_ABORTIFHUNG, SendMessageTimeoutW,
};

const EM_REPLACESEL: u32 = 0x00C2;

/// Pause between two keys this process sends. Windows 11 Notepad handles
/// arrow keys and typed characters on different paths, and in a burst they
/// come out of order: a Backspace that overtakes an arrow key deletes the
/// wrong text. 30 ms kept every test in order; 10 ms did not.
const KEY_PACE: Duration = Duration::from_millis(30);

/// Tag placed in `dwExtraInfo` of every key this process sends, so the keyboard
/// monitor can tell them apart from keys the user presses.
pub(crate) const SELF_INPUT_TAG: usize = 0x5459_504F; // "TYPO"

pub(crate) use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY as Vk;
pub(crate) const KEY_BACK: Vk = VK_BACK;
pub(crate) const KEY_DELETE: Vk = VK_DELETE;
pub(crate) const KEY_LEFT: Vk = VK_LEFT;
pub(crate) const KEY_RIGHT: Vk = VK_RIGHT;

/// Returns how long the session has gone without keyboard or mouse input.
pub(crate) fn idle_ms() -> u64 {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `info` is a valid, correctly sized LASTINPUTINFO.
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return 0;
    }
    // Both counters wrap every 49.7 days; a wrapping difference stays right
    // across the wrap.
    // SAFETY: GetTickCount has no preconditions.
    u64::from(unsafe { GetTickCount() }.wrapping_sub(info.dwTime))
}

/// Returns the window class name of `hwnd`.
pub(crate) fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    // SAFETY: the buffer is valid for its whole length.
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Replaces the selection of an Edit or RichEdit control in another process.
/// Returns whether the control received the message.
///
/// EM_REPLACESEL is marshalled across processes by user32 and never passes
/// through the IME. `wParam = 1` keeps the change undoable with Ctrl+Z.
pub(crate) fn replace_selection(hwnd: HWND, text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let mut result = 0usize;
    // SAFETY: `wide` is NUL-terminated and outlives the synchronous call.
    let ok = unsafe {
        SendMessageTimeoutW(
            hwnd,
            EM_REPLACESEL,
            WPARAM(1),
            LPARAM(wide.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            1000,
            Some(&mut result),
        )
    };
    ok.0 != 0
}

/// Returns whether a window of this class accepts EM_REPLACESEL.
pub(crate) fn supports_replace_selection(class: &str) -> bool {
    class.eq_ignore_ascii_case("Edit") || class.to_ascii_lowercase().starts_with("richedit")
}

/// Types `text` as VK_PACKET events, one character at a time. Returns whether
/// every key was sent.
///
/// Windows takes one UTF-16 code unit per key press, so a character outside
/// the Basic Multilingual Plane needs two. Both halves of such a surrogate
/// pair go out in one `SendInput` call, which no other input can split.
pub(crate) fn type_text(text: &str) -> bool {
    let mut buf = [0u16; 2];
    paced(text.chars().map(|c| {
        let mut inputs = Vec::new();
        for &unit in c.encode_utf16(&mut buf).iter() {
            push_key(&mut inputs, VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE);
        }
        inputs
    }))
}

/// Presses `vk` `count` times. Returns whether every press was sent.
///
/// Arrow keys and Delete need KEYEVENTF_EXTENDEDKEY, or they are read as
/// their numeric keypad counterparts.
pub(crate) fn tap_key(vk: Vk, count: usize) -> bool {
    let flags = if vk == VK_LEFT || vk == VK_RIGHT || vk == VK_DELETE {
        KEYEVENTF_EXTENDEDKEY
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    paced((0..count).map(|_| {
        let mut inputs = Vec::new();
        push_key(&mut inputs, vk, 0, flags);
        inputs
    }))
}

/// Sends each group of inputs on its own, [`KEY_PACE`] apart. Stops at the
/// first group that cannot be sent and returns `false`.
fn paced(groups: impl Iterator<Item = Vec<INPUT>>) -> bool {
    for (i, inputs) in groups.enumerate() {
        if i > 0 {
            std::thread::sleep(KEY_PACE);
        }
        if !send(&inputs) {
            return false;
        }
    }
    true
}

/// Appends a press and a release of `vk`, or of the UTF-16 code unit `scan`
/// when `flags` holds KEYEVENTF_UNICODE.
fn push_key(inputs: &mut Vec<INPUT>, vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) {
    for f in [flags, flags | KEYEVENTF_KEYUP] {
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: scan,
                    dwFlags: f,
                    time: 0,
                    dwExtraInfo: SELF_INPUT_TAG,
                },
            },
        });
    }
}

/// Sends `inputs` in one call, which Windows never interleaves with other
/// input. Returns whether all of them were inserted.
fn send(inputs: &[INPUT]) -> bool {
    if inputs.is_empty() {
        return true;
    }
    // SAFETY: every element is a fully initialized keyboard INPUT.
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    sent as usize == inputs.len()
}
