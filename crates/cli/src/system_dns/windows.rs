use std::io;
use std::mem::size_of;
use std::net::Ipv4Addr;
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_NO_DATA, ERROR_SUCCESS};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_FRIENDLY_NAME, GAA_FLAG_SKIP_MULTICAST,
    GAA_FLAG_SKIP_UNICAST, GET_ADAPTERS_ADDRESSES_FLAGS, GetAdaptersAddresses,
    IP_ADAPTER_ADDRESSES_LH, IP_ADAPTER_DNS_SERVER_ADDRESS_XP,
};
use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows_sys::Win32::Networking::WinSock::{AF_INET, SOCKADDR, SOCKET_ADDRESS};

use super::Adapter;

const FIRST_BUFFER_BYTES: usize = 15 * 1024;
const MAX_ATTEMPTS: usize = 4;
const ONLY_DNS_SERVERS: GET_ADAPTERS_ADDRESSES_FLAGS = GAA_FLAG_SKIP_UNICAST
    | GAA_FLAG_SKIP_ANYCAST
    | GAA_FLAG_SKIP_MULTICAST
    | GAA_FLAG_SKIP_FRIENDLY_NAME;

type AlignedWord = u64;

pub(super) fn adapters() -> io::Result<Vec<Adapter>> {
    adapters_from_a_first_buffer_of(FIRST_BUFFER_BYTES)
}

pub(super) fn adapters_from_a_first_buffer_of(first_bytes: usize) -> io::Result<Vec<Adapter>> {
    let mut wanted_bytes = first_bytes;
    for _ in 0..MAX_ATTEMPTS {
        let mut buffer: Vec<AlignedWord> = vec![0; wanted_bytes.div_ceil(size_of::<AlignedWord>())];
        let mut size = u32::try_from(buffer.len() * size_of::<AlignedWord>())
            .map_err(|_| io::Error::other("the adapter list is too large"))?;
        let first = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        let family = u32::from(AF_INET);
        let status = unsafe {
            GetAdaptersAddresses(family, ONLY_DNS_SERVERS, ptr::null(), first, &mut size)
        };
        match status {
            ERROR_SUCCESS => return Ok(walk_adapters(first)),
            ERROR_NO_DATA => return Ok(Vec::new()),
            ERROR_BUFFER_OVERFLOW => wanted_bytes = size as usize,
            code => return Err(io::Error::from_raw_os_error(code as i32)),
        }
    }
    Err(io::Error::other(
        "the adapter list kept growing while it was read",
    ))
}

fn walk_adapters(first: *const IP_ADAPTER_ADDRESSES_LH) -> Vec<Adapter> {
    let mut adapters = Vec::new();
    let mut next = first;
    while !next.is_null() {
        let adapter = unsafe { next.read_unaligned() };
        adapters.push(Adapter {
            up: adapter.OperStatus == IfOperStatusUp,
            servers: walk_dns_servers(adapter.FirstDnsServerAddress),
        });
        next = adapter.Next;
    }
    adapters
}

fn walk_dns_servers(first: *const IP_ADAPTER_DNS_SERVER_ADDRESS_XP) -> Vec<Ipv4Addr> {
    let mut servers = Vec::new();
    let mut next = first;
    while !next.is_null() {
        let server = unsafe { next.read_unaligned() };
        servers.extend(read_ipv4(server.Address));
        next = server.Next;
    }
    servers
}

pub(super) fn read_ipv4(address: SOCKET_ADDRESS) -> Option<Ipv4Addr> {
    let holds_a_sockaddr = usize::try_from(address.iSockaddrLength)
        .is_ok_and(|length| length >= size_of::<SOCKADDR>());
    if address.lpSockaddr.is_null() || !holds_a_sockaddr {
        return None;
    }
    let sockaddr = unsafe { address.lpSockaddr.read_unaligned() };
    ipv4_from(sockaddr)
}

fn ipv4_from(sockaddr: SOCKADDR) -> Option<Ipv4Addr> {
    if sockaddr.sa_family != AF_INET {
        return None;
    }
    let [_, _, a, b, c, d, ..] = sockaddr.sa_data.map(|byte| byte as u8);
    Some(Ipv4Addr::new(a, b, c, d))
}
