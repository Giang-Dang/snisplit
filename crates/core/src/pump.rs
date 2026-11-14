use std::io;

pub trait PacketIo {
    type Address;
    fn recv(&mut self, buf: &mut [u8]) -> io::Result<Option<(usize, Self::Address)>>;
    fn send(&mut self, packet: &[u8], address: &Self::Address) -> io::Result<()>;
}
