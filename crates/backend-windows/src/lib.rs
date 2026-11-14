#[cfg(windows)]
mod ffi;
#[cfg(windows)]
mod handle;

#[cfg(windows)]
pub use handle::{Address, Handle, flags};
