use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Enabled flag toggled by the UI checkbox.
pub static WATCH_ENABLED: AtomicBool = AtomicBool::new(false);
/// Set by the watcher thread: is a watched game process currently running?
pub static GAME_PRESENT: AtomicBool = AtomicBool::new(false);
/// Watch list, mutable so config.ini edits apply without restart.
pub static WATCH_NAMES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

pub fn init_names(names: Vec<String>) {
    let _ = WATCH_NAMES.set(Mutex::new(names));
}

pub fn set_names(names: Vec<String>) {
    if let Some(m) = WATCH_NAMES.get() {
        if let Ok(mut g) = m.lock() {
            *g = names;
        }
    }
}

/// Spawn the background watcher once. It polls `tasklist` every 2s while
/// watching is enabled; when disabled it idles without spawning processes.
pub fn start() {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    std::thread::spawn(|| loop {
        if !WATCH_ENABLED.load(Ordering::Relaxed) {
            GAME_PRESENT.store(false, Ordering::Relaxed);
            std::thread::sleep(Duration::from_secs(2));
            continue;
        }
        let names = WATCH_NAMES
            .get()
            .and_then(|m| m.lock().ok().map(|g| g.clone()))
            .unwrap_or_default();
        match tasklist_has(&names) {
            Some(p) => GAME_PRESENT.store(p, Ordering::Relaxed),
            None => {} // tasklist failed: keep last known state, don't flap
        }
        std::thread::sleep(Duration::from_secs(2));
    });
}

fn tasklist_has(names: &[String]) -> Option<bool> {
    let out = Command::new("tasklist")
        .args(["/FO", "CSV"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Some(crate::core::process_in_tasklist(&text, names))
}
