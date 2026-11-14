use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher, RandomState};
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use snisplit_core::dns;
use tokio::net::UdpSocket;
use tokio::time::timeout_at;

use crate::resolve::Resolve;

const CACHE_LIFETIME: Duration = Duration::from_secs(30);
const MAX_DATAGRAM: usize = 4096;

struct CachedAnswer {
    stored_at: Instant,
    addresses: Vec<Ipv4Addr>,
}

pub struct UdpResolver {
    servers: Vec<SocketAddr>,
    timeout: Duration,
    cache: Mutex<HashMap<String, CachedAnswer>>,
    queries_sent: AtomicU64,
    id_source: RandomState,
}

impl UdpResolver {
    pub fn new(servers: Vec<SocketAddr>, timeout: Duration) -> Self {
        Self {
            servers,
            timeout,
            cache: Mutex::new(HashMap::new()),
            queries_sent: AtomicU64::new(0),
            id_source: RandomState::new(),
        }
    }

    fn next_id(&self) -> u16 {
        let mut hasher = self.id_source.build_hasher();
        hasher.write_u64(self.queries_sent.fetch_add(1, Ordering::Relaxed));
        (hasher.finish() & u64::from(u16::MAX)) as u16
    }

    fn cached(&self, key: &str) -> Option<Vec<Ipv4Addr>> {
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let fresh = cache
            .get(key)
            .filter(|entry| entry.stored_at.elapsed() < CACHE_LIFETIME)
            .map(|entry| entry.addresses.clone());
        if fresh.is_none() {
            cache.remove(key);
        }
        fresh
    }

    fn remember(&self, key: String, addresses: &[Ipv4Addr]) {
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.retain(|_, entry| entry.stored_at.elapsed() < CACHE_LIFETIME);
        cache.insert(
            key,
            CachedAnswer {
                stored_at: Instant::now(),
                addresses: addresses.to_vec(),
            },
        );
    }

    async fn ask(
        &self,
        server: SocketAddr,
        id: u16,
        host: &str,
        query: &[u8],
    ) -> Option<Vec<Ipv4Addr>> {
        let socket = UdpSocket::bind(unspecified_like(server)).await.ok()?;
        socket.connect(server).await.ok()?;
        socket.send(query).await.ok()?;
        let deadline = tokio::time::Instant::now() + self.timeout;
        let mut buffer = [0u8; MAX_DATAGRAM];
        loop {
            let length = timeout_at(deadline, socket.recv(&mut buffer))
                .await
                .ok()?
                .ok()?;
            if let Some(addresses) = dns::parse_a_answer(&buffer[..length], id, host) {
                return Some(addresses);
            }
        }
    }
}

fn unspecified_like(peer: SocketAddr) -> SocketAddr {
    match peer {
        SocketAddr::V4(_) => SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
        SocketAddr::V6(_) => SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)),
    }
}

impl Resolve for UdpResolver {
    async fn resolve(&self, host: &str) -> io::Result<Vec<Ipv4Addr>> {
        let id = self.next_id();
        let query = dns::build_a_query(id, host).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{host:?} is not a valid DNS name"),
            )
        })?;
        let key = host.to_ascii_lowercase();
        if let Some(addresses) = self.cached(&key) {
            return Ok(addresses);
        }
        for server in &self.servers {
            if let Some(addresses) = self.ask(*server, id, host, &query).await {
                if !addresses.is_empty() {
                    self.remember(key, &addresses);
                }
                return Ok(addresses);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("no upstream DNS server answered for {host}"),
        ))
    }
}
