use std::hash::{BuildHasher, Hasher, RandomState};
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use snisplit_core::first_flight::Protocol;
use snisplit_core::probe::{
    Assessment, Attempt, AttemptRecord, FirstFlight, HelloRandom, Outcome, RANDOM_LEN, assess,
    attempts, first_flight, reply_outcome,
};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::{Instant, timeout, timeout_at};

use crate::pieces::write_pieces;
use crate::resolve::{Resolve, is_usable_upstream};

pub const TLS_PORT: u16 = 443;
pub const HTTP_PORT: u16 = 80;

const READ_CHUNK: usize = 1024;
const WORD_LEN: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeSettings {
    pub runs: usize,
    pub timeout: Duration,
    pub piece_gap: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeTarget {
    pub domain: String,
    pub address: Ipv4Addr,
    pub tls_port: u16,
    pub http_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    pub domain: String,
    pub address: Ipv4Addr,
    pub records: Vec<AttemptRecord>,
    pub assessment: Assessment,
}

pub async fn probe<R: Resolve>(
    resolver: &R,
    domain: &str,
    settings: &ProbeSettings,
) -> io::Result<ProbeReport> {
    let answers = resolver.resolve(domain).await?;
    let address = answers
        .iter()
        .copied()
        .find(|&address| is_usable_upstream(address))
        .ok_or_else(|| no_usable_address(domain, &answers))?;
    let target = ProbeTarget {
        domain: domain.to_owned(),
        address,
        tls_port: TLS_PORT,
        http_port: HTTP_PORT,
    };
    probe_target(&target, settings).await
}

fn no_usable_address(domain: &str, answers: &[Ipv4Addr]) -> io::Error {
    let message = if answers.is_empty() {
        format!("{domain} has no IPv4 address")
    } else {
        format!(
            "{domain} resolved only to loopback, unspecified or broadcast addresses: {answers:?}"
        )
    };
    io::Error::new(io::ErrorKind::NotFound, message)
}

pub async fn probe_target(
    target: &ProbeTarget,
    settings: &ProbeSettings,
) -> io::Result<ProbeReport> {
    let mut records: Vec<AttemptRecord> = attempts()
        .into_iter()
        .map(|attempt| AttemptRecord {
            attempt,
            outcomes: Vec::with_capacity(settings.runs),
        })
        .collect();
    for _ in 0..settings.runs {
        for record in &mut records {
            let flight = first_flight(&record.attempt, &target.domain, &fresh_random())
                .ok_or_else(|| not_probeable(&target.domain))?;
            let port = port_for(target, &record.attempt);
            let address = SocketAddr::from((target.address, port));
            let protocol = record.attempt.protocol();
            let outcome = attempt_once(address, protocol, &flight, settings).await;
            record.outcomes.push(outcome);
        }
    }
    let assessment = assess(&target.domain, &records);
    Ok(ProbeReport {
        domain: target.domain.clone(),
        address: target.address,
        records,
        assessment,
    })
}

fn not_probeable(domain: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{domain:?} cannot be sent as a TLS server name or HTTP host"),
    )
}

fn port_for(target: &ProbeTarget, attempt: &Attempt) -> u16 {
    match attempt.protocol() {
        Protocol::Tls => target.tls_port,
        Protocol::Http => target.http_port,
    }
}

async fn attempt_once(
    address: SocketAddr,
    protocol: Protocol,
    flight: &FirstFlight,
    settings: &ProbeSettings,
) -> Outcome {
    let mut stream = match timeout(settings.timeout, TcpStream::connect(address)).await {
        Err(_) => return Outcome::ConnectFailed(io::ErrorKind::TimedOut),
        Ok(Err(error)) => return Outcome::ConnectFailed(error.kind()),
        Ok(Ok(stream)) => stream,
    };
    if let Err(error) = stream.set_nodelay(true) {
        return Outcome::Failed(error.kind());
    }
    let written = write_pieces(&mut stream, &flight.bytes, &flight.cuts, settings.piece_gap).await;
    if let Err(error) = written {
        return Outcome::of_stream_error(error.kind());
    }
    first_reply(&mut stream, protocol, settings.timeout).await
}

async fn first_reply(stream: &mut TcpStream, protocol: Protocol, wait: Duration) -> Outcome {
    let deadline = Instant::now() + wait;
    let mut received = Vec::new();
    let mut chunk = [0u8; READ_CHUNK];
    loop {
        let read = match timeout_at(deadline, stream.read(&mut chunk)).await {
            Err(_) if received.is_empty() => return Outcome::TimedOut,
            Err(_) => return reply_outcome(protocol, &received, true).unwrap_or(Outcome::TimedOut),
            Ok(Err(error)) => return Outcome::of_stream_error(error.kind()),
            Ok(Ok(read)) => read,
        };
        let ended = read == 0;
        received.extend_from_slice(&chunk[..read]);
        if let Some(outcome) = reply_outcome(protocol, &received, ended) {
            return outcome;
        }
        if ended {
            return Outcome::Closed;
        }
    }
}

fn fresh_random() -> HelloRandom {
    HelloRandom {
        random: random_block(),
        session_id: random_block(),
        key_share: random_block(),
    }
}

fn random_block() -> [u8; RANDOM_LEN] {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let mut block = [0u8; RANDOM_LEN];
    for (index, word) in block.chunks_exact_mut(WORD_LEN).enumerate() {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u128(since_epoch.as_nanos());
        hasher.write_usize(index);
        word.copy_from_slice(&hasher.finish().to_be_bytes());
    }
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_hello_gets_its_own_unpredictable_secrets() {
        let first = fresh_random();
        let second = fresh_random();
        assert_ne!(first.random, second.random);
        assert_ne!(first.key_share, second.key_share);
        assert_ne!(first.random, first.session_id);
        assert_ne!(first.session_id, first.key_share);
        assert_ne!(
            first.random[..WORD_LEN],
            first.random[WORD_LEN..2 * WORD_LEN]
        );
    }
}
