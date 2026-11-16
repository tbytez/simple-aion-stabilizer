//! Background controller thread: owns the engine, polls game processes and
//! applies settings. The UI never blocks on child processes or the registry.

use crate::core;
use crate::engine::Engine;
use crate::procs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[allow(dead_code)] // some variants are only produced by the Windows tray
pub enum Cmd {
    Toggle,
    Start { auto: bool },
    Stop { manual: bool },
    SetWatch(bool),
    SetStartupEngine(bool),
    SetAutostart(bool),
    Shutdown,
}

#[derive(Clone, Default)]
pub struct Status {
    pub running: bool,
    pub auto_started: bool,
    pub game_present: bool,
    pub watch: bool,
    pub startup_engine: bool,
    pub autostart: bool,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct Controller {
    tx: Sender<Cmd>,
    status: Arc<Mutex<Status>>,
}

pub struct Config {
    pub autostart_engine: bool,
    pub watch_game: bool,
    pub watch_names: Vec<String>,
}

impl Config {
    /// Load settings from config.ini text + registry.
    pub fn load(config_text: &str) -> Self {
        Self {
            autostart_engine: core::config_get_bool(config_text, "autostart_engine", false),
            watch_game: core::config_get_bool(config_text, "watch_game", true),
            watch_names: core::config_get_list(
                config_text,
                "watch_process",
                core::DEFAULT_WATCH_PROCESSES,
            ),
        }
    }
}

fn autostart_enabled() -> bool {
    #[cfg(windows)]
    {
        crate::sys::autostart_enabled()
    }
    #[cfg(not(windows))]
    {
        std::env::var("AION_FAKE_AUTOSTART").is_ok()
    }
}

fn autostart_set(on: bool) {
    #[cfg(windows)]
    crate::sys::autostart_set(on);
    #[cfg(not(windows))]
    let _ = on;
}

fn save_config(app_dir: &Path, key: &str, value: &str) {
    let path = app_dir.join("config.ini");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let _ = std::fs::write(&path, core::config_set(&text, key, value));
}

impl Controller {
    /// Spawn the controller thread. `wake` is called after status changes.
    pub fn new(
        app_dir: PathBuf,
        cfg: Config,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (tx, rx) = channel::<Cmd>();
        let status = Arc::new(Mutex::new(Status {
            watch: cfg.watch_game,
            startup_engine: cfg.autostart_engine,
            autostart: autostart_enabled(),
            ..Status::default()
        }));
        let st = status.clone();
        std::thread::spawn(move || run(rx, st, app_dir, cfg, wake));
        Self { tx, status }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

const AUTO_RETRY_COOLDOWN: Duration = Duration::from_secs(60);

fn run(
    rx: Receiver<Cmd>,
    status: Arc<Mutex<Status>>,
    app_dir: PathBuf,
    cfg: Config,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let mut engine = Engine::new();
    let mut game_present = false;
    // User pressed Stop while the game was up - do not auto-restart
    // until the game exits.
    let mut manual_latch = false;
    let mut last_auto_fail: Option<Instant> = None;
    let mut poll_tick = 0u32;

    macro_rules! set_status {
        ($f:ident, $v:expr) => {{
            if let Ok(mut s) = status.lock() {
                s.$f = $v;
            }
            (wake)();
        }};
    }

    loop {
        match rx.recv_timeout(Duration::from_millis(400)) {
            Ok(Cmd::Shutdown) => {
                engine.stop();
                set_status!(running, false);
                break;
            }
            Ok(Cmd::Toggle) => {
                if engine.is_running() {
                    engine.stop();
                    if game_present {
                        manual_latch = true;
                    }
                    set_status!(running, false);
                    set_status!(auto_started, false);
                } else {
                    manual_latch = false;
                    match engine.start(&app_dir, false) {
                        Ok(()) => {
                            set_status!(running, true);
                            set_status!(auto_started, false);
                            set_status!(error, None);
                        }
                        Err(e) => set_status!(error, Some(e)),
                    }
                }
            }
            Ok(Cmd::Start { auto }) => {
                if !engine.is_running() {
                    if !auto {
                        manual_latch = false;
                    }
                    match engine.start(&app_dir, auto) {
                        Ok(()) => {
                            set_status!(running, true);
                            set_status!(auto_started, auto);
                            set_status!(error, None);
                        }
                        Err(e) => {
                            if auto {
                                last_auto_fail = Some(Instant::now());
                            }
                            set_status!(error, Some(e));
                        }
                    }
                }
            }
            Ok(Cmd::Stop { manual }) => {
                engine.stop();
                if manual && game_present {
                    manual_latch = true;
                }
                set_status!(running, false);
                set_status!(auto_started, false);
            }
            Ok(Cmd::SetWatch(on)) => {
                set_status!(watch, on);
                save_config(&app_dir, "watch_game", if on { "1" } else { "0" });
                if !on && engine.auto_started && engine.is_running() {
                    engine.stop();
                    set_status!(running, false);
                    set_status!(auto_started, false);
                }
            }
            Ok(Cmd::SetStartupEngine(on)) => {
                set_status!(startup_engine, on);
                save_config(&app_dir, "autostart_engine", if on { "1" } else { "0" });
            }
            Ok(Cmd::SetAutostart(on)) => {
                autostart_set(on);
                set_status!(autostart, autostart_enabled());
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                engine.stop();
                break;
            }
        }

        // engine died on its own?
        if !engine.is_running() {
            if let Some(exit) = engine.last_exit.take() {
                set_status!(running, false);
                set_status!(auto_started, false);
                set_status!(error, Some(format!("Engine stopped unexpectedly ({exit})")));
            }
        }

        // game presence poll (every ~2s)
        poll_tick += 1;
        let watch_on = status.lock().map(|s| s.watch).unwrap_or(false);
        if poll_tick >= 5 {
            poll_tick = 0;
            if watch_on {
                let now = procs::any_running(&cfg.watch_names);
                if now != game_present {
                    game_present = now;
                    set_status!(game_present, now);
                    if !game_present {
                        // game exited - reset manual latch; stop auto engine
                        manual_latch = false;
                        if engine.auto_started && engine.is_running() {
                            engine.stop();
                            set_status!(running, false);
                            set_status!(auto_started, false);
                        }
                    }
                }
            } else if game_present {
                game_present = false;
                set_status!(game_present, false);
            }
        }

        // auto-start when the game is up
        if watch_on
            && game_present
            && !manual_latch
            && !engine.is_running()
            && last_auto_fail.map(|t| t.elapsed() > AUTO_RETRY_COOLDOWN).unwrap_or(true)
        {
            match engine.start(&app_dir, true) {
                Ok(()) => {
                    set_status!(running, true);
                    set_status!(auto_started, true);
                    set_status!(error, None);
                }
                Err(e) => {
                    last_auto_fail = Some(Instant::now());
                    set_status!(error, Some(e));
                }
            }
        }
    }
}
