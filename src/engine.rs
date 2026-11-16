#[cfg(windows)]
use crate::core;
use std::path::Path;
use std::process::{Child, Command};

/// Manages the winws.exe (zapret) child process.
/// On non-Windows hosts it drives `sleep` instead so the UI can be previewed.
pub struct Engine {
    child: Option<Child>,
    /// true when the current run was started by the Aion watcher
    pub auto_started: bool,
    /// set once when a running engine exits on its own
    pub last_exit: Option<String>,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            child: None,
            auto_started: false,
            last_exit: None,
        }
    }

    /// True while the child we spawned is still alive. On death, records the
    /// exit status in `last_exit` (once).
    pub fn is_running(&mut self) -> bool {
        match self.child.as_mut().map(|c| c.try_wait()) {
            Some(Ok(None)) => true,
            Some(Ok(Some(status))) => {
                if self.last_exit.is_none() {
                    self.last_exit = Some(match status.code() {
                        Some(c) => format!("exit code {c}"),
                        None => "terminated".to_string(),
                    });
                }
                self.child = None;
                self.auto_started = false;
                false
            }
            Some(Err(_)) | None => {
                self.child = None;
                self.auto_started = false;
                false
            }
        }
    }

    #[cfg(windows)]
    pub fn start(&mut self, app_dir: &Path, auto: bool) -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        if self.is_running() {
            return Err("Engine is already running".to_string());
        }

        let winws = app_dir.join("bin").join("winws.exe");
        if !winws.is_file() {
            return Err(format!(
                "winws.exe not found in {}",
                app_dir.join("bin").display()
            ));
        }

        // Another zapret instance (ours from a zombie run, AXV, or a zapret
        // service) would fight over the same WinDivert filters. Refuse cleanly.
        if crate::procs::is_running("winws.exe") {
            return Err(
                "winws.exe is already running - another zapret instance is active. Stop it first."
                    .to_string(),
            );
        }

        let args = load_profile(app_dir)?;
        if args.is_empty() {
            return Err("profile.txt contains no arguments".to_string());
        }

        // Some desync modes need TCP timestamps enabled system-wide (same
        // thing the zapret service scripts do). Harmless standard TCP option.
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

        // If this app dies or is force-killed, the OS kills winws with it.
        crate::sys::tie_child_to_process(child.id());

        self.child = Some(child);
        self.auto_started = auto;
        self.last_exit = None;
        Ok(())
    }

    /// Preview mode on non-Windows: a real long-lived child we can kill.
    #[cfg(not(windows))]
    pub fn start(&mut self, _app_dir: &Path, auto: bool) -> Result<(), String> {
        if self.is_running() {
            return Err("Engine is already running".to_string());
        }
        if std::env::var("AION_MOCK_FAIL").is_ok() {
            return Err("mock failure".to_string());
        }
        let child = Command::new("sleep")
            .arg("3600")
            .spawn()
            .map_err(|e| e.to_string())?;
        self.child = Some(child);
        self.auto_started = auto;
        self.last_exit = None;
        Ok(())
    }

    /// Kill our child. Foreign winws.exe instances are left alone - the job
    /// object guarantees ours can never stray.
    pub fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.auto_started = false;
        self.last_exit = None;
    }
}

/// Parse profile.txt into a winws argument vector, writing the default
/// profile when the file is missing.
#[cfg(windows)]
fn load_profile(app_dir: &Path) -> Result<Vec<String>, String> {
    let profile_path = app_dir.join("profile.txt");
    if !profile_path.is_file() {
        std::fs::write(&profile_path, DEFAULT_PROFILE)
            .map_err(|e| format!("Cannot write profile.txt: {e}"))?;
    }

    let text = std::fs::read_to_string(&profile_path)
        .map_err(|e| format!("Cannot read profile.txt: {e}"))?;

    let bin = format!("{}\\", app_dir.join("bin").display());
    let lists = format!("{}\\", app_dir.join("lists").display());
    Ok(core::parse_profile_args(&text, &bin, &lists))
}

#[cfg(windows)]
const DEFAULT_PROFILE: &str = include_str!("../assets/profile.txt");
