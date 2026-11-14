#[cfg(windows)]
mod recorder {
    use snisplit_core::tls;
    use std::collections::HashMap;
    use std::fs;
    use std::io;
    use std::net::IpAddr;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    pub const OUTBOUND_TLS_PAYLOADS: &str =
        "outbound and tcp.DstPort == 443 and tcp.PayloadLength > 0";

    const MAX_PACKETS_PER_FLOW: usize = 4;
    const FLOW_IDLE_TIMEOUT: Duration = Duration::from_secs(2);
    const TLS_HANDSHAKE_RECORD: u8 = 0x16;
    const TLS_RECORD_HEADER_LEN: usize = 5;
    const TLS_RECORD_LENGTH_OFFSET: usize = 3;
    const IPV6_HEADER_LEN: usize = 40;
    const BYTES_PER_WORD: usize = 4;
    const TCP_SEQUENCE_OFFSET: usize = 4;

    pub type FlowKey = (IpAddr, u16, IpAddr, u16);

    pub fn tcp_sequence_number(packet: &[u8]) -> Option<u32> {
        let first = *packet.first()?;
        let ip_header_len = match first >> 4 {
            4 => usize::from(first & 0x0f) * BYTES_PER_WORD,
            6 => IPV6_HEADER_LEN,
            _ => return None,
        };
        let start = ip_header_len + TCP_SEQUENCE_OFFSET;
        let bytes = packet.get(start..start + 4)?;
        Some(u32::from_be_bytes(bytes.try_into().ok()?))
    }

    struct Segment {
        sequence: u32,
        payload: Vec<u8>,
    }

    struct Flow {
        segments: Vec<Segment>,
        last_seen: Instant,
    }

    impl Flow {
        fn new(now: Instant) -> Self {
            Self {
                segments: Vec::new(),
                last_seen: now,
            }
        }

        fn contains(&self, sequence: u32) -> bool {
            self.segments.iter().any(|s| s.sequence == sequence)
        }

        fn add(&mut self, sequence: u32, payload: &[u8], now: Instant) {
            self.segments.push(Segment {
                sequence,
                payload: payload.to_vec(),
            });
            self.last_seen = now;
        }

        fn in_sequence_order(&self) -> Vec<&Segment> {
            let first = self.segments[0].sequence;
            let mut ordered: Vec<&Segment> = self.segments.iter().collect();
            ordered.sort_by_key(|s| s.sequence.wrapping_sub(first));
            ordered
        }

        fn stream(&self) -> Vec<u8> {
            self.in_sequence_order()
                .iter()
                .flat_map(|s| s.payload.iter().copied())
                .collect()
        }

        fn is_complete(&self) -> bool {
            self.segments.len() >= MAX_PACKETS_PER_FLOW || covers_record(&self.stream())
        }
    }

    fn covers_record(stream: &[u8]) -> bool {
        let Some(length) = stream.get(TLS_RECORD_LENGTH_OFFSET..TLS_RECORD_HEADER_LEN) else {
            return false;
        };
        let record_len = usize::from(u16::from_be_bytes([length[0], length[1]]));
        stream.len() >= TLS_RECORD_HEADER_LEN + record_len
    }

    fn is_plain_host_name(host: &str) -> bool {
        host.split('.').all(|label| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
    }

    fn matches_suffix(host: &str, suffix: &str) -> bool {
        host == suffix
            || host
                .strip_suffix(suffix)
                .is_some_and(|rest| rest.ends_with('.'))
    }

    pub struct Recorder {
        out_dir: PathBuf,
        suffixes: Vec<String>,
        flows: HashMap<FlowKey, Flow>,
        saved: u32,
    }

    impl Recorder {
        pub fn new(out_dir: PathBuf, suffixes: &[String]) -> Self {
            Self {
                out_dir,
                suffixes: suffixes.iter().map(|s| s.to_ascii_lowercase()).collect(),
                flows: HashMap::new(),
                saved: 0,
            }
        }

        pub fn saved(&self) -> u32 {
            self.saved
        }

        pub fn on_packet(
            &mut self,
            key: FlowKey,
            sequence: u32,
            payload: &[u8],
            now: Instant,
        ) -> io::Result<()> {
            let starts_handshake = payload.first() == Some(&TLS_HANDSHAKE_RECORD);
            if self.flows.contains_key(&key) || starts_handshake {
                let flow = self.flows.entry(key).or_insert_with(|| Flow::new(now));
                if flow.contains(sequence) {
                    return Ok(());
                }
                flow.add(sequence, payload, now);
                if flow.is_complete() {
                    if let Some(flow) = self.flows.remove(&key) {
                        self.finish(&flow)?;
                    }
                }
            }
            self.finish_idle(now)
        }

        pub fn finish_all(&mut self) -> io::Result<()> {
            let flows: Vec<Flow> = self.flows.drain().map(|(_, flow)| flow).collect();
            flows.iter().try_for_each(|flow| self.finish(flow))
        }

        fn finish_idle(&mut self, now: Instant) -> io::Result<()> {
            let idle: Vec<FlowKey> = self
                .flows
                .iter()
                .filter(|(_, flow)| now.duration_since(flow.last_seen) >= FLOW_IDLE_TIMEOUT)
                .map(|(key, _)| *key)
                .collect();
            for key in idle {
                if let Some(flow) = self.flows.remove(&key) {
                    self.finish(&flow)?;
                }
            }
            Ok(())
        }

        fn allowlisted_host(&self, stream: &[u8]) -> Option<String> {
            let range = tls::client_hello_sni(stream)?;
            let host = String::from_utf8(stream[range].to_vec())
                .ok()?
                .to_ascii_lowercase();
            let allowed =
                is_plain_host_name(&host) && self.suffixes.iter().any(|s| matches_suffix(&host, s));
            allowed.then_some(host)
        }

        fn finish(&mut self, flow: &Flow) -> io::Result<()> {
            let stream = flow.stream();
            let Some(host) = self.allowlisted_host(&stream) else {
                return Ok(());
            };
            let index = (1..)
                .find(|n| {
                    !self.path(&host, *n, "bin").exists() && !self.path(&host, *n, "txt").exists()
                })
                .unwrap_or(u32::MAX);
            let lengths: String = flow
                .in_sequence_order()
                .iter()
                .map(|s| format!("{}\n", s.payload.len()))
                .collect();
            let packets = flow.segments.len();
            self.write_file(&host, index, "bin", &stream, packets, stream.len())?;
            self.write_file(
                &host,
                index,
                "txt",
                lengths.as_bytes(),
                packets,
                stream.len(),
            )?;
            self.saved += 1;
            Ok(())
        }

        fn path(&self, host: &str, index: u32, extension: &str) -> PathBuf {
            self.out_dir.join(format!("{host}-{index}.{extension}"))
        }

        fn write_file(
            &self,
            host: &str,
            index: u32,
            extension: &str,
            contents: &[u8],
            packets: usize,
            total_bytes: usize,
        ) -> io::Result<()> {
            fs::write(self.path(host, index, extension), contents)?;
            println!("{host}-{index}.{extension} packets={packets} bytes={total_bytes}");
            Ok(())
        }
    }
}

#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use recorder::{OUTBOUND_TLS_PAYLOADS, Recorder, tcp_sequence_number};
    use snisplit_backend_windows::{Handle, flags};
    use snisplit_core::packet;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    const USAGE: &str = "usage: capture <out_dir> <seconds> <allowlisted domain suffix>...";

    let args: Vec<String> = std::env::args().skip(1).collect();
    let (out_dir, seconds, suffixes) = match args.as_slice() {
        [out_dir, seconds, suffixes @ ..] if !suffixes.is_empty() => match seconds.parse::<u64>() {
            Ok(seconds) => (PathBuf::from(out_dir), seconds, suffixes),
            Err(_) => {
                eprintln!("{USAGE}");
                std::process::exit(2);
            }
        },
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    std::fs::create_dir_all(&out_dir)?;
    let handle = Handle::open(
        OUTBOUND_TLS_PAYLOADS,
        Handle::PRIORITY_HIGHEST,
        flags::SNIFF | flags::RECV_ONLY | flags::NO_INSTALL,
    )
    .inspect_err(|e| eprintln!("WinDivertOpen failed: {e}"))?;
    println!("capturing for {seconds}s into {}", out_dir.display());

    let mut recorder = Recorder::new(out_dir, suffixes);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_secs(seconds));
            let _ = handle.shutdown();
        });
        let mut buf = vec![0u8; usize::from(u16::MAX)];
        while let Ok((len, _address)) = handle.recv(&mut buf) {
            let packet_bytes = &buf[..len];
            let Some(tcp) = packet::parse_tcp(packet_bytes) else {
                continue;
            };
            let Some(sequence) = tcp_sequence_number(packet_bytes) else {
                continue;
            };
            let key = (tcp.src, tcp.src_port, tcp.dst, tcp.dst_port);
            recorder.on_packet(key, sequence, tcp.payload, Instant::now())?;
        }
        recorder.finish_all()
    })?;
    println!("done, {} ClientHellos saved", recorder.saved());
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("this example needs Windows");
}
