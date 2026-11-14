use std::future::{Future, poll_fn};
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::pin;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use snisplit_core::dns::{self, TYPE_A};
use snisplit_core::first_flight::RelayProfile;
use tokio::net::UdpSocket;

use crate::resolve::{Resolve, ResolveBoxed};
use tokio::task::JoinSet;
use tokio::time::{Instant, timeout_at};

const MAX_DATAGRAM: usize = 4096;
const ANSWER_TTL_SECONDS: u32 = 5;
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_PENDING_FORWARDS: usize = 512;
const ID_LEN: usize = 2;

struct Policy {
    profiles: Vec<RelayProfile>,
    answer: Ipv4Addr,
    upstreams: Vec<SocketAddr>,
    upstream_timeout: Duration,
    resolver: Box<dyn ResolveBoxed>,
}

pub struct DnsServer {
    socket: UdpSocket,
    local_addr: SocketAddr,
    policy: Arc<Policy>,
}

impl DnsServer {
    pub async fn bind<R: Resolve>(
        listen: SocketAddr,
        profiles: Vec<RelayProfile>,
        answer: Ipv4Addr,
        upstreams: Vec<SocketAddr>,
        resolver: R,
    ) -> io::Result<DnsServer> {
        Self::bind_with(
            listen,
            profiles,
            answer,
            upstreams,
            UPSTREAM_TIMEOUT,
            Box::new(resolver),
        )
        .await
    }

    pub(crate) async fn bind_with(
        listen: SocketAddr,
        profiles: Vec<RelayProfile>,
        answer: Ipv4Addr,
        upstreams: Vec<SocketAddr>,
        upstream_timeout: Duration,
        resolver: Box<dyn ResolveBoxed>,
    ) -> io::Result<DnsServer> {
        let socket = UdpSocket::bind(listen).await?;
        let local_addr = socket.local_addr()?;
        let policy = Policy {
            profiles,
            answer,
            upstreams,
            upstream_timeout,
            resolver,
        };
        Ok(DnsServer {
            socket,
            local_addr,
            policy: Arc::new(policy),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub async fn run(self, shutdown: impl Future<Output = ()> + Send) {
        let socket = Arc::new(self.socket);
        let mut shutdown = pin!(shutdown);
        let mut tasks = JoinSet::new();
        let mut buffer = [0u8; MAX_DATAGRAM];
        loop {
            while tasks.try_join_next().is_some() {}
            let received = {
                let mut receive = pin!(socket.recv_from(&mut buffer));
                poll_fn(|context| {
                    if shutdown.as_mut().poll(context).is_ready() {
                        return Poll::Ready(None);
                    }
                    receive.as_mut().poll(context).map(Some)
                })
                .await
            };
            let Some(received) = received else { break };
            let Ok((length, client)) = received else {
                continue;
            };
            let query = &buffer[..length];
            let Some(verdict) = decide(&self.policy, query) else {
                continue;
            };
            if tasks.len() >= MAX_PENDING_FORWARDS {
                continue;
            }
            let socket = Arc::clone(&socket);
            let policy = Arc::clone(&self.policy);
            let query = query.to_vec();
            match verdict {
                Verdict::Forward => {
                    tasks.spawn(forward(socket, policy, client, query));
                }
                Verdict::Listed { name, qtype } => {
                    tasks.spawn(answer_listed(socket, policy, client, query, name, qtype));
                }
            }
        }
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }
}

enum Verdict {
    Forward,
    Listed { name: String, qtype: u16 },
}

fn decide(policy: &Policy, datagram: &[u8]) -> Option<Verdict> {
    let question = dns::parse_query(datagram)?;
    let listed = policy.profiles.iter().any(|profile| {
        profile
            .domains
            .matched_domain(question.name.as_bytes())
            .is_some()
    });
    if listed {
        Some(Verdict::Listed {
            name: question.name,
            qtype: question.qtype,
        })
    } else {
        Some(Verdict::Forward)
    }
}

async fn answer_listed(
    client_socket: Arc<UdpSocket>,
    policy: Arc<Policy>,
    client: SocketAddr,
    query: Vec<u8>,
    name: String,
    qtype: u16,
) {
    let has_ipv4 = policy
        .resolver
        .resolve_boxed(&name)
        .await
        .is_ok_and(|addresses| !addresses.is_empty());
    if !has_ipv4 {
        forward(client_socket, policy, client, query).await;
        return;
    }
    let reply = if qtype == TYPE_A {
        dns::answer_a(&query, policy.answer, ANSWER_TTL_SECONDS)
    } else {
        dns::answer_empty(&query)
    };
    if let Some(reply) = reply {
        let _ = client_socket.send_to(&reply, client).await;
    }
}

async fn forward(
    client_socket: Arc<UdpSocket>,
    policy: Arc<Policy>,
    client: SocketAddr,
    query: Vec<u8>,
) {
    for upstream in &policy.upstreams {
        if let Some(reply) = ask(*upstream, &query, policy.upstream_timeout).await {
            let _ = client_socket.send_to(&reply, client).await;
            return;
        }
    }
}

async fn ask(upstream: SocketAddr, query: &[u8], patience: Duration) -> Option<Vec<u8>> {
    let socket = UdpSocket::bind(unspecified_like(upstream)).await.ok()?;
    socket.connect(upstream).await.ok()?;
    socket.send(query).await.ok()?;
    let deadline = Instant::now() + patience;
    let mut buffer = [0u8; MAX_DATAGRAM];
    loop {
        let length = timeout_at(deadline, socket.recv(&mut buffer))
            .await
            .ok()?
            .ok()?;
        if buffer[..length].get(..ID_LEN) == query.get(..ID_LEN) {
            return Some(buffer[..length].to_vec());
        }
    }
}

fn unspecified_like(peer: SocketAddr) -> SocketAddr {
    match peer {
        SocketAddr::V4(_) => SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
        SocketAddr::V6(_) => SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)),
    }
}

#[cfg(test)]
mod tests;
