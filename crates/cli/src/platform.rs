use std::io;

use snisplit_core::engine::Engine;
use snisplit_core::pump::{RunError, Stats};

pub const DIVERT_FILTER: &str = "outbound and !loopback and !impostor and tcp.PayloadLength > 0 and (tcp.DstPort == 443 or tcp.DstPort == 80)";

#[cfg(windows)]
pub fn divert(
    _filter: &str,
    _engine: &mut Engine,
    _now_ms: impl FnMut() -> u64,
    _on_split: impl FnMut(&[u8], &[Vec<u8>]),
) -> Result<Stats, RunError> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "the Windows backend is not wired into the command line yet",
    )
    .into())
}

#[cfg(not(windows))]
pub fn divert(
    _filter: &str,
    _engine: &mut Engine,
    _now_ms: impl FnMut() -> u64,
    _on_split: impl FnMut(&[u8], &[Vec<u8>]),
) -> Result<Stats, RunError> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "this platform is not supported yet",
    )
    .into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_is_the_decided_wide_filter_with_impostor_excluded() {
        let decided = "outbound and !loopback and !impostor and tcp.PayloadLength > 0 and \
                       (tcp.DstPort == 443 or tcp.DstPort == 80)";
        assert_eq!(DIVERT_FILTER, decided);
    }
}
