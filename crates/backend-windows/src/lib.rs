//! WinDivert backend.
//!
//! Hand-written bindings to the few WinDivert 2.x functions snisplit uses,
//! linked with `raw-dylib` so building needs neither the WinDivert SDK nor an
//! import library. `WinDivert.dll` is only needed when a binary using these
//! functions runs.

#[cfg(windows)]
mod ffi;
#[cfg(windows)]
mod handle;

#[cfg(windows)]
pub use handle::{Address, Handle, flags};
