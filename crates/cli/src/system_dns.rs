use std::io;
use std::net::Ipv4Addr;

use crate::routing;

pub fn servers() -> io::Result<Vec<Ipv4Addr>> {
    routing::system_dns_servers()
}
