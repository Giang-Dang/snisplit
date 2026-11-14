#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use snisplit_backend_windows::{Handle, flags};
    use snisplit_core::{packet, tls};
    use std::time::Duration;

    const DEFAULT_SECONDS: u64 = 30;
    const OUTBOUND_TLS_HANDSHAKES: &str = "outbound and tcp.DstPort == 443 \
        and tcp.PayloadLength > 0 and tcp.Payload[0] == 0x16";

    let seconds = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(DEFAULT_SECONDS);
    let handle = Handle::open(
        OUTBOUND_TLS_HANDSHAKES,
        Handle::PRIORITY_HIGHEST,
        flags::SNIFF | flags::RECV_ONLY | flags::NO_INSTALL,
    )
    .inspect_err(|e| eprintln!("WinDivertOpen failed: {e}"))?;
    println!("sniffing for {seconds}s: {OUTBOUND_TLS_HANDSHAKES}");

    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_secs(seconds));
            let _ = handle.shutdown();
        });
        let mut buf = vec![0u8; usize::from(u16::MAX)];
        let mut count = 0u32;
        while let Ok((len, _address)) = handle.recv(&mut buf) {
            let Some(tcp) = packet::parse_tcp(&buf[..len]) else {
                continue;
            };
            let sni = match tls::client_hello_sni(tcp.payload) {
                Some(host) => String::from_utf8_lossy(&tcp.payload[host]).into_owned(),
                None => format!("<no SNI in first packet, {} bytes>", tcp.payload.len()),
            };
            let ip_version = if tcp.dst.is_ipv6() { 6 } else { 4 };
            println!("ipv{ip_version} {}:{} {sni}", tcp.dst, tcp.dst_port);
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
