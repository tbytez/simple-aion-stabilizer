#![windows_subsystem = "windows"]

mod core;

#[cfg(windows)]
mod engine;
#[cfg(windows)]
mod watcher;

#[cfg(windows)]
mod app {
    use super::core;
    use super::engine::Engine;
    use super::watcher;
    use native_windows_gui as nwg;
    use std::cell::{Cell, RefCell};
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::Command;
    use std::rc::Rc;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

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
                .args([
                    "add", RUN_KEY, "/v", RUN_VALUE, "/t", "REG_SZ", "/d", &cmd, "/f",
                ])
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

    // ---- config.ini ----

    fn config_text(dir: &PathBuf) -> String {
        std::fs::read_to_string(dir.join("config.ini")).unwrap_or_default()
    }

    fn config_bool(dir: &PathBuf, key: &str, default: bool) -> bool {
        core::config_get_bool(&config_text(dir), key, default)
    }

    fn config_set(dir: &PathBuf, key: &str, value: &str) {
        let text = core::config_set(&config_text(dir), key, value);
        let _ = std::fs::write(dir.join("config.ini"), text);
    }

    fn watch_names(dir: &PathBuf) -> Vec<String> {
        core::config_get_list(&config_text(dir), "watch_process", core::DEFAULT_WATCH_PROCESSES)
    }

    // ---- single instance ----

    /// Returns Some(mutex_handle) to keep alive, or None if already running.
    fn try_single_instance() -> Option<winapi::shared::ntdef::HANDLE> {
        use winapi::shared::winerror::ERROR_ALREADY_EXISTS;
        use winapi::um::errhandlingapi::GetLastError;
        use winapi::um::synchapi::CreateMutexW;
        let name: Vec<u16> = "Local\\SimpleAionStabilizerMutex\0"
            .encode_utf16()
            .collect();
        unsafe {
            let h = CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr());
            if h.is_null() || GetLastError() == ERROR_ALREADY_EXISTS {
                None
            } else {
                Some(h)
            }
        }
    }

    pub fn run() {
        nwg::init().expect("Failed to init Native Windows GUI");
        nwg::Font::set_global_family("Segoe UI").expect("Failed to set default font");

        let _instance = match try_single_instance() {
            Some(h) => h,
            None => {
                nwg::simple_message(
                    "Aion Stabilizer",
                    "Already running - check the system tray.",
                );
                return;
            }
        };

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
            .size((400, 268))
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

        let mut toggle_btn = nwg::Button::default();
        nwg::Button::builder()
            .text("Start")
            .position((20, 88))
            .size((360, 45))
            .parent(window.handle)
            .build(&mut toggle_btn)
            .unwrap();

        let mut chk_startup = nwg::CheckBox::default();
        nwg::CheckBox::builder()
            .text("Run at Windows startup")
            .check_state(if startup_enabled() {
                nwg::CheckBoxState::Checked
            } else {
                nwg::CheckBoxState::Unchecked
            })
            .position((20, 146))
            .size((360, 22))
            .parent(window.handle)
            .build(&mut chk_startup)
            .unwrap();

        let mut chk_autostart_engine = nwg::CheckBox::default();
        nwg::CheckBox::builder()
            .text("Start engine when app opens")
            .check_state(if config_bool(&dir, "autostart_engine", false) {
                nwg::CheckBoxState::Checked
            } else {
                nwg::CheckBoxState::Unchecked
            })
            .position((20, 170))
            .size((360, 22))
            .parent(window.handle)
            .build(&mut chk_autostart_engine)
            .unwrap();

        let mut chk_watch = nwg::CheckBox::default();
        nwg::CheckBox::builder()
            .text("Start engine automatically when Aion 2 launches")
            .check_state(if config_bool(&dir, "watch_game", false) {
                nwg::CheckBoxState::Checked
            } else {
                nwg::CheckBoxState::Unchecked
            })
            .position((20, 194))
            .size((360, 22))
            .parent(window.handle)
            .build(&mut chk_watch)
            .unwrap();

        let mut hint = nwg::Label::default();
        nwg::Label::builder()
            .text("Minimizing or closing keeps the app in the system tray.")
            .position((20, 230))
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

        // ---- watcher thread + initial state ----
        watcher::init_names(watch_names(&dir));
        watcher::start();
        if config_bool(&dir, "watch_game", false) {
            watcher::WATCH_ENABLED.store(true, Ordering::Relaxed);
        }

        // ---- event handling ----
        let engine_events = engine.clone();
        let dir_events = dir.clone();
        let win_h = window.handle;
        // auto-start failure cooldown + one-shot error notification
        let auto_fail_until = Cell::new(Instant::now() - Duration::from_secs(1));
        let auto_err_shown = Cell::new(false);
        // a manual Stop suppresses watcher auto-start until the game exits
        let manual_stop = Cell::new(false);

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
                    if handle == toggle_btn.handle {
                        // borrows are statement-scoped so a modal shown below
                        // can never hit a live RefMut during its nested loop
                        let running = engine_events.borrow_mut().is_running();
                        let r = if running {
                            manual_stop.set(true);
                            engine_events.borrow_mut().stop();
                            Ok(())
                        } else {
                            manual_stop.set(false);
                            engine_events.borrow_mut().start(&dir_events, false)
                        };
                        if let Err(e) = r {
                            nwg::modal_error_message(win_h, "Aion Stabilizer", &e);
                        }
                    } else if handle == chk_startup.handle {
                        let checked = chk_startup.check_state() == nwg::CheckBoxState::Checked;
                        if let Err(e) = set_startup(checked) {
                            nwg::modal_error_message(win_h, "Aion Stabilizer", &e);
                        }
                    } else if handle == chk_autostart_engine.handle {
                        let checked =
                            chk_autostart_engine.check_state() == nwg::CheckBoxState::Checked;
                        config_set(&dir_events, "autostart_engine", if checked { "1" } else { "0" });
                    } else if handle == chk_watch.handle {
                        let checked = chk_watch.check_state() == nwg::CheckBoxState::Checked;
                        config_set(&dir_events, "watch_game", if checked { "1" } else { "0" });
                        if checked {
                            // reload names in case config.ini was edited
                            watcher::set_names(watch_names(&dir_events));
                        } else {
                            watcher::GAME_PRESENT.store(false, Ordering::Relaxed);
                        }
                        watcher::WATCH_ENABLED.store(checked, Ordering::Relaxed);
                    }
                }
                nwg::Event::OnMenuItemSelected => {
                    if handle == tray_open.handle {
                        window.set_visible(true);
                        window.restore();
                        window.set_focus();
                    } else if handle == tray_start.handle {
                        let r = {
                            let mut e = engine_events.borrow_mut();
                            if e.is_running() {
                                Ok(())
                            } else {
                                e.start(&dir_events, false)
                            }
                        };
                        if let Err(e) = r {
                            nwg::modal_error_message(win_h, "Aion Stabilizer", &e);
                        }
                    } else if handle == tray_stop.handle {
                        manual_stop.set(true);
                        engine_events.borrow_mut().stop();
                    } else if handle == tray_exit.handle {
                        engine_events.borrow_mut().stop();
                        nwg::stop_thread_dispatch();
                    }
                }
                nwg::Event::OnTimerTick => {
                    // 1) refresh engine state (borrow ends with the block)
                    let (mut running, exit_msg, auto_started) = {
                        let mut e = engine_events.borrow_mut();
                        (e.is_running(), e.last_exit.take(), e.auto_started)
                    };

                    // 2) watcher decisions
                    if watcher::WATCH_ENABLED.load(Ordering::Relaxed) {
                        let present = watcher::GAME_PRESENT.load(Ordering::Relaxed);
                        if !present {
                            manual_stop.set(false);
                        }
                        if present && !running && !manual_stop.get() {
                            if Instant::now() >= auto_fail_until.get() {
                                let r = engine_events.borrow_mut().start(&dir_events, true);
                                match r {
                                    Ok(()) => {
                                        running = true;
                                        auto_err_shown.set(false);
                                    }
                                    Err(e) => {
                                        auto_fail_until
                                            .set(Instant::now() + Duration::from_secs(60));
                                        if !auto_err_shown.get() {
                                            auto_err_shown.set(true);
                                            nwg::modal_error_message(
                                                win_h,
                                                "Aion Stabilizer",
                                                &format!("Auto-start failed: {e}"),
                                            );
                                        }
                                    }
                                }
                            }
                        } else if !present && running && auto_started {
                            engine_events.borrow_mut().stop();
                            running = false;
                            auto_err_shown.set(false);
                        }
                    } else {
                        auto_err_shown.set(false);
                    }

                    // 3) UI refresh
                    status_label.set_text(if running { "RUNNING" } else { "STOPPED" });
                    toggle_btn.set_text(if running { "Stop" } else { "Start" });

                    // 4) one-shot crash notification (borrow already dropped)
                    if let Some(msg) = exit_msg {
                        nwg::modal_error_message(
                            win_h,
                            "Aion Stabilizer",
                            &format!("Engine stopped unexpectedly ({msg})"),
                        );
                    }
                }
                _ => {}
            }
        });

        // autostart engine if requested
        if config_bool(&dir, "autostart_engine", false) {
            let r = engine.borrow_mut().start(&dir, false);
            if let Err(e) = r {
                nwg::modal_error_message(win_h, "Aion Stabilizer", &e);
            }
        }

        nwg::dispatch_thread_events();
        nwg::unbind_event_handler(&handler);
    }
}

#[cfg(windows)]
fn main() {
    app::run();
}

#[cfg(not(windows))]
fn main() {}
