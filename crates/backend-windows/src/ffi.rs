#![allow(non_camel_case_types, clippy::upper_case_acronyms)]

use core::ffi::{c_char, c_int, c_void};

pub type HANDLE = *mut c_void;
pub type BOOL = i32;

pub const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;

pub const WINDIVERT_LAYER_NETWORK: c_int = 0;
pub const WINDIVERT_SHUTDOWN_BOTH: c_int = 3;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WINDIVERT_ADDRESS {
    pub timestamp: i64,
    pub layer_event_and_flag_bits: u32,
    pub reserved: u32,
    pub layer_data: [u8; 64],
}

impl WINDIVERT_ADDRESS {
    pub const ZEROED: Self = Self {
        timestamp: 0,
        layer_event_and_flag_bits: 0,
        reserved: 0,
        layer_data: [0; 64],
    };
}

const WINDIVERT_ADDRESS_SIZE_IN_C: usize = 80;
const _: () = assert!(size_of::<WINDIVERT_ADDRESS>() == WINDIVERT_ADDRESS_SIZE_IN_C);

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
