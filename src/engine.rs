use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Manages the winws.exe (zapret) child process.
pub struct Engine {
    child: Option<Child>,
}

impl Engine {
    pub fn new() -> Self {
        Self { child: None }
    }

    /// True while the child we spawned is still alive.
    pub fn is_running(&mut self) -> bool {
        match self.child.as_mut().map(|c| c.try_wait()) {
            Some(Ok(None)) => true,
            Some(_) => {
                self.child = None;
                false
            }
            None => false,
        }
    }

    pub fn start(&mut self, app_dir: &Path) -> Result<(), String> {
        if self.is_running() {
            return Err("Engine is already running".to_string());
        }

        let winws = app_dir.join("bin").join("winws.exe");
        if !winws.is_file() {
            return Err(format!("winws.exe not found in {}", app_dir.join("bin").display()));
        }

        let args = load_profile(app_dir)?;
        if args.is_empty() {
            return Err("profile.txt contains no arguments".to_string());
        }

        // Some desync modes need TCP timestamps enabled system-wide (same thing
        // the zapret service scripts do). Harmless standard TCP option.
        let _ = Command::new("netsh")
            .args(["interface", "tcp", "set", "global", "timestamps=enabled"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();

        let child = Command::new(&winws)
            .args(&args)
            .current_dir(app_dir)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Failed to start winws.exe: {e}"))?;

        self.child = Some(child);
        Ok(())
    }

    /// Kill our child plus any stray winws.exe left over from earlier runs.
    pub fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        // belt & suspenders: sweep any stray winws.exe instances
        let _ = Command::new("taskkill")
            .args(["/F", "/IM", "winws.exe"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
}

/// Parse profile.txt into a winws argument vector.
///
/// Rules: `#` starts a comment line, tokens are separated by whitespace,
/// `{BIN}` and `{LISTS}` are replaced with the corresponding directories.
fn load_profile(app_dir: &Path) -> Result<Vec<String>, String> {
    let profile_path = app_dir.join("profile.txt");
    if !profile_path.is_file() {
        write_default_profile(&profile_path)?;
    }

    let text = std::fs::read_to_string(&profile_path)
        .map_err(|e| format!("Cannot read profile.txt: {e}"))?;

    let bin = format!("{}\\", app_dir.join("bin").display());
    let lists = format!("{}\\", app_dir.join("lists").display());

    let mut args = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        for tok in line.split_whitespace() {
            args.push(tok.replace("{BIN}", &bin).replace("{LISTS}", &lists));
        }
    }
    Ok(args)
}

fn write_default_profile(path: &PathBuf) -> Result<(), String> {
    std::fs::write(path, DEFAULT_PROFILE)
        .map_err(|e| format!("Cannot write profile.txt: {e}"))
}

const DEFAULT_PROFILE: &str = include_str!("../assets/profile.txt");
