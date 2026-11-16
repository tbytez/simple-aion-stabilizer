//! System tray icon on a dedicated thread with its own Win32 message pump,
//! so it keeps working while the main window is hidden (when egui does no
//! update passes at all).

#![cfg(windows)]

use crate::controller::{Cmd, Controller};
use crate::sys;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub static QUIT: AtomicBool = AtomicBool::new(false);

const ICON_ON: &[u8] = include_bytes!("../assets/tray_on.rgba");
const ICON_OFF: &[u8] = include_bytes!("../assets/tray_off.rgba");
const ICON_SIZE: u32 = 64;

fn icon(rgba: &[u8]) -> Icon {
    Icon::from_rgba(rgba.to_vec(), ICON_SIZE, ICON_SIZE).expect("tray icon")
}

/// Spawn the tray thread. It owns the TrayIcon (must be dropped there so the
/// icon disappears from the tray on exit).
pub fn spawn(ctrl: Controller) {
    std::thread::spawn(move || run(ctrl));
}

fn run(ctrl: Controller) {
    let open = MenuItem::new("Open", true, None);
    let start = MenuItem::new("Start engine", true, None);
    let stop = MenuItem::new("Stop engine", false, None);
    let exit = MenuItem::new("Exit", true, None);
    let menu = Menu::new();
    let _ = menu.append_items(&[
        &open,
        &PredefinedMenuItem::separator(),
        &start,
        &stop,
        &PredefinedMenuItem::separator(),
        &exit,
    ]);

    let tray = TrayIconBuilder::new()
        .with_id(tray_icon::TrayIconId::new("aion-stabilizer"))
        .with_menu(Box::new(menu))
        .with_tooltip("Simple Aion Stabilizer")
        .with_icon(icon(ICON_OFF))
        .with_menu_on_left_click(false)
        .build();

    let Ok(tray) = tray else {
        return; // no shell / no tray - keep the app usable
    };

    let mut last_on = false;

    loop {
        // pump this thread's message queue (required by the tray icon window)
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // tray icon clicks
        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = ev
            {
                sys::show_main_window();
            }
        }

        // menu actions
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if ev.id == open.id() {
                sys::show_main_window();
            } else if ev.id == start.id() {
                ctrl.send(Cmd::Start { auto: false });
            } else if ev.id == stop.id() {
                ctrl.send(Cmd::Stop { manual: true });
            } else if ev.id == exit.id() {
                ctrl.send(Cmd::Shutdown);
                QUIT.store(true, Ordering::SeqCst);
                // give the controller a moment to kill winws before we die
                std::thread::sleep(Duration::from_millis(800));
                drop(tray);
                std::process::exit(0);
            }
        }

        // reflect engine state on the icon + menu
        let st = ctrl.status();
        if st.running != last_on {
            last_on = st.running;
            let _ = tray.set_icon(Some(icon(if st.running { ICON_ON } else { ICON_OFF })));
            let _ = tray.set_tooltip(Some(&format!(
                "Simple Aion Stabilizer - {}",
                if st.running { "engine ON" } else { "engine off" }
            )));
            start.set_enabled(!st.running);
            stop.set_enabled(st.running);
        }

        // sleep until a window message arrives or 500ms pass
        unsafe {
            MsgWaitForMultipleObjects(0, ptr::null(), 0, 500, QS_ALLINPUT);
        }
    }
}
