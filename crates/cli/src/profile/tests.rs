use super::*;

const EXPECTED_STEAM_DOMAINS: [&str; 14] = [
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
fn steam_domain_list_is_exactly_the_decided_list() {
    assert_eq!(STEAM_DOMAINS, EXPECTED_STEAM_DOMAINS);
}

#[test]
fn steam_profile_matches_every_listed_domain_and_its_subdomains() {
    let profile = steam();
    for domain in EXPECTED_STEAM_DOMAINS {
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
fn steam_profile_ignores_unlisted_and_lookalike_hosts() {
    let profile = steam();
    for host in [
        "example.org",
        "notsteampowered.com",
        "akamaihd.net",
        "cdn.akamaihd.net",
        "akamaized.net",
        "medium.com",
    ] {
        assert_eq!(
            profile.domains.matched_domain(host.as_bytes()),
            None,
            "{host}"
        );
    }
}

#[test]
fn steam_cuts_after_the_first_byte_and_mid_domain_as_measured() {
    assert_eq!(
        steam().tls_positions,
        vec![SplitPosition::Offset(1), SplitPosition::DomainMid]
    );
}

#[test]
fn steam_listens_on_https_and_the_client_range_without_its_streaming_port() {
    let ports = steam().tls_ports;
    let mut expected = vec![443];
    expected.extend(27015..=27030);
    assert_eq!(ports, expected);
    for observed_cm_port in [443, 27018, 27019, 27020, 27024] {
        assert!(ports.contains(&observed_cm_port), "{observed_cm_port}");
    }
    assert!(!ports.contains(&27036));
    assert_eq!(steam().http_ports, vec![80]);
}

#[test]
fn medium_profile_matches_medium_and_its_subdomains_only() {
    let profile = medium();
    for (host, expected) in [
        ("medium.com", Some(0..10)),
        ("www.medium.com", Some(4..14)),
        ("help.medium.com", Some(5..15)),
        ("notmedium.com", None),
        ("medium.com.example", None),
        ("store.steampowered.com", None),
    ] {
        assert_eq!(
            profile.domains.matched_domain(host.as_bytes()),
            expected,
            "{host}"
        );
    }
}

#[test]
fn medium_cuts_only_after_the_first_byte_on_https_and_relays_http() {
    let profile = medium();
    assert_eq!(profile.tls_positions, vec![SplitPosition::Offset(1)]);
    assert_eq!(profile.tls_ports, vec![443]);
    assert_eq!(profile.http_ports, vec![80]);
}

#[test]
fn built_in_profiles_are_found_by_name_and_carry_their_own_list() {
    assert_eq!(find("steam").map(|b| b.domains.len()), Some(14));
    assert_eq!(find("medium").map(|b| b.domains), Some(&["medium.com"][..]));
    assert_eq!(
        find("steam").map(|b| (b.profile)().name),
        Some("steam".to_string())
    );
    assert!(find("Steam").is_none());
    assert!(find("other").is_none());
}
