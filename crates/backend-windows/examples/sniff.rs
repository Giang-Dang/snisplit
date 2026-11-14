//! Milestone 0 spike: print the SNI of outbound TLS ClientHellos.
//!
//! Sniff mode only: packets are copied, never held or changed, so this is safe
//! to run next to other WinDivert users. It never installs the driver; it
//! attaches to one that is already loaded. Needs admin rights and
//! `WinDivert.dll` on the DLL search path.
//!
//! Usage: sniff [seconds, default 30]

#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use snisplit_backend_windows::{Handle, flags};
    use snisplit_core::{packet, tls};
    use std::time::Duration;

    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let filter = "outbound and tcp.DstPort == 443 and tcp.PayloadLength > 0 \
                  and tcp.Payload[0] == 0x16";
    let h = Handle::open(
        filter,
        Handle::PRIORITY_HIGHEST,
        flags::SNIFF | flags::RECV_ONLY | flags::NO_INSTALL,
    )
    .inspect_err(|e| eprintln!("WinDivertOpen failed: {e}"))?;
    println!("sniffing for {secs}s: {filter}");

    std::thread::scope(|s| {
        s.spawn(|| {
            std::thread::sleep(Duration::from_secs(secs));
            let _ = h.shutdown();
        });
        let mut buf = vec![0u8; 65535];
        let mut count = 0u32;
        while let Ok((len, _addr)) = h.recv(&mut buf) {
            let Some(p) = packet::parse_tcp(&buf[..len]) else {
                continue;
            };
            let sni = match tls::client_hello_sni(p.payload) {
                Some(r) => String::from_utf8_lossy(&p.payload[r]).into_owned(),
                None => format!("<no SNI in first packet, {} bytes>", p.payload.len()),
            };
            let v = if p.dst.is_ipv6() { 6 } else { 4 };
            println!("ipv{v} {}:{} {sni}", p.dst, p.dst_port);
            count += 1;
        }
        println!("done, {count} ClientHello packets");
    });
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("this example needs Windows");
}
