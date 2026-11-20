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
    SetAutoUpdate(bool),
    CheckUpdate,
    ApplyUpdate,
    Rollback,
    DismissRollback,
    Shutdown,
}

#[derive(Clone)]
pub enum UpdateState {
    Available,
    Downloading(u8),
    Verifying,
    Installing,
    Failed(String),
}

impl UpdateState {
    /// True while an update is downloading or being applied - blocks
    /// engine (re)starts and re-entry into the updater.
    fn busy(&self) -> bool {
        matches!(
            self,
            UpdateState::Downloading(_) | UpdateState::Verifying | UpdateState::Installing
        )
    }
}

/// Feedback for the manual "check for updates" button.
#[derive(Clone, Default)]
pub enum CheckState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Failed,
}

#[derive(Clone)]
pub struct UpdateInfo {
    pub version: String,
    #[allow(dead_code)] // read only by the Windows updater
    pub zip_url: String,
    #[allow(dead_code)] // read only by the Windows updater
    pub manifest_url: String,
    pub state: UpdateState,
}

#[derive(Clone, Default)]
pub struct Status {
    pub running: bool,
    pub auto_started: bool,
    pub game_present: bool,
    pub watch: bool,
    pub startup_engine: bool,
    pub autostart: bool,
    pub auto_update: bool,
    pub error: Option<String>,
    pub update: Option<UpdateInfo>,
    pub check: CheckState,
    pub rollback_offer: bool,
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
    pub update_check: bool,
    pub auto_update: bool,
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
            update_check: core::config_get_bool(config_text, "update_check", true),
            auto_update: core::config_get_bool(config_text, "auto_update", false),
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

/// GitHub release check on a worker thread. Reports Checking/UpToDate/
/// Failed in `status.check`; a found release goes to `status.update`.
#[cfg(windows)]
fn check_for_update(status: Arc<Mutex<Status>>, wake: Arc<dyn Fn() + Send + Sync>) {
    if let Ok(mut s) = status.lock() {
        if matches!(s.check, CheckState::Checking) {
            return;
        }
        s.check = CheckState::Checking;
    }
    wake();
    std::thread::spawn(move || {
        let next = match crate::update::check_latest() {
            Ok(Some(rel)) => {
                if let Ok(mut s) = status.lock() {
                    // never clobber an in-progress download
                    let busy = s.update.as_ref().is_some_and(|u| u.state.busy());
                    if !busy {
                        s.update = Some(UpdateInfo {
                            version: rel.version,
                            zip_url: rel.zip_url,
                            manifest_url: rel.manifest_url,
                            state: UpdateState::Available,
                        });
                    }
                }
                CheckState::Idle
            }
            Ok(None) => CheckState::UpToDate,
            Err(_) => CheckState::Failed,
        };
        if let Ok(mut s) = status.lock() {
            s.check = next;
        }
        wake();
    });
}

/// Preview build: no real updater - simulate a quick check.
/// AION_FAKE_CHECK=failed previews the failure state.
#[cfg(not(windows))]
fn check_for_update(status: Arc<Mutex<Status>>, wake: Arc<dyn Fn() + Send + Sync>) {
    if let Ok(mut s) = status.lock() {
        s.check = CheckState::Checking;
    }
    wake();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(600));
        if let Ok(mut s) = status.lock() {
            s.check = if std::env::var("AION_FAKE_CHECK").as_deref() == Ok("failed") {
                CheckState::Failed
            } else {
                CheckState::UpToDate
            };
        }
        wake();
    });
}

/// Stop the engine (it holds file locks) and hand the update to a worker.
#[cfg(windows)]
fn apply_update_now(
    app_dir: PathBuf,
    engine: &mut Engine,
    status: Arc<Mutex<Status>>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let info = status
        .lock()
        .ok()
        .and_then(|s| s.update.clone())
        .filter(|i| !i.state.busy());
    let Some(info) = info else { return };
    engine.stop();
    if let Ok(mut s) = status.lock() {
        s.running = false;
        s.auto_started = false;
        if let Some(u) = &mut s.update {
            u.state = UpdateState::Downloading(0);
        }
    }
    wake();
    std::thread::spawn(move || apply_update(app_dir, info, status, wake));
}

#[cfg(not(windows))]
fn apply_update_now(
    _app_dir: PathBuf,
    _engine: &mut Engine,
    _status: Arc<Mutex<Status>>,
    _wake: Arc<dyn Fn() + Send + Sync>,
) {
}

/// Stop the engine and hand the restore to a headless copy of this exe
/// (`--finish-rollback`), then exit so the file swap can proceed. The
/// previous version parked as `*.exe.old` goes back into place.
#[cfg(windows)]
fn rollback_now(engine: &mut Engine, status: &Arc<Mutex<Status>>, wake: &Arc<dyn Fn() + Send + Sync>) {
    use std::process::Command;
    engine.stop();
    if let Ok(mut s) = status.lock() {
        s.running = false;
        s.auto_started = false;
        s.rollback_offer = false;
    }
    wake();
    let spawned = std::env::current_exe()
        .map_err(|e| e.to_string())
        .and_then(|exe| {
            Command::new(exe)
                .arg("--finish-rollback")
                .spawn()
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
    match spawned {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            if let Ok(mut s) = status.lock() {
                s.error = Some(format!("cannot start rollback: {e}"));
            }
            wake();
        }
    }
}

#[cfg(not(windows))]
fn rollback_now(_engine: &mut Engine, _status: &Arc<Mutex<Status>>, _wake: &Arc<dyn Fn() + Send + Sync>) {
}

/// Worker: private stage -> download -> verify manifest -> extract ->
/// verify files -> swap -> relaunch. Exits the process on success; on
/// failure reports UpdateState::Failed (retry keeps the release info).
#[cfg(windows)]
fn apply_update(
    app_dir: PathBuf,
    info: UpdateInfo,
    status: Arc<Mutex<Status>>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let mut on_progress = {
        let status = status.clone();
        let wake = wake.clone();
        move |p: crate::update::Progress| {
            let state = match p {
                crate::update::Progress::Download(pct) => UpdateState::Downloading(pct),
                crate::update::Progress::Verify => UpdateState::Verifying,
                crate::update::Progress::Install => UpdateState::Installing,
            };
            if let Ok(mut s) = status.lock() {
                if let Some(u) = &mut s.update {
                    u.state = state;
                }
            }
            wake();
        }
    };

    let rel = crate::core::ReleaseInfo {
        version: info.version.clone(),
        zip_url: info.zip_url.clone(),
        manifest_url: info.manifest_url.clone(),
    };
    let result = crate::update::run_update(&app_dir, &rel, &mut on_progress);

    match result {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            if let Ok(mut s) = status.lock() {
                if let Some(u) = &mut s.update {
                    u.state = UpdateState::Failed(e);
                }
            }
            wake();
        }
    }
}

impl Controller {
    /// Spawn the controller thread. `wake` is called after status changes.
    /// `rollback_offer` marks the first launch after a successful update,
    /// while the previous exe is still parked as `*.exe.old`.
    pub fn new(
        app_dir: PathBuf,
        cfg: Config,
        rollback_offer: bool,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (tx, rx) = channel::<Cmd>();
        let status = Arc::new(Mutex::new(Status {
            watch: cfg.watch_game,
            startup_engine: cfg.autostart_engine,
            autostart: autostart_enabled(),
            auto_update: cfg.auto_update,
            rollback_offer,
            ..Status::default()
        }));
        let st = status.clone();
        std::thread::spawn(move || run(rx, st, app_dir, cfg, wake));
        Self { tx, status }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    #[cfg(test)]
    pub fn for_ui_test_with_commands() -> (Self, Receiver<Cmd>) {
        let (tx, rx) = channel();
        (
            Self {
                tx,
                status: Arc::new(Mutex::new(Status::default())),
            },
            rx,
        )
    }

    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

/// Read current update state at each decision, including after a command
/// changes it. A loop-entry snapshot is stale after ApplyUpdate stops the
/// engine and marks the download busy.
fn update_in_progress(status: &Mutex<Status>) -> bool {
    status
        .lock()
        .map(|s| s.update.as_ref().is_some_and(|u| u.state.busy()))
        .unwrap_or(true)
}

/// True when auto-update should start applying: the feature is on, a
/// release is waiting in Available state and the engine is off so the
/// install (which stops the engine and restarts the app) never
/// interrupts a game session. Anything else - checking, downloading,
/// failed, feature off - returns false.
fn auto_update_ready(status: &Mutex<Status>, engine_running: bool) -> bool {
    if engine_running {
        return false;
    }
    status
        .lock()
        .map(|s| {
            s.auto_update
                && s.update
                    .as_ref()
                    .is_some_and(|u| matches!(u.state, UpdateState::Available))
        })
        .unwrap_or(false)
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
    let mut last_poll = Instant::now();

    // refresh the autostart entry with the current exe path - heals it
    // after an update renamed or replaced the binary
    if autostart_enabled() {
        autostart_set(true);
    }

    // one release check at startup (config.ini: update_check=0 disables)
    if cfg.update_check {
        check_for_update(status.clone(), wake.clone());
    }

    macro_rules! set_status {
        ($f:ident, $v:expr) => {{
            if let Ok(mut s) = status.lock() {
                s.$f = $v;
            }
            (wake)();
        }};
    }

    // preview: AION_FAKE_UPDATE=1|working|failed shows the update banner
    #[cfg(not(windows))]
    if let Ok(fake) = std::env::var("AION_FAKE_UPDATE") {
        let state = match fake.as_str() {
            "working" | "downloading" => UpdateState::Downloading(42),
            "verifying" => UpdateState::Verifying,
            "installing" => UpdateState::Installing,
            "failed" | "fail" => UpdateState::Failed("mock failure".into()),
            _ => UpdateState::Available,
        };
        set_status!(
            update,
            Some(UpdateInfo {
                version: "9.9.9".into(),
                zip_url: String::new(),
                manifest_url: String::new(),
                state,
            })
        );
    }

    loop {
        // while an update downloads/applies, block engine (re)starts - it
        // would re-lock the winws/WinDivert files the swap needs to write
        let updating = update_in_progress(&status);

        // auto-update: hand a waiting release to the verified install
        // path once the engine is off. The manual Install button
        // (Cmd::ApplyUpdate) still applies immediately regardless.
        if !updating && auto_update_ready(&status, engine.is_running()) {
            apply_update_now(app_dir.clone(), &mut engine, status.clone(), wake.clone());
        }

        match rx.recv_timeout(Duration::from_millis(400)) {
            Ok(Cmd::Shutdown) => {
                engine.stop();
                set_status!(running, false);
                break;
            }
            Ok(Cmd::Toggle) | Ok(Cmd::Start { .. }) if updating => {}
            Ok(Cmd::Toggle) => {
                if engine.is_running() {
                    engine.stop();
                    manual_latch = true;
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
                if manual {
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
            Ok(Cmd::SetAutoUpdate(on)) => {
                set_status!(auto_update, on);
                save_config(&app_dir, "auto_update", if on { "1" } else { "0" });
                // enabling applies now, not only at next launch
                if on {
                    check_for_update(status.clone(), wake.clone());
                }
            }
            Ok(Cmd::CheckUpdate) => {
                check_for_update(status.clone(), wake.clone());
            }
            Ok(Cmd::ApplyUpdate) => {
                apply_update_now(app_dir.clone(), &mut engine, status.clone(), wake.clone());
            }
            Ok(Cmd::Rollback) => {
                rollback_now(&mut engine, &status, &wake);
            }
            Ok(Cmd::DismissRollback) => {
                // One-time offer: delete the parked previous exe so the
                // banner is gone for good (startup cleanup would anyway).
                let mut names = vec![format!("{}.old", crate::manifest::APP_EXE)];
                if let Some(stem) = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                {
                    names.push(format!("{stem}.exe.old"));
                }
                for n in names {
                    let _ = std::fs::remove_file(app_dir.join(n));
                }
                set_status!(rollback_offer, false);
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
        let watch_on = status.lock().map(|s| s.watch).unwrap_or(false);
        if last_poll.elapsed() >= Duration::from_secs(2) {
            last_poll = Instant::now();
            if watch_on {
                let now = procs::any_running(&cfg.watch_names);
                // A manual Stop must also hold before the first process poll.
                // Only a confirmed absence clears it, never stale cached state.
                if !now {
                    manual_latch = false;
                }
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
            && !update_in_progress(&status)
            && last_auto_fail
                .map(|t| t.elapsed() > AUTO_RETRY_COOLDOWN)
                .unwrap_or(true)
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

#[cfg(test)]
mod update_watcher_tests {
    use super::*;

    #[test]
    fn update_started_during_command_handling_blocks_watcher_restart() {
        let status = Mutex::new(Status::default());
        assert!(!update_in_progress(&status)); // loop-entry state
                                               // ApplyUpdate stops the engine during command dispatch. The watcher
                                               // must see the new state before considering an automatic restart.
        for state in [
            UpdateState::Downloading(0),
            UpdateState::Downloading(100),
            UpdateState::Verifying,
            UpdateState::Installing,
        ] {
            status.lock().unwrap().update = Some(UpdateInfo {
                version: "9.0.0".into(),
                zip_url: String::new(),
                manifest_url: String::new(),
                state,
            });
            assert!(update_in_progress(&status));
        }
        status.lock().unwrap().update.as_mut().unwrap().state =
            UpdateState::Failed("download failed".into());
        assert!(!update_in_progress(&status));
    }

    #[test]
    fn auto_update_waits_for_available_release_and_idle_engine() {
        let status = Mutex::new(Status::default());
        // no release found yet
        assert!(!auto_update_ready(&status, false));
        status.lock().unwrap().update = Some(UpdateInfo {
            version: "9.9.9".into(),
            zip_url: String::new(),
            manifest_url: String::new(),
            state: UpdateState::Available,
        });
        // feature off / engine busy -> hold
        assert!(!auto_update_ready(&status, false));
        assert!(!auto_update_ready(&status, true));
        status.lock().unwrap().auto_update = true;
        assert!(!auto_update_ready(&status, true));
        // feature on, release waiting, engine off -> apply
        assert!(auto_update_ready(&status, false));
        // once applying or after a failure the gate stays closed
        for state in [
            UpdateState::Downloading(0),
            UpdateState::Verifying,
            UpdateState::Installing,
            UpdateState::Failed("download failed".into()),
        ] {
            status.lock().unwrap().update.as_mut().unwrap().state = state;
            assert!(!auto_update_ready(&status, false));
        }
    }
}
