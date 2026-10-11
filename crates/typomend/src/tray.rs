// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The notification area icon and its menu.
//!
//! tray-icon delivers menu and icon events from the window procedure of a
//! hidden window owned by the thread that created the icon, which is the UI
//! thread. The icon itself is not `Send`, so it lives in a thread-local and the
//! event handlers, which must be `Send`, reach it from there.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::state::AppState;
use crate::{fl, icon, platform};

/// The tray icon and the menu items whose text or check mark change.
struct Tray {
    icon: TrayIcon,
    /// Whether the icon shows corrections as active, for redrawing it when
    /// the taskbar theme changes.
    active: Cell<bool>,
    open: MenuItem,
    pause: MenuItem,
    private: CheckMenuItem,
    autostart: CheckMenuItem,
    quit: MenuItem,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

/// Creates the tray icon and its menu, and installs the event handlers.
///
/// Must be called on the UI thread. A left click on the icon opens the
/// settings window, and the menu opens on a right click.
///
/// # Errors
///
/// Returns an error when the menu cannot be built, the icon cannot be loaded
/// or the notification area refuses the icon.
pub fn create(state: Arc<AppState>) -> anyhow::Result<()> {
    let open = MenuItem::new(fl!("menu-open"), true, None);
    let pause = MenuItem::new(fl!("menu-pause"), true, None);
    let private = CheckMenuItem::new(fl!("menu-private"), true, state.config().private_mode, None);
    let autostart = CheckMenuItem::new(
        fl!("menu-autostart"),
        true,
        platform::autostart_enabled(),
        None,
    );
    let quit = MenuItem::new(fl!("menu-quit"), true, None);
    let menu = Menu::new();
    menu.append_items(&[
        &open,
        &pause,
        &private,
        &autostart,
        &PredefinedMenuItem::separator(),
        &quit,
    ])?;

    let icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip(fl!("tray-tooltip"))
        .with_icon(make_icon(true)?)
        .build()?;

    let ids = (
        open.id().clone(),
        pause.id().clone(),
        private.id().clone(),
        autostart.id().clone(),
        quit.id().clone(),
    );
    TRAY.with(|t| {
        *t.borrow_mut() = Some(Tray {
            icon,
            active: Cell::new(true),
            open,
            pause,
            private,
            autostart,
            quit,
        })
    });
    sync(&state);

    let menu_state = state.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let (open, pause, private, autostart, quit) = &ids;
        if event.id == *open {
            menu_state.show_window();
        } else if event.id == *pause {
            menu_state.update_config(|c| c.enabled = !c.enabled);
            sync(&menu_state);
        } else if event.id == *private {
            menu_state.update_config(|c| c.private_mode = !c.private_mode);
            sync(&menu_state);
        } else if event.id == *autostart {
            if let Err(e) = platform::set_autostart(!platform::autostart_enabled()) {
                tracing::error!("changing autostart failed: {e}");
            }
            sync(&menu_state);
            crate::app::post(crate::app::WM_APP_REFRESH);
        } else if event.id == *quit {
            quit_program();
        }
    }));

    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            state.show_window();
        }
    }));
    Ok(())
}

/// Brings the menu texts, check marks and icon in line with the current
/// configuration and language. Must be called on the UI thread.
pub fn sync(state: &AppState) {
    let config = state.config();
    let enabled = config.enabled;
    TRAY.with(|t| {
        let tray = t.borrow();
        let Some(tray) = tray.as_ref() else { return };
        tray.open.set_text(fl!("menu-open"));
        tray.pause.set_text(if enabled {
            fl!("menu-pause")
        } else {
            fl!("menu-resume")
        });
        tray.private.set_text(fl!("menu-private"));
        tray.private.set_checked(config.private_mode);
        tray.autostart.set_text(fl!("menu-autostart"));
        tray.autostart.set_checked(platform::autostart_enabled());
        tray.quit.set_text(fl!("menu-quit"));
        let tooltip = if !enabled {
            fl!("tray-tooltip-paused")
        } else if config.private_mode {
            fl!("tray-tooltip-private")
        } else {
            fl!("tray-tooltip")
        };
        let _ = tray.icon.set_tooltip(Some(tooltip));
        tray.active.set(enabled);
        set_icon(tray);
    });
}

/// Redraws the icon for the current taskbar theme. Must be called on the UI
/// thread.
pub fn retheme() {
    TRAY.with(|t| {
        if let Some(tray) = t.borrow().as_ref() {
            set_icon(tray);
        }
    });
}

/// Removes the tray icon and exits. Dropping the icon first keeps a dead icon
/// from lingering in the notification area until the mouse passes over it.
pub fn quit_program() -> ! {
    TRAY.with(|t| t.borrow_mut().take());
    std::process::exit(0);
}

/// Replaces the icon with the one for the current state and theme, logging
/// a failure and keeping the old icon.
fn set_icon(tray: &Tray) {
    match make_icon(tray.active.get()) {
        Ok(icon) => {
            let _ = tray.icon.set_icon(Some(icon));
        }
        Err(e) => tracing::error!("loading the tray icon failed: {e}"),
    }
}

/// Loads the tray icon for the current taskbar theme from the executable's
/// resources, in the paused variant when `active` is false.
fn make_icon(active: bool) -> anyhow::Result<Icon> {
    let (id, size) = icon::tray(active);
    Ok(Icon::from_resource(id, Some((size, size)))?)
}
