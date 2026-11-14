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

use crate::resolve::Resolve;
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
        _resolver: R,
    ) -> io::Result<DnsServer> {
        Self::bind_with(listen, profiles, answer, upstreams, UPSTREAM_TIMEOUT).await
    }

    pub(crate) async fn bind_with(
        listen: SocketAddr,
        profiles: Vec<RelayProfile>,
        answer: Ipv4Addr,
        upstreams: Vec<SocketAddr>,
        upstream_timeout: Duration,
    ) -> io::Result<DnsServer> {
        let socket = UdpSocket::bind(listen).await?;
        let local_addr = socket.local_addr()?;
        let policy = Policy {
            profiles,
            answer,
            upstreams,
            upstream_timeout,
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
        let mut forwards = JoinSet::new();
        let mut buffer = [0u8; MAX_DATAGRAM];
        loop {
            while forwards.try_join_next().is_some() {}
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
            let datagram = &buffer[..length];
            match decide(&self.policy, datagram) {
                Verdict::Ignore => {}
                Verdict::Reply(reply) => {
                    let _ = socket.send_to(&reply, client).await;
                }
                Verdict::Forward => {
                    if forwards.len() < MAX_PENDING_FORWARDS {
                        let query = datagram.to_vec();
                        let policy = Arc::clone(&self.policy);
                        forwards.spawn(forward(Arc::clone(&socket), policy, client, query));
                    }
                }
            }
        }
        forwards.abort_all();
        while forwards.join_next().await.is_some() {}
    }
}

enum Verdict {
    Ignore,
    Reply(Vec<u8>),
    Forward,
}

fn decide(policy: &Policy, datagram: &[u8]) -> Verdict {
    let Some(question) = dns::parse_query(datagram) else {
        return Verdict::Ignore;
    };
    let listed = policy.profiles.iter().any(|profile| {
        profile
            .domains
            .matched_domain(question.name.as_bytes())
            .is_some()
    });
    if !listed {
        return Verdict::Forward;
    }
    let reply = if question.qtype == TYPE_A {
        dns::answer_a(datagram, policy.answer, ANSWER_TTL_SECONDS)
    } else {
        dns::answer_empty(datagram)
    };
    reply.map_or(Verdict::Ignore, Verdict::Reply)
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
