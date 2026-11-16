#![windows_subsystem = "windows"]

mod engine;

use engine::Engine;
use native_windows_gui as nwg;
use std::cell::RefCell;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "AionStabilizer";

fn app_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn startup_enabled() -> bool {
    Command::new("reg")
        .args(["query", RUN_KEY, "/v", RUN_VALUE])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn set_startup(enabled: bool) -> Result<(), String> {
    let status = if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let cmd = format!("\"{}\" --tray", exe.display());
        Command::new("reg")
            .args(["add", RUN_KEY, "/v", RUN_VALUE, "/t", "REG_SZ", "/d", &cmd, "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| e.to_string())?
    } else {
        Command::new("reg")
            .args(["delete", RUN_KEY, "/v", RUN_VALUE, "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| e.to_string())?
    };
    if status.success() || !enabled {
        Ok(())
    } else {
        Err("Failed to update the startup registry key".to_string())
    }
}

fn read_autostart_engine(dir: &PathBuf) -> bool {
    std::fs::read_to_string(dir.join("config.ini"))
        .map(|t| t.lines().any(|l| l.trim() == "autostart_engine=1"))
        .unwrap_or(false)
}

fn write_autostart_engine(dir: &PathBuf, on: bool) {
    let _ = std::fs::write(
        dir.join("config.ini"),
        format!("autostart_engine={}\n", if on { 1 } else { 0 }),
    );
}

fn main() {
    nwg::init().expect("Failed to init Native Windows GUI");
    nwg::Font::set_global_family("Segoe UI").expect("Failed to set default font");

    let start_minimized = std::env::args().any(|a| a == "--tray" || a == "--minimized");
    let dir = app_dir();
    let engine = Rc::new(RefCell::new(Engine::new()));

    // ---- resources ----
    let embed = nwg::EmbedResource::load(None).expect("Failed to load embedded resources");
    let mut icon = nwg::Icon::default();
    nwg::Icon::builder()
        .source_embed(Some(&embed))
        .source_embed_id(1)
        .build(&mut icon)
        .expect("Failed to load icon");

    let mut status_font = nwg::Font::default();
    nwg::Font::builder()
        .size(24)
        .weight(700)
        .family("Segoe UI")
        .build(&mut status_font)
        .expect("Failed to create font");

    // ---- window ----
    let mut window = nwg::Window::default();
    let base_flags = nwg::WindowFlags::WINDOW | nwg::WindowFlags::MINIMIZE_BOX;
    nwg::Window::builder()
        .size((400, 240))
        .center(true)
        .title("Simple Aion Stabilizer")
        .icon(Some(&icon))
        .flags(if start_minimized {
            base_flags
        } else {
            base_flags | nwg::WindowFlags::VISIBLE
        })
        .build(&mut window)
        .expect("Failed to build window");

    // ---- controls ----
    let mut caption = nwg::Label::default();
    nwg::Label::builder()
        .text("Engine status:")
        .position((20, 16))
        .size((360, 20))
        .parent(window.handle)
        .build(&mut caption)
        .unwrap();

    let mut status_label = nwg::Label::default();
    nwg::Label::builder()
        .text("STOPPED")
        .font(Some(&status_font))
        .position((20, 36))
        .size((360, 42))
        .parent(window.handle)
        .build(&mut status_label)
        .unwrap();

    let mut start_btn = nwg::Button::default();
    nwg::Button::builder()
        .text("Start")
        .position((20, 88))
        .size((175, 45))
        .parent(window.handle)
        .build(&mut start_btn)
        .unwrap();

    let mut stop_btn = nwg::Button::default();
    nwg::Button::builder()
        .text("Stop")
        .enabled(false)
        .position((205, 88))
        .size((175, 45))
        .parent(window.handle)
        .build(&mut stop_btn)
        .unwrap();

    let mut chk_startup = nwg::CheckBox::default();
    nwg::CheckBox::builder()
        .text("Run at Windows startup")
        .check_state(if startup_enabled() {
            nwg::CheckBoxState::Checked
        } else {
            nwg::CheckBoxState::Unchecked
        })
        .position((20, 148))
        .size((360, 22))
        .parent(window.handle)
        .build(&mut chk_startup)
        .unwrap();

    let mut chk_autostart_engine = nwg::CheckBox::default();
    nwg::CheckBox::builder()
        .text("Start engine when app opens")
        .check_state(if read_autostart_engine(&dir) {
            nwg::CheckBoxState::Checked
        } else {
            nwg::CheckBoxState::Unchecked
        })
        .position((20, 172))
        .size((360, 22))
        .parent(window.handle)
        .build(&mut chk_autostart_engine)
        .unwrap();

    let mut hint = nwg::Label::default();
    nwg::Label::builder()
        .text("Minimizing or closing keeps the app in the system tray.")
        .position((20, 202))
        .size((360, 18))
        .parent(window.handle)
        .build(&mut hint)
        .unwrap();

    // ---- tray ----
    let mut tray = nwg::TrayNotification::default();
    nwg::TrayNotification::builder()
        .parent(window.handle)
        .icon(Some(&icon))
        .tip(Some("Simple Aion Stabilizer"))
        .build(&mut tray)
        .expect("Failed to create tray icon");

    let mut tray_menu = nwg::Menu::default();
    nwg::Menu::builder()
        .popup(true)
        .parent(window.handle)
        .build(&mut tray_menu)
        .unwrap();

    let mut tray_open = nwg::MenuItem::default();
    nwg::MenuItem::builder()
        .text("Open")
        .parent(tray_menu.handle)
        .build(&mut tray_open)
        .unwrap();

    let mut tray_start = nwg::MenuItem::default();
    nwg::MenuItem::builder()
        .text("Start engine")
        .parent(tray_menu.handle)
        .build(&mut tray_start)
        .unwrap();

    let mut tray_stop = nwg::MenuItem::default();
    nwg::MenuItem::builder()
        .text("Stop engine")
        .parent(tray_menu.handle)
        .build(&mut tray_stop)
        .unwrap();

    let mut tray_sep = nwg::MenuSeparator::default();
    nwg::MenuSeparator::builder()
        .parent(tray_menu.handle)
        .build(&mut tray_sep)
        .unwrap();

    let mut tray_exit = nwg::MenuItem::default();
    nwg::MenuItem::builder()
        .text("Exit")
        .parent(tray_menu.handle)
        .build(&mut tray_exit)
        .unwrap();

    // 1s timer to watch the engine process
    let mut timer = nwg::AnimationTimer::default();
    nwg::AnimationTimer::builder()
        .interval(Duration::from_secs(1))
        .active(true)
        .parent(window.handle)
        .build(&mut timer)
        .unwrap();

    // ---- event handling ----
    let engine_events = engine.clone();
    let dir_events = dir.clone();
    let win_h = window.handle;

    let handler = nwg::full_bind_event_handler(&win_h, move |evt, evt_data, handle| {
        match evt {
            nwg::Event::OnWindowClose => {
                if let nwg::EventData::OnWindowClose(data) = evt_data {
                    // Hide to tray instead of exiting
                    data.close(false);
                    window.set_visible(false);
                }
            }
            nwg::Event::OnWindowMinimize => {
                window.set_visible(false);
            }
            nwg::Event::OnContextMenu => {
                if handle == tray.handle {
                    let (x, y) = nwg::GlobalCursor::position();
                    tray_menu.popup(x, y);
                }
            }
            nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) => {
                if handle == tray.handle {
                    window.set_visible(true);
                    window.restore();
                    window.set_focus();
                }
            }
            nwg::Event::OnButtonClick => {
                if handle == start_btn.handle {
                    if let Err(e) = engine_events.borrow_mut().start(&dir_events) {
                        nwg::modal_error_message(win_h, "Aion Stabilizer", &e);
                    }
                } else if handle == stop_btn.handle {
                    engine_events.borrow_mut().stop();
                } else if handle == chk_startup.handle {
                    let checked = chk_startup.check_state() == nwg::CheckBoxState::Checked;
                    if let Err(e) = set_startup(checked) {
                        nwg::modal_error_message(win_h, "Aion Stabilizer", &e);
                    }
                } else if handle == chk_autostart_engine.handle {
                    let checked =
                        chk_autostart_engine.check_state() == nwg::CheckBoxState::Checked;
                    write_autostart_engine(&dir_events, checked);
                }
            }
            nwg::Event::OnMenuItemSelected => {
                if handle == tray_open.handle {
                    window.set_visible(true);
                    window.restore();
                    window.set_focus();
                } else if handle == tray_start.handle {
                    if let Err(e) = engine_events.borrow_mut().start(&dir_events) {
                        nwg::modal_error_message(win_h, "Aion Stabilizer", &e);
                    }
                } else if handle == tray_stop.handle {
                    engine_events.borrow_mut().stop();
                } else if handle == tray_exit.handle {
                    engine_events.borrow_mut().stop();
                    nwg::stop_thread_dispatch();
                }
            }
            nwg::Event::OnTimerTick => {
                let running = engine_events.borrow_mut().is_running();
                status_label.set_text(if running { "RUNNING" } else { "STOPPED" });
                start_btn.set_enabled(!running);
                stop_btn.set_enabled(running);
            }
            _ => {}
        }
    });

    // autostart engine if requested
    if read_autostart_engine(&dir) {
        if let Err(e) = engine.borrow_mut().start(&dir) {
            nwg::modal_error_message(win_h, "Aion Stabilizer", &e);
        }
    }

    nwg::dispatch_thread_events();
    nwg::unbind_event_handler(&handler);
}
