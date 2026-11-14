#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    TransferEncoding,
    ContentLength,
    Upgrade,
    Connect,
    HeadTooLong,
    NotHttp,
}

pub const MAX_HEAD: usize = 16 * 1024;

#[derive(Debug, Default)]
pub struct HostRespeller {
    respelled: usize,
    stopped: Option<StopReason>,
}

impl HostRespeller {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, input: &[u8], output: &mut Vec<u8>) {
        output.extend_from_slice(input);
    }

    pub fn finish(&mut self, output: &mut Vec<u8>) {
        output.extend_from_slice(&[]);
    }

    pub fn respelled(&self) -> usize {
        self.respelled
    }

    pub fn stopped(&self) -> Option<StopReason> {
        self.stopped
    }
}
