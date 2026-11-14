use snisplit_core::first_flight::HttpMethod;
use snisplit_core::strategy::SplitPosition;

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

fn file_spec(name: &str, domains: &[&str]) -> ProfileSpec {
    ProfileSpec {
        name: name.to_string(),
        domains: domains.iter().map(|domain| domain.to_string()).collect(),
        tls_ports: vec![443],
        tls_positions: vec![SplitPosition::Offset(1)],
        http_ports: vec![80],
        http_method: HttpMethod::HostSpell,
    }
}

fn addressed(selected: &[Selected]) -> Vec<(&str, Ipv4Addr)> {
    selected
        .iter()
        .map(|s| (s.profile.name.as_str(), s.address))
        .collect()
}

#[test]
fn each_built_in_profile_keeps_its_own_address_whatever_is_selected() {
    let both = select(&["steam", "medium"], vec![]).unwrap();
    assert_eq!(
        addressed(&both),
        vec![("steam", ip(127, 0, 0, 2)), ("medium", ip(127, 0, 0, 3))]
    );
    let medium_only = select(&["medium"], vec![]).unwrap();
    assert_eq!(addressed(&medium_only), vec![("medium", ip(127, 0, 0, 3))]);
    assert!(select(&[], vec![]).unwrap().is_empty());
}

#[test]
fn files_get_the_next_addresses_in_the_order_given_whatever_built_ins_are_selected() {
    let files = || {
        vec![
            file_spec("alpha", &["alpha.example.com"]),
            file_spec("beta", &["beta.example.com"]),
        ]
    };
    assert_eq!(
        addressed(&select(&[], files()).unwrap()),
        vec![("alpha", ip(127, 0, 0, 4)), ("beta", ip(127, 0, 0, 5))]
    );
    assert_eq!(
        addressed(&select(&["medium"], files()).unwrap()),
        vec![
            ("medium", ip(127, 0, 0, 3)),
            ("alpha", ip(127, 0, 0, 4)),
            ("beta", ip(127, 0, 0, 5)),
        ]
    );
    assert_eq!(
        addressed(&select(&["steam", "medium"], files()).unwrap()),
        vec![
            ("steam", ip(127, 0, 0, 2)),
            ("medium", ip(127, 0, 0, 3)),
            ("alpha", ip(127, 0, 0, 4)),
            ("beta", ip(127, 0, 0, 5)),
        ]
    );
}

#[test]
fn built_ins_come_in_built_in_order_before_files_whatever_the_order_of_the_names() {
    let selected = select(
        &["medium", "steam"],
        vec![file_spec("alpha", &["a.example.com"])],
    )
    .unwrap();
    assert_eq!(
        addressed(&selected),
        vec![
            ("steam", ip(127, 0, 0, 2)),
            ("medium", ip(127, 0, 0, 3)),
            ("alpha", ip(127, 0, 0, 4)),
        ]
    );
}

#[test]
fn the_same_domain_in_two_selected_profiles_is_an_error_naming_both() {
    let clash = select(
        &[],
        vec![
            file_spec("alpha", &["a.example.com", "shared.example.com"]),
            file_spec("beta", &["shared.example.com"]),
        ],
    );
    assert_eq!(
        clash,
        Err(SelectionError::DuplicateDomain {
            domain: "shared.example.com".to_string(),
            first: "alpha".to_string(),
            second: "beta".to_string(),
        })
    );
    let with_built_in = select(&["medium"], vec![file_spec("alpha", &["medium.com"])]);
    assert_eq!(
        with_built_in,
        Err(SelectionError::DuplicateDomain {
            domain: "medium.com".to_string(),
            first: "medium".to_string(),
            second: "alpha".to_string(),
        })
    );
}

#[test]
fn a_domain_of_a_built_in_that_is_not_selected_is_free_for_a_file() {
    let selected = select(&["steam"], vec![file_spec("alpha", &["medium.com"])]).unwrap();
    assert_eq!(
        addressed(&selected),
        vec![("steam", ip(127, 0, 0, 2)), ("alpha", ip(127, 0, 0, 4))]
    );
}

#[test]
fn two_files_with_the_same_name_are_an_error_even_with_different_domains() {
    let clash = select(
        &[],
        vec![
            file_spec("alpha", &["a.example.com"]),
            file_spec("alpha", &["b.example.com"]),
        ],
    );
    assert_eq!(
        clash,
        Err(SelectionError::DuplicateName("alpha".to_string()))
    );
}

#[test]
fn the_loopback_range_runs_out_after_address_254() {
    let files = |count: usize| -> Vec<ProfileSpec> {
        (0..count)
            .map(|i| file_spec(&format!("p{i}"), &[&format!("d{i}.example.com")]))
            .collect()
    };
    let last_that_fits = select(&[], files(251)).unwrap();
    assert_eq!(last_that_fits[250].address, ip(127, 0, 0, 254));
    assert_eq!(
        select(&[], files(252)),
        Err(SelectionError::TooManyProfiles)
    );
}

#[test]
fn the_error_texts_name_the_profiles_and_domains() {
    assert_eq!(
        SelectionError::DuplicateDomain {
            domain: "x.example.com".to_string(),
            first: "alpha".to_string(),
            second: "beta".to_string(),
        }
        .to_string(),
        "domain x.example.com is in both profile alpha and profile beta"
    );
    assert_eq!(
        SelectionError::DuplicateName("alpha".to_string()).to_string(),
        "two profiles are named alpha"
    );
}

#[test]
fn medium_listens_for_tls_on_443_and_http_on_80_at_its_address() {
    let config = relay_config(&select(&["medium"], vec![]).unwrap());
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
fn a_file_profile_listens_on_its_own_ports_at_its_own_address_with_its_own_profile_index() {
    let mut file = file_spec("alpha", &["a.example.com"]);
    file.tls_ports = vec![8443, 9000];
    file.http_ports = vec![];
    file.http_method = HttpMethod::PassThrough;
    let config = relay_config(&select(&["medium"], vec![file]).unwrap());
    assert_eq!(
        listened(&config),
        vec![
            ("127.0.0.3:443".parse().unwrap(), 443, Protocol::Tls, 0),
            ("127.0.0.3:80".parse().unwrap(), 80, Protocol::Http, 0),
            ("127.0.0.4:8443".parse().unwrap(), 8443, Protocol::Tls, 1),
            ("127.0.0.4:9000".parse().unwrap(), 9000, Protocol::Tls, 1),
        ]
    );
    assert_eq!(config.profiles[1].name, "alpha");
    assert_eq!(config.profiles[1].http_method, HttpMethod::PassThrough);
}

#[test]
fn every_listener_points_at_its_own_profile_and_ports_never_clash_across_addresses() {
    let config = relay_config(&select(&["steam", "medium"], vec![]).unwrap());
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
    let config = relay_config(&select(&["medium"], vec![]).unwrap());
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
