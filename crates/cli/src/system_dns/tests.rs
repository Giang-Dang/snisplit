use std::net::Ipv4Addr;

use super::*;

fn adapter(up: bool, servers: &[Ipv4Addr]) -> Adapter {
    Adapter {
        up,
        servers: servers.to_vec(),
    }
}

#[test]
fn servers_of_up_adapters_are_kept_in_order_without_loopback_or_repeats() {
    let adapters = [
        adapter(false, &[Ipv4Addr::new(10, 0, 0, 1)]),
        adapter(
            true,
            &[
                Ipv4Addr::new(192, 168, 1, 1),
                Ipv4Addr::new(127, 0, 0, 1),
                Ipv4Addr::new(1, 1, 1, 1),
            ],
        ),
        adapter(true, &[]),
        adapter(
            true,
            &[
                Ipv4Addr::new(127, 0, 0, 53),
                Ipv4Addr::new(1, 1, 1, 1),
                Ipv4Addr::new(9, 9, 9, 9),
            ],
        ),
        adapter(false, &[Ipv4Addr::new(8, 8, 8, 8)]),
    ];
    assert_eq!(
        usable_servers(&adapters),
        [
            Ipv4Addr::new(192, 168, 1, 1),
            Ipv4Addr::new(1, 1, 1, 1),
            Ipv4Addr::new(9, 9, 9, 9)
        ]
    );
}

#[test]
fn adapters_that_are_down_give_no_servers() {
    let adapters = [
        adapter(false, &[Ipv4Addr::new(192, 168, 1, 1)]),
        adapter(false, &[Ipv4Addr::new(1, 1, 1, 1)]),
    ];
    assert!(usable_servers(&adapters).is_empty());
}

#[cfg(not(windows))]
#[test]
fn the_lookup_is_unsupported_off_windows() {
    assert_eq!(
        servers().unwrap_err().kind(),
        std::io::ErrorKind::Unsupported
    );
}

#[cfg(windows)]
mod native {
    use std::net::Ipv4Addr;
    use std::process::Command;
    use std::ptr;

    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6, SOCKADDR, SOCKET_ADDRESS};

    use super::super::servers;
    use super::super::windows::{adapters, adapters_from_a_first_buffer_of, read_ipv4};

    const SOCKADDR_BYTES: i32 = 16;
    const UP_ADAPTER_SERVERS: &str = "$ErrorActionPreference = 'Stop'; $up = @(Get-NetAdapter | Where-Object { $_.Status -eq 'Up' } | ForEach-Object { $_.ifIndex }); Get-DnsClientServerAddress -AddressFamily IPv4 | Where-Object { $up -contains $_.InterfaceIndex } | ForEach-Object { $_.ServerAddresses }";

    fn sockaddr(family: u16, port: u16, address: [u8; 4]) -> SOCKADDR {
        let mut data = [0i8; 14];
        let bytes = port.to_be_bytes().into_iter().chain(address);
        for (slot, byte) in data.iter_mut().zip(bytes) {
            *slot = byte as i8;
        }
        SOCKADDR {
            sa_family: family,
            sa_data: data,
        }
    }

    #[test]
    fn an_ipv4_socket_address_gives_its_address_and_not_its_port() {
        let mut held = sockaddr(AF_INET, 53, [192, 0, 2, 200]);
        let address = SOCKET_ADDRESS {
            lpSockaddr: &mut held,
            iSockaddrLength: SOCKADDR_BYTES,
        };
        assert_eq!(read_ipv4(address), Some(Ipv4Addr::new(192, 0, 2, 200)));
    }

    #[test]
    fn other_families_short_lengths_and_null_pointers_give_nothing() {
        let mut ipv6 = sockaddr(AF_INET6, 53, [192, 0, 2, 200]);
        let mut ipv4 = sockaddr(AF_INET, 53, [192, 0, 2, 200]);
        let cases = [
            SOCKET_ADDRESS {
                lpSockaddr: &mut ipv6,
                iSockaddrLength: 28,
            },
            SOCKET_ADDRESS {
                lpSockaddr: &mut ipv4,
                iSockaddrLength: SOCKADDR_BYTES - 1,
            },
            SOCKET_ADDRESS {
                lpSockaddr: &mut ipv4,
                iSockaddrLength: -1,
            },
            SOCKET_ADDRESS {
                lpSockaddr: ptr::null_mut(),
                iSockaddrLength: SOCKADDR_BYTES,
            },
        ];
        for case in cases {
            assert_eq!(read_ipv4(case), None);
        }
    }

    fn powershell_servers() -> Option<Vec<Ipv4Addr>> {
        let output = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                UP_ADAPTER_SERVERS,
            ])
            .output()
            .ok()
            .filter(|output| output.status.success() && output.stderr.is_empty())?;
        let mut found: Vec<Ipv4Addr> = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let address: Ipv4Addr = line.trim().parse().ok()?;
            if !address.is_loopback() && !found.contains(&address) {
                found.push(address);
            }
        }
        Some(found)
    }

    #[test]
    fn the_native_lookup_matches_powershell_on_this_machine() {
        let Some(mut expected) = powershell_servers() else {
            eprintln!("skipped: the PowerShell DNS client cmdlets are not available");
            return;
        };
        let mut found = servers().unwrap();
        expected.sort();
        found.sort();
        assert_eq!(found, expected);
    }

    #[test]
    fn a_first_buffer_too_small_for_one_adapter_grows_and_reads_the_same_list() {
        let grown = adapters_from_a_first_buffer_of(16).unwrap();
        assert!(!grown.is_empty());
        assert_eq!(grown, adapters().unwrap());
    }
}
