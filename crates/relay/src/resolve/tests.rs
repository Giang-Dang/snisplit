use std::io;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

#[test]
fn addresses_that_lead_back_to_this_machine_or_nowhere_are_not_usable() {
    let rejected = [
        Ipv4Addr::new(127, 0, 0, 1),
        Ipv4Addr::new(127, 0, 0, 2),
        Ipv4Addr::new(127, 255, 255, 254),
        Ipv4Addr::new(0, 0, 0, 0),
        Ipv4Addr::new(255, 255, 255, 255),
    ];
    for address in rejected {
        assert!(!is_usable_upstream(address), "{address}");
    }
}

#[test]
fn ordinary_and_private_addresses_are_usable() {
    let accepted = [
        Ipv4Addr::new(8, 8, 8, 8),
        Ipv4Addr::new(10, 0, 0, 1),
        Ipv4Addr::new(192, 168, 1, 5),
        Ipv4Addr::new(126, 255, 255, 255),
        Ipv4Addr::new(128, 0, 0, 1),
        Ipv4Addr::new(23, 192, 228, 80),
    ];
    for address in accepted {
        assert!(is_usable_upstream(address), "{address}");
    }
}

struct CountingResolver {
    calls: AtomicUsize,
}

impl Resolve for CountingResolver {
    async fn resolve(&self, host: &str) -> io::Result<Vec<Ipv4Addr>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if host == "known.test" {
            Ok(vec![Ipv4Addr::new(203, 0, 113, 9)])
        } else {
            Ok(Vec::new())
        }
    }
}

#[test]
fn a_shared_resolver_forwards_each_lookup_to_the_inner_one() {
    let inner = Arc::new(CountingResolver {
        calls: AtomicUsize::new(0),
    });
    let shared = Arc::clone(&inner);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime builds");

    let known = runtime.block_on(shared.resolve("known.test")).unwrap();
    let unknown = runtime.block_on(shared.resolve("other.test")).unwrap();

    assert_eq!(known, vec![Ipv4Addr::new(203, 0, 113, 9)]);
    assert!(unknown.is_empty());
    assert_eq!(inner.calls.load(Ordering::SeqCst), 2);
}
