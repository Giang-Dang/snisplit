mod common;

use std::net::Ipv4Addr;
use std::time::Duration;

use common::{
    API_HELLO, Answer, COMMUNITY_HELLO, Harness, STORE_HELLO, STORE_HOST, UNUSED_UPSTREAM_PORT,
    block_on, config, listener, reaches_end_of_stream, single_tls_config,
};
use snisplit_core::first_flight::{Protocol, Refusal};
use snisplit_relay::Event;
use tokio::io::AsyncWriteExt;
use tokio::time::sleep;

const STEAM: &[&str] = &["steampowered.com", "steamcommunity.com"];
const OTHER_LIST: &[&str] = &["medium.com"];
const HTTP_REQUEST: &[u8] = b"GET / HTTP/1.1\r\nHost: store.steampowered.com\r\n\r\n";
const UNLISTED_HTTP_REQUEST: &[u8] = b"GET / HTTP/1.1\r\nHost: example.org\r\n\r\n";
const NOTHING_LISTENS_HERE: u16 = 1;
const TRICKLE_INTERVAL: Duration = Duration::from_millis(100);
const SHORT_FIRST_FLIGHT_TIMEOUT: Duration = Duration::from_millis(400);
const CAP_BELOW_THE_SNI: usize = 1000;
const NO_ANSWER: Answer = Answer::Addresses(Vec::new());

fn refused(port: u16, refusal: Refusal) -> Event {
    Event::Refused { port, refusal }
}

#[test]
fn a_hello_for_an_unlisted_name_is_refused_closed_and_never_resolved() {
    block_on(async {
        let config = single_tls_config(OTHER_LIST);
        let mut harness = Harness::start(config, Answer::Failure("must not resolve")).await;
        let mut first = harness.connect().await;
        first.write_all(STORE_HELLO).await.unwrap();
        assert_eq!(
            harness.next_event().await,
            refused(UNUSED_UPSTREAM_PORT, Refusal::NotListed)
        );
        assert!(reaches_end_of_stream(&mut first).await);

        let mut second = harness.connect().await;
        second.write_all(API_HELLO).await.unwrap();
        assert_eq!(
            harness.next_event().await,
            refused(UNUSED_UPSTREAM_PORT, Refusal::NotListed)
        );

        let mut events = harness.shut_down().await;
        assert!(events.try_recv().is_err());
    });
}

#[test]
fn bytes_that_are_not_for_the_listeners_protocol_are_refused_as_having_no_host() {
    block_on(async {
        let listeners = vec![
            listener(0, 1111, Protocol::Tls),
            listener(0, 2222, Protocol::Http),
        ];
        let mut harness = Harness::start(config(listeners, STEAM), Answer::Failure("no")).await;

        let mut plain_http_on_tls = harness.connect_to(0).await;
        plain_http_on_tls.write_all(HTTP_REQUEST).await.unwrap();
        assert_eq!(harness.next_event().await, refused(1111, Refusal::NoHost));

        let mut hello_on_http = harness.connect_to(1).await;
        hello_on_http.write_all(STORE_HELLO).await.unwrap();
        assert_eq!(harness.next_event().await, refused(2222, Refusal::NoHost));

        let mut unlisted_on_http = harness.connect_to(1).await;
        unlisted_on_http
            .write_all(UNLISTED_HTTP_REQUEST)
            .await
            .unwrap();
        assert_eq!(
            harness.next_event().await,
            refused(2222, Refusal::NotListed)
        );
        harness.shut_down().await;
    });
}

#[test]
fn a_client_that_sends_nothing_is_failed_by_the_first_flight_timeout() {
    block_on(async {
        let mut config = single_tls_config(STEAM);
        config.first_flight_timeout = SHORT_FIRST_FLIGHT_TIMEOUT;
        let mut harness = Harness::start(config, Answer::Failure("no")).await;
        let mut client = harness.connect().await;

        match harness.next_event().await {
            Event::Failed { host, port, .. } => {
                assert_eq!(host, "");
                assert_eq!(port, UNUSED_UPSTREAM_PORT);
            }
            other => panic!("expected a failure, got {other:?}"),
        }
        assert!(reaches_end_of_stream(&mut client).await);
        harness.shut_down().await;
    });
}

#[test]
fn a_client_trickling_bytes_is_cut_off_by_the_total_first_flight_timeout() {
    block_on(async {
        let mut config = single_tls_config(STEAM);
        config.first_flight_timeout = SHORT_FIRST_FLIGHT_TIMEOUT;
        let mut harness = Harness::start(config, Answer::Failure("no")).await;
        let mut client = harness.connect().await;
        let trickle = tokio::spawn(async move {
            for byte in STORE_HELLO {
                if client.write_all(&[*byte]).await.is_err() {
                    break;
                }
                sleep(TRICKLE_INTERVAL).await;
            }
        });

        assert!(matches!(harness.next_event().await, Event::Failed { .. }));
        trickle.abort();
        harness.shut_down().await;
    });
}

#[test]
fn a_hello_whose_name_lies_beyond_the_cap_is_refused_as_too_long() {
    block_on(async {
        let mut config = single_tls_config(STEAM);
        config.max_first_flight = CAP_BELOW_THE_SNI;
        let mut harness = Harness::start(config, Answer::Failure("no")).await;
        let mut client = harness.connect().await;
        client.write_all(STORE_HELLO).await.unwrap();

        assert_eq!(
            harness.next_event().await,
            refused(UNUSED_UPSTREAM_PORT, Refusal::TooLong)
        );
        harness.shut_down().await;
    });
}

#[test]
fn a_hello_exactly_as_long_as_the_cap_is_still_accepted() {
    block_on(async {
        let mut config = single_tls_config(STEAM);
        config.max_first_flight = COMMUNITY_HELLO.len();
        let mut harness = Harness::start(config, Answer::Failure("resolver says no")).await;
        let mut client = harness.connect().await;
        client.write_all(COMMUNITY_HELLO).await.unwrap();

        match harness.next_event().await {
            Event::Failed { host, error, .. } => {
                assert_eq!(host, "steamcommunity.com");
                assert!(error.contains("resolver says no"), "{error}");
            }
            other => panic!("expected the resolver failure, got {other:?}"),
        }
        harness.shut_down().await;
    });
}

#[test]
fn a_resolver_failure_is_reported_with_the_host_and_closes_the_client() {
    block_on(async {
        let mut harness = Harness::start(
            single_tls_config(STEAM),
            Answer::Failure("upstream dns unreachable"),
        )
        .await;
        let mut client = harness.connect().await;
        client.write_all(STORE_HELLO).await.unwrap();

        match harness.next_event().await {
            Event::Failed { host, port, error } => {
                assert_eq!(host, STORE_HOST);
                assert_eq!(port, UNUSED_UPSTREAM_PORT);
                assert!(error.contains("upstream dns unreachable"), "{error}");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
        assert!(reaches_end_of_stream(&mut client).await);
        harness.shut_down().await;
    });
}

#[test]
fn answers_that_lead_back_to_this_machine_or_nowhere_are_failures_not_connections() {
    let unusable_answers = [
        vec![Ipv4Addr::new(127, 0, 0, 1)],
        vec![Ipv4Addr::new(127, 0, 0, 2)],
        vec![Ipv4Addr::new(0, 0, 0, 0)],
        vec![Ipv4Addr::new(255, 255, 255, 255)],
        vec![Ipv4Addr::new(127, 0, 0, 1), Ipv4Addr::new(0, 0, 0, 0)],
    ];
    for addresses in unusable_answers {
        block_on(async {
            let decoy = tokio::net::TcpListener::bind((common::LOOPBACK, 0))
                .await
                .unwrap();
            let decoy_port = decoy.local_addr().unwrap().port();
            let config = config(vec![listener(0, decoy_port, Protocol::Tls)], STEAM);
            let mut harness = Harness::start(config, Answer::Addresses(addresses.clone())).await;
            let mut client = harness.connect().await;
            client.write_all(STORE_HELLO).await.unwrap();

            match harness.next_event().await {
                Event::Failed { host, port, .. } => {
                    assert_eq!(host, STORE_HOST, "{addresses:?}");
                    assert_eq!(port, decoy_port);
                }
                other => panic!("{addresses:?}: expected a failure, got {other:?}"),
            }
            assert!(reaches_end_of_stream(&mut client).await);
            harness.shut_down().await;
            let connected = decoy.accept();
            assert!(
                tokio::time::timeout(Duration::from_millis(50), connected)
                    .await
                    .is_err(),
                "{addresses:?}: the relay connected to the decoy"
            );
        });
    }
}

#[test]
fn an_empty_answer_is_a_failure() {
    block_on(async {
        let config = config(
            vec![listener(0, NOTHING_LISTENS_HERE, Protocol::Tls)],
            STEAM,
        );
        let mut harness = Harness::start(config, NO_ANSWER).await;
        let mut client = harness.connect().await;
        client.write_all(STORE_HELLO).await.unwrap();

        assert!(matches!(
            harness.next_event().await,
            Event::Failed {
                port: NOTHING_LISTENS_HERE,
                ..
            }
        ));
        harness.shut_down().await;
    });
}
