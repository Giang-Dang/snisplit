use std::ffi::CString;
use std::io;

use crate::ffi;

pub mod flags {
    pub const SNIFF: u64 = 0x0001;
    pub const RECV_ONLY: u64 = 0x0004;
    pub const NO_INSTALL: u64 = 0x0010;
}

#[derive(Clone, Copy)]
pub struct Address(#[allow(dead_code)] ffi::WINDIVERT_ADDRESS);

pub struct Handle(ffi::HANDLE);

unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Handle {
    pub const PRIORITY_HIGHEST: i16 = 30000;

    pub fn open(filter: &str, priority: i16, flags: u64) -> io::Result<Self> {
        let filter = CString::new(filter).map_err(io::Error::other)?;
        let handle = unsafe {
            ffi::WinDivertOpen(
                filter.as_ptr(),
                ffi::WINDIVERT_LAYER_NETWORK,
                priority,
                flags,
            )
        };
        if handle == ffi::INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(handle))
    }

    pub fn recv(&self, buf: &mut [u8]) -> io::Result<(usize, Address)> {
        let capacity = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        let mut received_len = 0u32;
        let mut address = ffi::WINDIVERT_ADDRESS::ZEROED;
        let succeeded = unsafe {
            ffi::WinDivertRecv(
                self.0,
                buf.as_mut_ptr().cast(),
                capacity,
                &mut received_len,
                &mut address,
            )
        };
        if succeeded == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((received_len as usize, Address(address)))
    }

    pub fn shutdown(&self) -> io::Result<()> {
        let succeeded = unsafe { ffi::WinDivertShutdown(self.0, ffi::WINDIVERT_SHUTDOWN_BOTH) };
        if succeeded == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { ffi::WinDivertClose(self.0) };
    }
}
