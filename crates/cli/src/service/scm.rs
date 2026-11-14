use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use windows_sys::Win32::Foundation::ERROR_SERVICE_DOES_NOT_EXIST;
use windows_sys::Win32::System::Services::{
    ChangeServiceConfig2W, CloseServiceHandle, ControlService, CreateServiceW, DeleteService,
    OpenSCManagerW, OpenServiceW, QueryServiceStatus, SC_ACTION, SC_HANDLE, SERVICE_ALL_ACCESS,
    SERVICE_AUTO_START, SERVICE_CONFIG_DESCRIPTION, SERVICE_CONFIG_FAILURE_ACTIONS,
    SERVICE_CONFIG_FAILURE_ACTIONS_FLAG, SERVICE_CONTROL_STOP, SERVICE_DESCRIPTIONW,
    SERVICE_ERROR_NORMAL, SERVICE_FAILURE_ACTIONS_FLAG, SERVICE_FAILURE_ACTIONSW, SERVICE_STATUS,
    SERVICE_STATUS_CURRENT_STATE, SERVICE_WIN32_OWN_PROCESS, StartServiceW,
};
use windows_sys::core::BOOL;

use super::SERVICE_NAME;

const _: () = assert!(size_of::<SERVICE_STATUS>() == 28);
const _: () = assert!(size_of::<SC_ACTION>() == 8);

pub fn wide(text: impl AsRef<OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain([0]).collect()
}

pub fn is_error(error: &io::Error, code: u32) -> bool {
    i32::try_from(code).is_ok_and(|code| error.raw_os_error() == Some(code))
}

pub struct Handle(SC_HANDLE);

impl Handle {
    fn checked(raw: SC_HANDLE) -> io::Result<Self> {
        if raw.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let raw = self.0;
        unsafe { CloseServiceHandle(raw) };
    }
}

fn succeeded(result: BOOL) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn open_manager(access: u32) -> io::Result<Handle> {
    let raw = unsafe { OpenSCManagerW(ptr::null(), ptr::null(), access) };
    Handle::checked(raw)
}

pub fn open_service(manager: &Handle, access: u32) -> io::Result<Option<Handle>> {
    let name = wide(SERVICE_NAME);
    let raw = unsafe { OpenServiceW(manager.0, name.as_ptr(), access) };
    match Handle::checked(raw) {
        Ok(service) => Ok(Some(service)),
        Err(error) if is_error(&error, ERROR_SERVICE_DOES_NOT_EXIST) => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn create_service(manager: &Handle, command_line: &str) -> io::Result<Handle> {
    let name = wide(SERVICE_NAME);
    let binary = wide(command_line);
    let raw = unsafe {
        CreateServiceW(
            manager.0,
            name.as_ptr(),
            name.as_ptr(),
            SERVICE_ALL_ACCESS,
            SERVICE_WIN32_OWN_PROCESS,
            SERVICE_AUTO_START,
            SERVICE_ERROR_NORMAL,
            binary.as_ptr(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
        )
    };
    Handle::checked(raw)
}

pub fn set_description(service: &Handle, text: &str) -> io::Result<()> {
    let mut text = wide(text);
    let description = SERVICE_DESCRIPTIONW {
        lpDescription: text.as_mut_ptr(),
    };
    let info = ptr::from_ref(&description).cast();
    let result = unsafe { ChangeServiceConfig2W(service.0, SERVICE_CONFIG_DESCRIPTION, info) };
    succeeded(result)
}

pub fn set_failure_actions(
    service: &Handle,
    reset_period_seconds: u32,
    mut actions: Vec<SC_ACTION>,
) -> io::Result<()> {
    let count = u32::try_from(actions.len()).map_err(io::Error::other)?;
    let failure_actions = SERVICE_FAILURE_ACTIONSW {
        dwResetPeriod: reset_period_seconds,
        lpRebootMsg: ptr::null_mut(),
        lpCommand: ptr::null_mut(),
        cActions: count,
        lpsaActions: actions.as_mut_ptr(),
    };
    let info = ptr::from_ref(&failure_actions).cast();
    let result = unsafe { ChangeServiceConfig2W(service.0, SERVICE_CONFIG_FAILURE_ACTIONS, info) };
    succeeded(result)
}

pub fn apply_failure_actions_to_error_exits(service: &Handle) -> io::Result<()> {
    let flag = SERVICE_FAILURE_ACTIONS_FLAG {
        fFailureActionsOnNonCrashFailures: 1,
    };
    let info = ptr::from_ref(&flag).cast();
    let result =
        unsafe { ChangeServiceConfig2W(service.0, SERVICE_CONFIG_FAILURE_ACTIONS_FLAG, info) };
    succeeded(result)
}

pub fn start(service: &Handle) -> io::Result<()> {
    let result = unsafe { StartServiceW(service.0, 0, ptr::null()) };
    succeeded(result)
}

pub fn send_stop(service: &Handle) -> io::Result<()> {
    let mut status = SERVICE_STATUS::default();
    let result = unsafe { ControlService(service.0, SERVICE_CONTROL_STOP, &mut status) };
    succeeded(result)
}

pub fn current_state(service: &Handle) -> io::Result<SERVICE_STATUS_CURRENT_STATE> {
    let mut status = SERVICE_STATUS::default();
    let result = unsafe { QueryServiceStatus(service.0, &mut status) };
    succeeded(result).map(|()| status.dwCurrentState)
}

pub fn delete(service: &Handle) -> io::Result<()> {
    let result = unsafe { DeleteService(service.0) };
    succeeded(result)
}
