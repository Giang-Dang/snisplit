#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    InSequence,
    Reversed,
}

pub fn split(_packet: &[u8], _payload_offsets: &[usize], _order: Order) -> Option<Vec<Vec<u8>>> {
    todo!()
}
