use std::future::Future;
use std::net::SocketAddr;

use tokio::net::TcpListener;

use super::*;

const PATIENCE: Duration = Duration::from_secs(30);

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime builds")
        .block_on(future)
}

fn closed_addresses<const N: usize>() -> [SocketAddr; N] {
    let listeners: Vec<std::net::TcpListener> = (0..N)
        .map(|_| std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap())
        .collect();
    let addresses: Vec<SocketAddr> = listeners
        .iter()
        .map(|listener| listener.local_addr().unwrap())
        .collect();
    addresses.try_into().unwrap()
}

#[test]
fn the_next_candidate_is_tried_when_the_first_refuses() {
    block_on(async {
        let live = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let live_address = live.local_addr().unwrap();
        let [closed] = closed_addresses();
        let candidates = [closed, live_address];

        let stream = connect_first(&candidates, PATIENCE).await.unwrap();

        assert_eq!(stream.peer_addr().unwrap(), live_address);
    });
}

#[test]
fn when_every_candidate_refuses_the_error_names_the_last_one() {
    block_on(async {
        let [first, last] = closed_addresses();

        let error = connect_first(&[first, last], PATIENCE).await.unwrap_err();

        assert!(error.contains(&last.to_string()), "{error}");
    });
}
