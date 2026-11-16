//! Windows-only OS glue: single instance, window handle, registry, jobs.

#![cfg(windows)]

use std::ptr;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, HWND};
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::Services::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const MUTEX_NAME: &[u16] = &[
    b'L' as u16, b'o' as u16, b'c' as u16, b'a' as u16, b'l' as u16, b'\\' as u16, b'S' as u16,
    b'i' as u16, b'm' as u16, b'p' as u16, b'l' as u16, b'e' as u16, b'A' as u16, b'i' as u16,
    b'o' as u16, b'n' as u16, b'S' as u16, b't' as u16, b'a' as u16, b'b' as u16, b'i' as u16,
    b'l' as u16, b'i' as u16, b'z' as u16, b'e' as u16, b'r' as u16, b'M' as u16, b'u' as u16,
    b't' as u16, b'e' as u16, b'x' as u16, 0,
];
const ERROR_ALREADY_EXISTS: u32 = 183;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Returns false when another instance already holds the mutex.
pub fn acquire_single_instance() -> bool {
    unsafe {
        let h = CreateMutexW(ptr::null(), 1, MUTEX_NAME.as_ptr());
        if h.is_null() {
            return true; // cannot tell - don't block the user
        }
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}

/// Find our main window by title.
pub fn main_hwnd() -> HWND {
    unsafe { FindWindowW(ptr::null(), wide(crate::TITLE).as_ptr()) }
}

/// Is the main window actually visible right now (WS_VISIBLE)?
/// Ground truth for re-shows that bypass winit's flag cache.
pub fn main_window_visible() -> bool {
    unsafe {
        let hwnd = main_hwnd();
        !hwnd.is_null() && IsWindowVisible(hwnd) != 0
    }
}

/// Restore + foreground the main window.
pub fn show_main_window() {
    unsafe {
        let hwnd = main_hwnd();
        if !hwnd.is_null() {
            ShowWindow(hwnd, SW_RESTORE);
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }
    }
}

/// Focus the already-running instance's window, if it exists.
pub fn focus_existing_instance() {
    show_main_window();
}

const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWCP_ROUND: u32 = 2;

/// Ask DWM for rounded corners + shadow (no-op on Windows 10 or failure).
pub fn apply_window_style() {
    extern "C" {
        fn DwmSetWindowAttribute(
            hwnd: HWND,
            dwAttribute: u32,
            pvAttribute: *const core::ffi::c_void,
            cbAttribute: u32,
        ) -> i32;
    }
    unsafe {
        let hwnd = main_hwnd();
        if !hwnd.is_null() {
            let v = DWMWCP_ROUND;
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &v as *const _ as *const _,
                4,
            );
        }
    }
}

// ---------------- registry autostart ----------------

fn run_key_wide() -> Vec<u16> {
    wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
}

fn value_name_wide() -> Vec<u16> {
    wide("AionStabilizer")
}

fn open_run_key() -> HKEY {
    unsafe {
        let mut key: HKEY = ptr::null_mut();
        let name = run_key_wide();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            name.as_ptr(),
            0,
            KEY_READ | KEY_WRITE,
            &mut key,
        ) == 0
        {
            key
        } else {
            ptr::null_mut()
        }
    }
}

pub fn autostart_enabled() -> bool {
    unsafe {
        let key = open_run_key();
        if key.is_null() {
            return false;
        }
        let mut ty = 0u32;
        let mut size = 0u32;
        let name = value_name_wide();
        let ok = RegQueryValueExW(
            key,
            name.as_ptr(),
            ptr::null(),
            &mut ty,
            ptr::null_mut(),
            &mut size,
        ) == 0;
        RegCloseKey(key);
        ok
    }
}

pub fn autostart_set(enable: bool) {
    unsafe {
        let key = open_run_key();
        if key.is_null() {
            return;
        }
        let name = value_name_wide();
        if enable {
            if let Ok(exe) = std::env::current_exe() {
                let cmd = format!("\"{}\" --tray", exe.display());
                let w = wide(&cmd);
                RegSetValueExW(
                    key,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    w.as_ptr() as *const u8,
                    (w.len() * 2) as u32,
                );
            }
        } else {
            RegDeleteValueW(key, name.as_ptr());
        }
        RegCloseKey(key);
    }
}

// ---------------- job object: kill winws if we die ----------------

fn kill_on_close_job() -> HANDLE {
    static JOB: OnceLock<usize> = OnceLock::new();
    let v = *JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(ptr::null(), ptr::null());
        if job.is_null() {
            return 0usize;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        job as usize
    });
    v as HANDLE
}

/// Tie a child pid to this process: if we exit/crash, the OS kills the child.
pub fn tie_child_to_process(pid: u32) {
    unsafe {
        let job = kill_on_close_job();
        if job.is_null() {
            return;
        }
        let h = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
        if !h.is_null() {
            AssignProcessToJobObject(job, h);
            CloseHandle(h);
        }
    }
}

// ---------------- WinDivert driver cleanup ----------------
//
// We kill winws with TerminateProcess, so its WinDivertClose (which stops
// and deletes the "WinDivert" service) never runs. Without that the driver
// image stays loaded for the rest of the boot and bin\WinDivert64.sys
// cannot be deleted or overwritten. Stopping + deleting the service
// unloads it right away - the app always runs elevated so recreating it
// on the next engine start is fine.

const DELETE_ACCESS: u32 = 0x0001_0000;
const DRIVER_SERVICES: &[&str] = &["WinDivert", "WinDivert14"];

/// Best-effort: stop + delete the WinDivert service(s). Safe to call when
//  nothing is running - missing/stopped services are ignored.
pub fn windivert_unload() {
    unsafe {
        let scm = OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT);
        if scm.is_null() {
            return;
        }
        for name in DRIVER_SERVICES {
            let w = wide(name);
            let svc = OpenServiceW(scm, w.as_ptr(), SERVICE_STOP | DELETE_ACCESS);
            if svc.is_null() {
                continue; // not installed under this name
            }
            let mut status: SERVICE_STATUS = std::mem::zeroed();
            let _ = ControlService(svc, SERVICE_CONTROL_STOP, &mut status);
            let _ = DeleteService(svc);
            CloseServiceHandle(svc);
        }
        CloseServiceHandle(scm);
    }
}
