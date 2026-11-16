//! Process enumeration via Toolhelp32 snapshot (no tasklist subprocess).

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;

    /// Lowercase image names of every running process.
    pub fn running_names() -> Vec<String> {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return Vec::new();
            }
            let mut names = Vec::with_capacity(160);
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            if Process32FirstW(snap, &mut e) != 0 {
                loop {
                    let end = e
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(e.szExeFile.len());
                    names.push(String::from_utf16_lossy(&e.szExeFile[..end]).to_lowercase());
                    if Process32NextW(snap, &mut e) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
            names
        }
    }
}

#[cfg(not(windows))]
mod imp {
    /// Preview stub on non-Windows hosts.
    pub fn running_names() -> Vec<String> {
        std::env::var("AION_FAKE_GAME")
            .map(|_| vec!["aion2.exe".to_string()])
            .unwrap_or_default()
    }
}

pub use imp::running_names;

/// True when any of `names` (lowercase) is running.
pub fn any_running(names: &[String]) -> bool {
    if names.is_empty() {
        return false;
    }
    let running = running_names();
    names.iter().any(|n| running.iter().any(|r| r == n))
}

/// True when a single process name is running.
#[allow(dead_code)] // used on Windows (winws conflict check)
pub fn is_running(name: &str) -> bool {
    running_names().iter().any(|r| r == name)
}
