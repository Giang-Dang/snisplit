use std::fmt;
use std::io;

use crate::engine::{Engine, Verdict};

pub const MAX_PACKET_LEN: usize = 65535;

pub trait PacketIo {
    type Address;
    fn recv(&mut self, buf: &mut [u8]) -> io::Result<Option<(usize, Self::Address)>>;
    fn send(&mut self, packet: &[u8], address: &Self::Address) -> io::Result<()>;
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub received: u64,
    pub passed: u64,
    pub replaced: u64,
    pub segments_sent: u64,
    pub send_failures: u64,
}

#[derive(Debug)]
pub struct RunError {
    pub error: io::Error,
    pub stats: Stats,
}

impl From<io::Error> for RunError {
    fn from(error: io::Error) -> Self {
        Self {
            error,
            stats: Stats::default(),
        }
    }
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

pub fn run<I: PacketIo>(
    io: &mut I,
    engine: &mut Engine,
    mut now_ms: impl FnMut() -> u64,
    mut on_split: impl FnMut(&[u8], &[Vec<u8>]),
) -> Result<Stats, RunError> {
    let mut buf = vec![0; MAX_PACKET_LEN];
    let mut stats = Stats::default();
    loop {
        let (len, address) = match io.recv(&mut buf) {
            Ok(Some(received)) => received,
            Ok(None) => return Ok(stats),
            Err(error) => return Err(RunError { error, stats }),
        };
        let Some(packet) = buf.get(..len) else {
            let error = io::Error::new(
                io::ErrorKind::InvalidData,
                "recv reported more bytes than the buffer holds",
            );
            return Err(RunError { error, stats });
        };
        stats.received += 1;
        match engine.process(packet, now_ms()) {
            Verdict::Pass => {
                stats.passed += 1;
                if io.send(packet, &address).is_err() {
                    stats.send_failures += 1;
                }
            }
            Verdict::Replace(segments) => {
                stats.replaced += 1;
                on_split(packet, &segments);
                for segment in &segments {
                    match io.send(segment, &address) {
                        Ok(()) => stats.segments_sent += 1,
                        Err(_) => stats.send_failures += 1,
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
