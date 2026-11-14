use super::*;

fn ip(a: u8, b: u8, c: u8, d: u8) -> Ipv4Addr {
    Ipv4Addr::new(a, b, c, d)
}

fn listened(config: &Config) -> Vec<(SocketAddr, u16, Protocol, usize)> {
    config
        .listeners
        .iter()
        .map(|l| (l.listen, l.upstream_port, l.protocol, l.profile))
        .collect()
}

#[test]
fn each_built_in_profile_keeps_its_own_address_whatever_is_selected() {
    let both: Vec<_> = select(&["steam", "medium"])
        .iter()
        .map(|s| (s.built_in.name, s.address))
        .collect();
    assert_eq!(
        both,
        vec![("steam", ip(127, 0, 0, 2)), ("medium", ip(127, 0, 0, 3))]
    );
    let medium_only: Vec<_> = select(&["medium"])
        .iter()
        .map(|s| (s.built_in.name, s.address))
        .collect();
    assert_eq!(medium_only, vec![("medium", ip(127, 0, 0, 3))]);
    assert!(select(&[]).is_empty());
}

#[test]
fn medium_listens_for_tls_on_443_and_http_on_80_at_its_address() {
    let config = relay_config(&select(&["medium"]));
    assert_eq!(
        listened(&config),
        vec![
            ("127.0.0.3:443".parse().unwrap(), 443, Protocol::Tls, 0),
            ("127.0.0.3:80".parse().unwrap(), 80, Protocol::Http, 0),
        ]
    );
    assert_eq!(config.profiles.len(), 1);
    assert_eq!(config.profiles[0].name, "medium");
}

#[test]
fn every_listener_points_at_its_own_profile_and_ports_never_clash_across_addresses() {
    let config = relay_config(&select(&["steam", "medium"]));
    assert_eq!(config.listeners.len(), 1 + 16 + 1 + 2);
    for listener in &config.listeners {
        let owner = &config.profiles[listener.profile];
        let expected_address = if owner.name == "steam" {
            ip(127, 0, 0, 2)
        } else {
            ip(127, 0, 0, 3)
        };
        assert_eq!(listener.listen.ip(), expected_address, "{listener:?}");
        assert_eq!(listener.listen.port(), listener.upstream_port);
    }
    let mut addresses: Vec<_> = config.listeners.iter().map(|l| l.listen).collect();
    addresses.sort();
    addresses.dedup();
    assert_eq!(addresses.len(), config.listeners.len());
}

#[test]
fn relay_limits_are_the_planned_values() {
    let config = relay_config(&select(&["medium"]));
    assert_eq!(config.piece_gap, Duration::from_millis(30));
    assert_eq!(config.first_flight_timeout, Duration::from_secs(10));
    assert_eq!(config.max_first_flight, 16384);
    assert_eq!(config.connect_timeout, Duration::from_secs(10));
}

#[test]
fn explicit_upstreams_win_over_the_system_ones() {
    assert_eq!(
        upstream_servers(&[ip(9, 9, 9, 9)], &[ip(1, 1, 1, 1)]),
        vec!["9.9.9.9:53".parse::<SocketAddr>().unwrap()]
    );
}

#[test]
fn system_upstreams_lose_loopback_unspecified_and_duplicates_in_order() {
    assert_eq!(
        upstream_servers(
            &[],
            &[
                ip(127, 0, 0, 2),
                ip(1, 1, 1, 1),
                ip(0, 0, 0, 0),
                ip(192, 168, 1, 1),
                ip(1, 1, 1, 1),
                ip(127, 0, 0, 1),
            ]
        ),
        vec![
            "1.1.1.1:53".parse::<SocketAddr>().unwrap(),
            "192.168.1.1:53".parse().unwrap(),
        ]
    );
    assert!(upstream_servers(&[], &[ip(127, 0, 0, 1)]).is_empty());
}
