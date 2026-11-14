use std::ffi::CString;
use std::io;

use crate::ffi;

/// `WinDivertOpen` flags.
pub mod flags {
    /// Copy packets instead of diverting them; traffic is never affected.
    pub const SNIFF: u64 = 0x0001;
    pub const RECV_ONLY: u64 = 0x0004;
    /// Fail instead of installing the driver if it is not already loaded.
    pub const NO_INSTALL: u64 = 0x0010;
}

/// Metadata WinDivert attaches to each captured packet. Opaque for now; it is
/// passed back unchanged when sending (milestone 2). Bitfield accessors are
/// added once their bit positions are checked against a real capture.
#[derive(Clone, Copy)]
pub struct Address(#[allow(dead_code)] ffi::WINDIVERT_ADDRESS);

/// An open WinDivert handle on the network layer. Closed on drop.
pub struct Handle(ffi::HANDLE);

// WinDivert handles may be used from several threads at once (e.g. one
// thread blocked in recv while another calls shutdown).
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Handle {
    /// This handle sees packets before any other WinDivert user.
    pub const PRIORITY_HIGHEST: i16 = 30000;

    /// Opens a handle with a WinDivert filter expression. Needs admin rights.
    pub fn open(filter: &str, priority: i16, flags: u64) -> io::Result<Self> {
        let filter = CString::new(filter).map_err(io::Error::other)?;
        // SAFETY: `filter` is a valid NUL-terminated string for the call.
        let h = unsafe {
            ffi::WinDivertOpen(
                filter.as_ptr(),
                ffi::WINDIVERT_LAYER_NETWORK,
                priority,
                flags,
            )
        };
        if h == ffi::INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(h))
    }

    /// Blocks until a packet arrives. Returns its length in `buf` and its
    /// metadata. Fails after [`Handle::shutdown`] once the queue is drained.
    pub fn recv(&self, buf: &mut [u8]) -> io::Result<(usize, Address)> {
        let mut len = 0u32;
        let mut addr = core::mem::MaybeUninit::<ffi::WINDIVERT_ADDRESS>::zeroed();
        let cap = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        // SAFETY: buf is valid for `cap` bytes; len and addr are valid out-pointers.
        let ok = unsafe {
            ffi::WinDivertRecv(
                self.0,
                buf.as_mut_ptr().cast(),
                cap,
                &mut len,
                addr.as_mut_ptr(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: zero-initialised and then filled by WinDivert; all bit patterns are valid.
        let addr = unsafe { addr.assume_init() };
        Ok((len as usize, Address(addr)))
    }

    /// Stops receiving; a blocked [`Handle::recv`] returns an error.
    pub fn shutdown(&self) -> io::Result<()> {
        // SAFETY: self.0 is an open handle until drop.
        if unsafe { ffi::WinDivertShutdown(self.0, ffi::WINDIVERT_SHUTDOWN_BOTH) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: self.0 is an open handle and is not used after this.
        unsafe { ffi::WinDivertClose(self.0) };
    }
}
