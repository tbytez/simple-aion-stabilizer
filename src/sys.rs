//! Windows-only OS glue: single instance, window handle, registry, jobs.

#![cfg(windows)]

use std::path::PathBuf;
use std::ptr;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, HWND, INVALID_HANDLE_VALUE, WAIT_ABANDONED, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::Services::*;
use windows_sys::Win32::System::SystemInformation::{GetSystemDirectoryW, GetWindowsDirectoryW};
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

pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_path(p: &std::path::Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

// ---------------- trusted system paths ----------------

/// Absolute path of the Windows system directory (e.g. C:\Windows\System32)
/// via the OS API - never PATH- or environment-derived.
pub fn system_dir() -> PathBuf {
    let mut buf = [0u16; 260];
    unsafe {
        let n = GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32);
        if n > 0 && (n as usize) < buf.len() {
            return PathBuf::from(String::from_utf16_lossy(&buf[..n as usize]));
        }
    }
    PathBuf::from(r"C:\Windows\System32")
}

/// Absolute path of a tool living in the Windows system directory
/// (netsh.exe, sc.exe, ...). Use instead of a bare name so a hostile
/// PATH cannot redirect the call.
pub fn system_tool(exe: &str) -> PathBuf {
    system_dir().join(exe)
}

/// Machine temp dir (e.g. C:\Windows\Temp). Its default ACL only allows
/// admins/SYSTEM, so update staging under it is outside any
/// unelevated user's reach. Falls back to the user temp dir.
pub fn machine_temp_dir() -> PathBuf {
    let mut buf = [0u16; 260];
    unsafe {
        let n = GetWindowsDirectoryW(buf.as_mut_ptr(), buf.len() as u32);
        if n > 0 && (n as usize) < buf.len() {
            return PathBuf::from(String::from_utf16_lossy(&buf[..n as usize])).join("Temp");
        }
    }
    std::env::temp_dir()
}

// ---------------- private update staging dir ----------------

/// SDDL: protected DACL (no inheritance from the parent), full control
/// for SYSTEM and the Administrators group only. An unelevated process
/// of the same interactive user does not hold the Administrators SID in
/// its filtered token, so it cannot open, write, or create anything
/// inside the staging tree.
const STAGE_SDDL: &str = "O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

fn stage_security_descriptor() -> Option<windows_sys::Win32::Security::PSECURITY_DESCRIPTOR> {
    unsafe {
        let mut sd = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide(STAGE_SDDL).as_ptr(),
            SDDL_REVISION_1,
            &mut sd,
            ptr::null_mut(),
        ) != 0
        {
            Some(sd)
        } else {
            None
        }
    }
}

/// A staging directory that only elevated admins can touch.
///
/// - created exclusively under the machine temp dir with a random name
///   and a restrictive ACL set atomically at creation (no
///   world-writable window, no pre-created or squatted directory)
/// - an open handle with sharing but no FILE_SHARE_DELETE is held for
///   the whole update, so nothing can rename or delete the dir out
///   from under the updater
/// - children inherit the ACL, keeping the downloaded zip, manifest,
///   and extraction tree private for the update's lifetime
pub struct SecureStage {
    dir: PathBuf,
    hold: HANDLE,
}

impl SecureStage {
    /// Create a private staging dir under `parent` (use machine_temp_dir).
    pub fn new(parent: &std::path::Path) -> Result<Self, String> {
        let Some(sd) = stage_security_descriptor() else {
            return Err("cannot build staging security descriptor".to_string());
        };
        let result = Self::create(parent, sd);
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(sd as _);
        }
        result
    }

    fn create(
        parent: &std::path::Path,
        sd: windows_sys::Win32::Security::PSECURITY_DESCRIPTOR,
    ) -> Result<Self, String> {
        const ERROR_ALREADY_EXISTS: u32 = 183;
        let mut sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd as *mut _,
            bInheritHandle: 0,
        };
        for _ in 0..64 {
            let dir = parent.join(format!("aion-stabilizer-update-{}", rand_hex16()));
            let w = wide_path(&dir);
            unsafe {
                if CreateDirectoryW(w.as_ptr(), &mut sa) == 0 {
                    if GetLastError() == ERROR_ALREADY_EXISTS {
                        continue;
                    }
                    return Err(format!(
                        "cannot create staging dir {}: error {}",
                        dir.display(),
                        GetLastError()
                    ));
                }
                // pin the dir: shared read/write, no delete
                let hold = CreateFileW(
                    w.as_ptr(),
                    FILE_GENERIC_READ,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS,
                    ptr::null_mut(),
                );
                if hold.is_null() || hold == INVALID_HANDLE_VALUE {
                    let _ = std::fs::remove_dir_all(&dir);
                    return Err("cannot pin staging dir".to_string());
                }
                return Ok(Self { dir, hold });
            }
        }
        Err("cannot create staging dir (name collision)".to_string())
    }

    pub fn path(&self) -> &std::path::Path {
        &self.dir
    }
}

impl Drop for SecureStage {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.hold);
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn rand_hex16() -> String {
    let mut b = [0u8; 8];
    unsafe {
        BCryptGenRandom(
            ptr::null_mut(),
            b.as_mut_ptr(),
            b.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        );
    }
    crate::manifest::hex_encode(&b)
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

/// Block until the instance mutex is owned or `timeout` expires.
/// Used by `--finish-update`: acquiring it proves the previous instance
/// exited AND keeps any new launch from starting during the file swap.
/// WAIT_ABANDONED counts as acquired - that is exactly the dead-process
/// case we are waiting for. Pass the handle to release_mutex when done.
pub fn wait_instance_mutex(timeout: std::time::Duration) -> Option<HANDLE> {
    unsafe {
        let h = CreateMutexW(ptr::null(), 0, MUTEX_NAME.as_ptr());
        if h.is_null() {
            return None;
        }
        let ms = timeout.as_millis().min(u32::MAX as u128) as u32;
        match WaitForSingleObject(h, ms) {
            WAIT_OBJECT_0 | WAIT_ABANDONED => Some(h),
            _ => {
                CloseHandle(h);
                None
            }
        }
    }
}

/// Release ownership of the instance mutex and close the handle.
pub fn release_mutex(h: HANDLE) {
    unsafe {
        ReleaseMutex(h);
        CloseHandle(h);
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
