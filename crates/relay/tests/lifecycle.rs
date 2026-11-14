mod common;

use std::io;
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;

use common::{
    Answer, FakeResolver, Harness, LOOPBACK, STORE_HELLO, UNUSED_UPSTREAM_PORT, block_on, config,
    listener, single_tls_config,
};
use snisplit_core::first_flight::{Protocol, Refusal};
use snisplit_relay::{Event, Relay};
use tokio::io::AsyncWriteExt;

const STEAM: &[&str] = &["steampowered.com"];
const OTHER_LIST: &[&str] = &["medium.com"];
const NO_SUCH_PROFILE: usize = 5;

fn free_port() -> u16 {
    let listener = TcpListener::bind((LOOPBACK, 0)).unwrap();
    listener.local_addr().unwrap().port()
}

fn ignore_events() -> Arc<dyn Fn(Event) + Send + Sync> {
    Arc::new(|_| {})
}

#[test]
fn binding_a_port_twice_fails_and_leaves_nothing_bound() {
    block_on(async {
        let port = free_port();
        let listeners = vec![
            listener(port, 1, Protocol::Tls),
            listener(port, 2, Protocol::Tls),
        ];
        let resolver = FakeResolver {
            answer: Answer::Failure("unused"),
        };

        let result = Relay::bind(config(listeners, STEAM), resolver, ignore_events()).await;

        let error = result.err().expect("the second bind fails");
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        TcpListener::bind((LOOPBACK, port)).expect("the first listener was released");
    });
}

#[test]
fn a_port_held_by_another_process_fails_the_bind_of_the_whole_relay() {
    block_on(async {
        let occupant = TcpListener::bind((LOOPBACK, 0)).unwrap();
        let occupied = occupant.local_addr().unwrap().port();
        let free = free_port();
        let listeners = vec![
            listener(free, 1, Protocol::Tls),
            listener(occupied, 2, Protocol::Http),
        ];
        let resolver = FakeResolver {
            answer: Answer::Failure("unused"),
        };

        let result = Relay::bind(config(listeners, STEAM), resolver, ignore_events()).await;

        let error = result.err().expect("the occupied port fails the bind");
        assert!(
            error
                .to_string()
                .starts_with(&format!("binding 127.0.0.1:{occupied} failed: ")),
            "{error}"
        );
        TcpListener::bind((LOOPBACK, free)).expect("the free port was released");
    });
}

#[test]
fn a_listener_naming_a_missing_profile_is_rejected_before_binding() {
    block_on(async {
        let port = free_port();
        let mut bad = listener(port, 1, Protocol::Tls);
        bad.profile = NO_SUCH_PROFILE;
        let resolver = FakeResolver {
            answer: Answer::Failure("unused"),
        };

        let result = Relay::bind(config(vec![bad], STEAM), resolver, ignore_events()).await;

        let error = result.err().expect("the profile index is invalid");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        TcpListener::bind((LOOPBACK, port)).expect("nothing was bound");
    });
}

#[test]
fn every_listener_serves_its_own_route_and_is_reported_in_order() {
    block_on(async {
        let listeners = vec![
            listener(0, 1111, Protocol::Tls),
            listener(0, 2222, Protocol::Tls),
        ];
        let mut harness =
            Harness::start(config(listeners, OTHER_LIST), Answer::Failure("no")).await;
        assert_eq!(harness.addrs.len(), 2);
        assert!(harness.addrs.iter().all(|addr| addr.port() != 0));
        assert_ne!(harness.addrs[0], harness.addrs[1]);

        let mut second = harness.connect_to(1).await;
        second.write_all(STORE_HELLO).await.unwrap();
        assert_eq!(
            harness.next_event().await,
            Event::Refused {
                port: 2222,
                refusal: Refusal::NotListed
            }
        );
        let mut first = harness.connect_to(0).await;
        first.write_all(STORE_HELLO).await.unwrap();
        assert_eq!(
            harness.next_event().await,
            Event::Refused {
                port: 1111,
                refusal: Refusal::NotListed
            }
        );
        harness.shut_down().await;
    });
}

#[test]
fn shutdown_ends_run_serves_until_then_and_releases_the_port() {
    block_on(async {
        let mut harness =
            Harness::start(single_tls_config(OTHER_LIST), Answer::Failure("no")).await;
        let addr: SocketAddr = harness.addrs[0];
        let mut client = harness.connect().await;
        client.write_all(STORE_HELLO).await.unwrap();
        assert_eq!(
            harness.next_event().await,
            Event::Refused {
                port: UNUSED_UPSTREAM_PORT,
                refusal: Refusal::NotListed
            }
        );
        drop(client);

        harness.shut_down().await;

        TcpListener::bind(addr).expect("the relay released its port");
    });
}
