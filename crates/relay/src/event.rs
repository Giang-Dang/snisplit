use snisplit_core::first_flight::Refusal;

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
}
