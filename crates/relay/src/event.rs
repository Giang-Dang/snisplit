use snisplit_core::first_flight::Refusal;
use snisplit_core::http_stream::StopReason;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Forwarded {
        host: String,
        port: u16,
        pieces: usize,
    },
    Refused {
        port: u16,
        refusal: Refusal,
    },
    Failed {
        host: String,
        port: u16,
        error: String,
    },
    Closed {
        host: String,
        port: u16,
        to_server: u64,
        to_client: u64,
        respelled: usize,
        stopped: Option<StopReason>,
    },
}
