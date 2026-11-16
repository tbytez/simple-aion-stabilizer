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

        let args = crate::profile::arguments(app_dir)?;
        if args.is_empty() {
            return Err("profile.txt contains no arguments".to_string());
        }

        // Keep the engine's own startup/driver errors available instead of
        // discarding them with the hidden console.
        let log = std::fs::File::create(app_dir.join("engine.log"))
            .map_err(|e| format!("Cannot create engine.log: {e}"))?;
        use std::io::Write;
        let mut header = &log;
        writeln!(
            header,
            "Aion Stabilizer v{}; profile.txt; engine startup",
            env!("CARGO_PKG_VERSION")
        )
        .map_err(|e| e.to_string())?;
        for arg in &args {
            writeln!(header, "{arg}").map_err(|e| e.to_string())?;
        }
        writeln!(header, "--- winws output ---").map_err(|e| e.to_string())?;
        let stderr = log
            .try_clone()
            .map_err(|e| format!("Cannot open engine log: {e}"))?;

        let child = Command::new(&winws)
            .args(&args)
            .stdout(log)
            .stderr(stderr)
            .current_dir(app_dir)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Failed to start winws.exe: {e}"))?;

        // If this app dies or is force-killed, the OS kills winws with it.
        crate::sys::tie_child_to_process(child.id());

        self.child = Some(child);
        self.auto_started = auto;
        self.last_exit = None;
        // Controller thread only: catch immediate argument/driver failures
        // before presenting the engine as running, matching AXV's startup check.
        std::thread::sleep(std::time::Duration::from_millis(400));
        if !self.is_running() {
            let error = self
                .last_exit
                .take()
                .unwrap_or_else(|| "unknown startup error".into());
            return Err(format!("Engine failed to start ({error}). See engine.log."));
        }
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
        // winws died via TerminateProcess without WinDivertClose - unload
        // the driver service ourselves so bin\WinDivert64.sys unlocks and
        // the folder can be deleted/updated without a reboot. Skip when a
        // foreign zapret instance is alive: its winws still owns the driver.
        #[cfg(windows)]
        if !crate::procs::is_running("winws.exe") {
            crate::sys::windivert_unload();
        }
        self.auto_started = false;
        self.last_exit = None;
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn repeated_start_stop_reaps_children() {
        let mut engine = Engine::new();
        for _ in 0..200 {
            engine.start(Path::new("."), false).unwrap();
            assert!(engine.is_running());
            assert!(engine.start(Path::new("."), false).is_err());
            engine.stop();
            assert!(!engine.is_running());
            assert!(engine.child.is_none());
            engine.stop();
        }
    }
}
