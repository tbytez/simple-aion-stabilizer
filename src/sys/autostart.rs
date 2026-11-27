use super::wide;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use windows::core::{Interface, BSTR, HRESULT, PCWSTR};
use windows::Win32::Foundation::VARIANT_FALSE;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::TaskScheduler::*;
use windows::Win32::System::Variant::VARIANT;
use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, ERROR_FILE_NOT_FOUND};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenUser, SECURITY_MAX_SID_SIZE, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const LEGACY_RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const LEGACY_APPROVAL: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const LEGACY_NAME: &str = "AionStabilizer";
const TASK_NOT_FOUND: HRESULT = HRESULT::from_win32(ERROR_FILE_NOT_FOUND);

struct ComApartment;

impl ComApartment {
    fn initialize() -> windows::core::Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn current_user_sid() -> io::Result<String> {
    // The token user record and the maximum-sized SID both fit in this aligned buffer.
    let mut user = [0usize;
        (size_of::<TOKEN_USER>() + SECURITY_MAX_SID_SIZE as usize).div_ceil(size_of::<usize>())];
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut size = 0;
        let result = GetTokenInformation(
            token,
            TokenUser,
            user.as_mut_ptr().cast(),
            size_of_val(&user) as u32,
            &mut size,
        );
        let error = (result == 0).then(io::Error::last_os_error);
        CloseHandle(token);
        if let Some(error) = error {
            return Err(error);
        }

        let user = &*user.as_ptr().cast::<TOKEN_USER>();
        let mut sid = ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid) == 0 {
            return Err(io::Error::last_os_error());
        }
        let name = PCWSTR(sid).to_string();
        LocalFree(sid.cast());
        name.map_err(io::Error::other)
    }
}

struct StartupTask {
    folder: ITaskFolder,
    service: ITaskService,
    name: BSTR,
    user: BSTR,
    _apartment: ComApartment,
}

impl StartupTask {
    fn connect() -> Result<Self, Box<dyn std::error::Error>> {
        let user = current_user_sid()?;
        let apartment = ComApartment::initialize()?;
        let empty = VARIANT::default();
        unsafe {
            let service: ITaskService =
                CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
            service.Connect(&empty, &empty, &empty, &empty)?;
            Ok(Self {
                folder: service.GetFolder(&BSTR::from("\\"))?,
                service,
                name: BSTR::from(format!("SimpleAionStabilizer-{user}")),
                user: BSTR::from(user),
                _apartment: apartment,
            })
        }
    }

    fn registered(&self) -> windows::core::Result<Option<IRegisteredTask>> {
        match unsafe { self.folder.GetTask(&self.name) } {
            Ok(task) => Ok(Some(task)),
            Err(error) if error.code() == TASK_NOT_FOUND => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn register(&self, exe: &BSTR) -> windows::core::Result<()> {
        let empty = VARIANT::default();
        unsafe {
            let definition = self.service.NewTask(0)?;
            let principal = definition.Principal()?;
            principal.SetUserId(&self.user)?;
            principal.SetLogonType(TASK_LOGON_INTERACTIVE_TOKEN)?;
            principal.SetRunLevel(TASK_RUNLEVEL_HIGHEST)?;

            let trigger: ILogonTrigger =
                definition.Triggers()?.Create(TASK_TRIGGER_LOGON)?.cast()?;
            trigger.SetUserId(&self.user)?;

            let settings = definition.Settings()?;
            settings.SetExecutionTimeLimit(&BSTR::from("PT0S"))?;
            settings.SetDisallowStartIfOnBatteries(VARIANT_FALSE)?;
            settings.SetStopIfGoingOnBatteries(VARIANT_FALSE)?;

            let action: IExecAction = definition.Actions()?.Create(TASK_ACTION_EXEC)?.cast()?;
            action.SetPath(exe)?;
            action.SetArguments(&BSTR::from("--tray"))?;

            self.folder.RegisterTaskDefinition(
                &self.name,
                &definition,
                TASK_CREATE_OR_UPDATE.0,
                &empty,
                &empty,
                TASK_LOGON_INTERACTIVE_TOKEN,
                &empty,
            )?;
            Ok(())
        }
    }

    fn remove(&self) -> windows::core::Result<()> {
        match unsafe { self.folder.DeleteTask(&self.name, 0) } {
            Err(error) if error.code() == TASK_NOT_FOUND => Ok(()),
            result => result,
        }
    }
}

fn current_exe() -> io::Result<BSTR> {
    let exe = std::env::current_exe()?;
    Ok(BSTR::from_wide(
        &exe.as_os_str().encode_wide().collect::<Vec<_>>(),
    ))
}

fn registry_result(code: u32) -> io::Result<bool> {
    match code {
        0 => Ok(true),
        ERROR_FILE_NOT_FOUND => Ok(false),
        code => Err(io::Error::from_raw_os_error(code as i32)),
    }
}

fn remove_legacy_entry() -> io::Result<()> {
    let mut key = ptr::null_mut();
    unsafe {
        if registry_result(RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(LEGACY_RUN).as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut key,
        ))? {
            let result = RegDeleteValueW(key, wide(LEGACY_NAME).as_ptr());
            RegCloseKey(key);
            registry_result(result)?;
        }
    }
    Ok(())
}

fn legacy_enabled() -> io::Result<bool> {
    let name = wide(LEGACY_NAME);
    let mut size = 0;
    unsafe {
        if !registry_result(RegGetValueW(
            HKEY_CURRENT_USER,
            wide(LEGACY_RUN).as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut size,
        ))? {
            return Ok(false);
        }

        // Task Manager leaves the Run value in place and records its decision separately.
        // Migrate only the normal enabled states (or an entry with no recorded decision).
        let mut approval = [0u8; 12];
        size = approval.len() as u32;
        let has_approval = registry_result(RegGetValueW(
            HKEY_CURRENT_USER,
            wide(LEGACY_APPROVAL).as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_BINARY,
            ptr::null_mut(),
            approval.as_mut_ptr().cast(),
            &mut size,
        ))?;
        Ok(!has_approval
            || (size as usize == approval.len()
                && matches!(approval[0], 2 | 6)
                && approval[1..].iter().all(|&byte| byte == 0)))
    }
}

pub fn enabled() -> Result<bool, Box<dyn std::error::Error>> {
    let scheduler = StartupTask::connect()?;
    Ok(match scheduler.registered()? {
        Some(task) => unsafe { task.Enabled()? != VARIANT_FALSE },
        None => false,
    })
}

pub fn refresh() -> Result<bool, Box<dyn std::error::Error>> {
    let scheduler = StartupTask::connect()?;
    if let Some(task) = scheduler.registered()? {
        let enabled = unsafe { task.Enabled()? != VARIANT_FALSE };
        if enabled {
            let exe = current_exe()?;
            unsafe {
                let action: IExecAction = task.Definition()?.Actions()?.get_Item(1)?.cast()?;
                let mut path = BSTR::new();
                action.Path(&mut path)?;
                if path != exe {
                    scheduler.register(&exe)?;
                }
            }
        }
        remove_legacy_entry()?;
        return Ok(enabled);
    }

    // Leave a disabled legacy entry alone. Explicitly toggling startup cleans it up.
    if !legacy_enabled()? {
        return Ok(false);
    }
    scheduler.register(&current_exe()?)?;
    remove_legacy_entry()?;
    Ok(true)
}

pub fn set(enable: bool) -> Result<bool, Box<dyn std::error::Error>> {
    let scheduler = StartupTask::connect()?;
    if enable {
        scheduler.register(&current_exe()?)?;
        remove_legacy_entry()?;
    } else {
        // Remove the old entry first so a later refresh cannot migrate it back.
        remove_legacy_entry()?;
        scheduler.remove()?;
    }
    Ok(enable)
}
