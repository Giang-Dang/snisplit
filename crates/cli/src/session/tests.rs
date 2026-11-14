use super::*;

#[test]
fn a_relayed_connection_names_host_port_and_pieces() {
    let event = Event::Forwarded {
        host: "medium.com".to_string(),
        port: 443,
        pieces: 2,
    };
    assert_eq!(describe_event(&event), "relayed medium.com:443 in 2 pieces");
    let single = Event::Forwarded {
        host: "medium.com".to_string(),
        port: 80,
        pieces: 1,
    };
    assert_eq!(describe_event(&single), "relayed medium.com:80 in 1 piece");
}

#[test]
fn an_empty_first_write_says_the_request_head_is_still_arriving() {
    let event = Event::Forwarded {
        host: "cache5-hkg1.steamcontent.com".to_string(),
        port: 80,
        pieces: 0,
    };
    assert_eq!(
        describe_event(&event),
        "relayed cache5-hkg1.steamcontent.com:80 with the request head still arriving"
    );
}

#[test]
fn refusals_say_why_in_words() {
    let cases = [
        (
            Refusal::NoHost,
            "refused a connection on port 80: no host name in the first bytes",
        ),
        (
            Refusal::NotListed,
            "refused a connection on port 80: host is not in the profile",
        ),
        (
            Refusal::TooLong,
            "refused a connection on port 80: first bytes too long without a host name",
        ),
    ];
    for (refusal, expected) in cases {
        assert_eq!(
            describe_event(&Event::Refused { port: 80, refusal }),
            expected
        );
    }
}

#[test]
fn failures_name_the_host_when_it_is_known() {
    let known = Event::Failed {
        host: "store.steampowered.com".to_string(),
        port: 443,
        error: "connecting timed out".to_string(),
    };
    let unknown = Event::Failed {
        host: String::new(),
        port: 27019,
        error: "first flight timed out".to_string(),
    };
    assert_eq!(
        describe_event(&known),
        "failed store.steampowered.com:443: connecting timed out"
    );
    assert_eq!(
        describe_event(&unknown),
        "failed a connection on port 27019: first flight timed out"
    );
}

#[test]
fn a_closed_connection_reports_bytes_respelling_and_why_it_stopped() {
    let plain = Event::Closed {
        host: "medium.com".to_string(),
        port: 80,
        to_server: 210,
        to_client: 4096,
        respelled: 2,
        stopped: None,
    };
    let stopped = Event::Closed {
        host: "store.steampowered.com".to_string(),
        port: 80,
        to_server: 90,
        to_client: 0,
        respelled: 1,
        stopped: Some(StopReason::Upgrade),
    };
    assert_eq!(
        describe_event(&plain),
        "closed medium.com:80: 210 bytes sent, 4096 received, 2 Host headers respelled"
    );
    assert_eq!(
        describe_event(&stopped),
        "closed store.steampowered.com:80: 90 bytes sent, 0 received, 1 Host header respelled; \
         stopped respelling: the request asks for a protocol upgrade"
    );
    let tls = Event::Closed {
        host: "medium.com".to_string(),
        port: 443,
        to_server: 693,
        to_client: 10713,
        respelled: 0,
        stopped: None,
    };
    assert_eq!(
        describe_event(&tls),
        "closed medium.com:443: 693 bytes sent, 10713 received"
    );
}

#[test]
fn a_routing_failure_keeps_only_the_first_line_and_says_what_to_do() {
    let raw = io::Error::other(
        "powershell: Add-DnsClientNrptRule : Failed to load the NRPT information.\nAt line:1 char:88\n+ ...",
    );
    let shown = routing_failure(&raw);
    assert_eq!(
        shown.to_string(),
        "adding DNS rules failed (run from an administrator terminal, or pass --no-routing): \
         powershell: Add-DnsClientNrptRule : Failed to load the NRPT information."
    );
    assert_eq!(shown.kind(), io::ErrorKind::Other);
}

#[test]
fn the_start_line_lists_profiles_addresses_upstreams_and_routing() {
    let selected = plan::select(&["steam", "medium"], Vec::new()).unwrap();
    let upstreams = ["1.1.1.1:53".parse().unwrap(), "1.0.0.1:53".parse().unwrap()];
    assert_eq!(
        describe_start(&selected, &upstreams, true),
        format!(
            "snisplit {VERSION}: relaying steam (14 domains) on 127.0.0.2, \
             medium (1 domain) on 127.0.0.3; upstream DNS 1.1.1.1:53, 1.0.0.1:53; \
             DNS rules added; Ctrl+C to stop"
        )
    );
    assert!(describe_start(&selected, &upstreams, false).contains("; no DNS rules;"));
}

#[test]
fn the_start_line_shows_file_profiles_after_the_built_ins_with_their_own_addresses() {
    let file = profile::parse_file(
        "game.profile",
        "name game\ndomains game.example.com cdn.example.org\n",
    )
    .unwrap();
    let selected = plan::select(&["medium"], vec![file]).unwrap();
    let upstreams = ["1.1.1.1:53".parse().unwrap()];
    assert_eq!(
        describe_start(&selected, &upstreams, false),
        format!(
            "snisplit {VERSION}: relaying medium (1 domain) on 127.0.0.3, \
             game (2 domains) on 127.0.0.4; upstream DNS 1.1.1.1:53; \
             no DNS rules; Ctrl+C to stop"
        )
    );
}

mod start_up;
