use snisplit_core::domains::DomainList;
use snisplit_core::engine::Profile;
use snisplit_core::segment::Order;
use snisplit_core::strategy::{SplitPosition, Strategy};

pub const STEAM_DOMAINS: [&str; 14] = [
    "steampowered.com",
    "steamcommunity.com",
    "steamstatic.com",
    "steamcontent.com",
    "steamserver.net",
    "steam-chat.com",
    "steamgames.com",
    "steamusercontent.com",
    "steamcdn-a.akamaihd.net",
    "steamuserimages-a.akamaihd.net",
    "steamvideo-a.akamaihd.net",
    "steambroadcast.akamaized.net",
    "steambroadcast-test.akamaized.net",
    "valvesoftware.com",
];

pub fn steam() -> Profile {
    Profile {
        domains: DomainList::new(STEAM_DOMAINS),
        tls: Some(strategy(Order::InSequence)),
        http: Some(strategy(Order::Reversed)),
    }
}

fn strategy(order: Order) -> Strategy {
    Strategy {
        order,
        positions: vec![SplitPosition::Offset(1), SplitPosition::DomainMid],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPECTED_DOMAINS: [&str; 14] = [
        "steampowered.com",
        "steamcommunity.com",
        "steamstatic.com",
        "steamcontent.com",
        "steamserver.net",
        "steam-chat.com",
        "steamgames.com",
        "steamusercontent.com",
        "steamcdn-a.akamaihd.net",
        "steamuserimages-a.akamaihd.net",
        "steamvideo-a.akamaihd.net",
        "steambroadcast.akamaized.net",
        "steambroadcast-test.akamaized.net",
        "valvesoftware.com",
    ];

    #[test]
    fn domain_list_is_exactly_the_decided_steam_list() {
        assert_eq!(STEAM_DOMAINS, EXPECTED_DOMAINS);
    }

    #[test]
    fn profile_matches_every_listed_domain_and_its_subdomains() {
        let profile = steam();
        for domain in EXPECTED_DOMAINS {
            let sub = format!("cdn.{domain}");
            assert_eq!(
                profile.domains.matched_domain(domain.as_bytes()),
                Some(0..domain.len()),
                "{domain}"
            );
            assert_eq!(
                profile.domains.matched_domain(sub.as_bytes()),
                Some(4..sub.len()),
                "{sub}"
            );
        }
    }

    #[test]
    fn profile_ignores_unlisted_and_lookalike_hosts() {
        let profile = steam();
        for host in [
            "example.org",
            "notsteampowered.com",
            "akamaihd.net",
            "cdn.akamaihd.net",
            "akamaized.net",
        ] {
            assert_eq!(
                profile.domains.matched_domain(host.as_bytes()),
                None,
                "{host}"
            );
        }
    }

    #[test]
    fn tls_splits_after_the_first_byte_and_mid_domain_in_sequence() {
        let expected = Strategy {
            order: Order::InSequence,
            positions: vec![SplitPosition::Offset(1), SplitPosition::DomainMid],
        };
        assert_eq!(steam().tls, Some(expected));
    }

    #[test]
    fn http_uses_the_same_positions_reversed() {
        let expected = Strategy {
            order: Order::Reversed,
            positions: vec![SplitPosition::Offset(1), SplitPosition::DomainMid],
        };
        assert_eq!(steam().http, Some(expected));
    }
}
