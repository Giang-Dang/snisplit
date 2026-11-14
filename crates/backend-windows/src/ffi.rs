//! Raw declarations from WinDivert 2.x `windivert.h` (x64).

#![allow(non_camel_case_types, clippy::upper_case_acronyms)]

use core::ffi::{c_char, c_int, c_void};

pub type HANDLE = *mut c_void;
pub type BOOL = i32;

pub const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;

pub const WINDIVERT_LAYER_NETWORK: c_int = 0;
pub const WINDIVERT_SHUTDOWN_BOTH: c_int = 3;

/// `WINDIVERT_ADDRESS`: 8 + 4 + 4 + 64 = 80 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WINDIVERT_ADDRESS {
    pub timestamp: i64,
    /// Bitfields: Layer:8, Event:8, Sniffed:1, Outbound:1, Loopback:1,
    /// Impostor:1, IPv6:1, IPChecksum:1, TCPChecksum:1, UDPChecksum:1, Reserved1:8.
    pub bits: u32,
    pub reserved2: u32,
    pub data: [u8; 64],
}

const _: () = assert!(core::mem::size_of::<WINDIVERT_ADDRESS>() == 80);

#[link(name = "WinDivert", kind = "raw-dylib")]
unsafe extern "C" {
    pub fn WinDivertOpen(filter: *const c_char, layer: c_int, priority: i16, flags: u64) -> HANDLE;
    pub fn WinDivertRecv(
        handle: HANDLE,
        packet: *mut c_void,
        packet_len: u32,
        recv_len: *mut u32,
        addr: *mut WINDIVERT_ADDRESS,
    ) -> BOOL;
    pub fn WinDivertShutdown(handle: HANDLE, how: c_int) -> BOOL;
    pub fn WinDivertClose(handle: HANDLE) -> BOOL;
}
