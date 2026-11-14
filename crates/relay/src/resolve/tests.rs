use std::net::Ipv4Addr;

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
