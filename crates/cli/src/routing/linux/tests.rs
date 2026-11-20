use std::io::ErrorKind;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

use super::*;

const SERVER: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 2);

const DUMMY_LINK_DETAILS: &str = "3: snisplit0: <BROADCAST,NOARP> mtu 1500 qdisc noop state DOWN mode DEFAULT group default qlen 1000\\    link/ether ba:22:5e:5c:56:4e brd ff:ff:ff:ff:ff:ff promiscuity 0  allmulti 0 minmtu 0 maxmtu 0 \\    dummy addrgenmode eui64 numtxqueues 1 numrxqueues 1 gso_max_size 65536 gso_max_segs 65535 tso_max_size 65536 tso_max_segs 65535 gro_max_size 65536 \n";
const VETH_LINK_DETAILS: &str = "5: snisplitv@snisplitw: <BROADCAST,MULTICAST,M-DOWN> mtu 1500 qdisc noop state DOWN mode DEFAULT group default qlen 1000\\    link/ether 82:d1:6a:cd:04:54 brd ff:ff:ff:ff:ff:ff promiscuity 0  allmulti 0 minmtu 68 maxmtu 65535 \\    veth addrgenmode eui64 numtxqueues 28 numrxqueues 28 gso_max_size 65536 gso_max_segs 65535 tso_max_size 524280 tso_max_segs 65535 gro_max_size 65536 \n";
const LOOPBACK_LINK_DETAILS: &str = "1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536 qdisc noqueue state UNKNOWN mode DEFAULT group default qlen 1000\\    link/loopback 00:00:00:00:00:00 brd 00:00:00:00:00:00 promiscuity 0  allmulti 0 minmtu 0 maxmtu 0 addrgenmode eui64 numtxqueues 1 numrxqueues 1 gso_max_size 65536 gso_max_segs 65535 tso_max_size 524280 tso_max_segs 65535 gro_max_size 65536 \n";

fn rule<'a>(suffixes: &[&'a str], server: Ipv4Addr) -> ProfileRule<'a> {
    ProfileRule {
        suffixes: suffixes.to_vec(),
        server,
    }
}

fn spelled(invocations: &[Invocation]) -> Vec<String> {
    invocations
        .iter()
        .map(|invocation| format!("{} {}", invocation.tool.name(), invocation.args.join(" ")))
        .collect()
}

#[test]
fn one_rule_makes_a_dummy_link_with_an_address_the_servers_and_domains_and_flushes() {
    let rules = [rule(&["steampowered.com", "steam-chat.com"], SERVER)];
    assert_eq!(
        spelled(&start_invocations(&rules).unwrap()),
        [
            "ip link add snisplit0 type dummy",
            "ip addr add 169.254.53.1/32 dev snisplit0",
            "ip route add local 127.0.0.2/32 dev snisplit0 table local",
            "ip link set snisplit0 up",
            "resolvectl dns snisplit0 127.0.0.2",
            "resolvectl domain snisplit0 ~steampowered.com ~steam-chat.com",
            "resolvectl default-route snisplit0 no",
            "resolvectl llmnr snisplit0 no",
            "resolvectl mdns snisplit0 no",
            "resolvectl dnsovertls snisplit0 no",
            "resolvectl dnssec snisplit0 no",
            "resolvectl flush-caches",
        ]
    );
}

#[test]
fn the_second_rule_gets_the_next_link_name_and_the_next_address() {
    let rules = [
        rule(&["a.com"], SERVER),
        rule(&["medium.com"], Ipv4Addr::new(127, 0, 0, 3)),
    ];
    let spelled = spelled(&start_invocations(&rules).unwrap());
    assert_eq!(spelled.len(), 23);
    assert_eq!(spelled[11], "ip link add snisplit1 type dummy");
    assert_eq!(spelled[12], "ip addr add 169.254.53.2/32 dev snisplit1");
    assert_eq!(
        spelled[13],
        "ip route add local 127.0.0.3/32 dev snisplit1 table local"
    );
    assert_eq!(spelled[15], "resolvectl dns snisplit1 127.0.0.3");
    assert_eq!(spelled[16], "resolvectl domain snisplit1 ~medium.com");
    assert_eq!(spelled[22], "resolvectl flush-caches");
}

#[test]
fn arguments_are_separate_words_not_a_shell_string() {
    let invocations = start_invocations(&[rule(&["a.com", "b.org"], SERVER)]).unwrap();
    assert_eq!(
        invocations[5],
        Invocation {
            tool: Tool::Resolvectl,
            args: vec![
                "domain".to_string(),
                "snisplit0".to_string(),
                "~a.com".to_string(),
                "~b.org".to_string()
            ]
        }
    );
}

#[test]
fn an_empty_rule_list_and_a_rule_without_suffixes_are_invalid_input() {
    assert_eq!(
        start_invocations(&[]).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
    let one_empty = [rule(&["a.com"], SERVER), rule(&[], SERVER)];
    assert_eq!(
        start_invocations(&one_empty).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
}

#[test]
fn rejected_suffixes_never_reach_an_invocation() {
    for rejected in [
        "A.com", "a b.com", "a..com", "a.com~", "~a.com", "", "-x.com\n",
    ] {
        let rules = [rule(&["fine.com", rejected], SERVER)];
        let error = start_invocations(&rules).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput, "{rejected:?}");
    }
}

#[test]
fn a_rejected_name_in_a_later_rule_stops_everything_before_any_invocation_is_built() {
    let rules = [rule(&["fine.com"], SERVER), rule(&["Bad.com"], SERVER)];
    assert_eq!(
        start_invocations(&rules).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
}

#[test]
fn at_most_254_rules_fit_in_the_address_block() {
    let rules: Vec<ProfileRule> = (0..254).map(|_| rule(&["a.com"], SERVER)).collect();
    let spelled = spelled(&start_invocations(&rules).unwrap());
    assert_eq!(spelled.len(), 254 * 11 + 1);
    assert_eq!(
        spelled[253 * 11 + 1],
        "ip addr add 169.254.53.254/32 dev snisplit253"
    );
    let too_many: Vec<ProfileRule> = (0..255).map(|_| rule(&["a.com"], SERVER)).collect();
    assert_eq!(
        start_invocations(&too_many).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
}

#[test]
fn link_names_stay_within_the_kernel_limit_of_15_bytes() {
    let longest = link_for(253).unwrap();
    assert_eq!(longest.name, "snisplit253");
    assert!(longest.name.len() <= 15);
}

#[test]
fn the_stale_link_removal_and_listing_use_these_command_lines() {
    assert_eq!(
        spelled(&[delete_invocation("snisplit3")]),
        ["ip link del snisplit3"]
    );
    assert_eq!(
        spelled(&[show_invocation("snisplit3")]),
        ["ip -d -o link show dev snisplit3"]
    );
    assert_eq!(spelled(&[flush_invocation()]), ["resolvectl flush-caches"]);
    assert_eq!(spelled(&[status_invocation()]), ["resolvectl status"]);
}

#[test]
fn only_names_starting_with_snisplit_are_snisplit_links_and_they_come_sorted() {
    let names = [
        "eth0",
        "snisplit1",
        "lo",
        "snisplit0",
        "my-snisplit",
        "snisplitv",
        "Snisplit2",
    ];
    assert_eq!(
        snisplit_link_names(names.iter().map(|name| name.to_string()).collect()),
        ["snisplit0", "snisplit1", "snisplitv"]
    );
}

#[test]
fn a_dummy_link_is_told_from_a_veth_pair_and_the_loopback_in_real_ip_output() {
    assert!(is_dummy_link(DUMMY_LINK_DETAILS));
    assert!(!is_dummy_link(VETH_LINK_DETAILS));
    assert!(!is_dummy_link(LOOPBACK_LINK_DETAILS));
    assert!(!is_dummy_link(""));
}

#[test]
fn the_word_dummy_elsewhere_in_the_output_does_not_make_a_link_a_dummy() {
    let aliased = "7: snisplit9: <BROADCAST,MULTICAST> mtu 1500 qdisc noop state DOWN mode DEFAULT group default qlen 1000\\    link/ether 82:d1:6a:cd:04:54 brd ff:ff:ff:ff:ff:ff promiscuity 0 minmtu 68 maxmtu 65535 \\    veth addrgenmode eui64 numtxqueues 1 \\    altname dummy\n";
    assert!(!is_dummy_link(aliased));
}

const STATUS_OF_ROOT: &str = "Name:\tsnisplit\nUmask:\t0022\nState:\tR (running)\nUid:\t1000\t0\t0\t0\nGid:\t1000\t1000\t1000\t1000\n";
const STATUS_OF_USER: &str = "Name:\tsnisplit\nUid:\t0\t1000\t0\t0\nGid:\t0\t0\t0\t0\n";

#[test]
fn the_effective_uid_is_the_second_field_of_the_uid_line() {
    assert_eq!(effective_uid(STATUS_OF_ROOT), Some(0));
    assert_eq!(effective_uid(STATUS_OF_USER), Some(1000));
    assert_eq!(effective_uid("Name:\tx\n"), None);
    assert_eq!(effective_uid("Uid:\t5\n"), None);
    assert_eq!(effective_uid("Uid:\t5\tabc\t5\t5\n"), None);
}

#[test]
fn only_an_effective_uid_of_zero_may_start() {
    assert!(check_root(STATUS_OF_ROOT).is_ok());
    let refused = check_root(STATUS_OF_USER).unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::PermissionDenied);
    assert!(refused.to_string().contains("sudo"), "{refused}");
    assert_eq!(
        check_root("Name:\tx\n").unwrap_err().kind(),
        ErrorKind::InvalidData
    );
}

#[test]
fn a_stub_nameserver_line_reaches_the_stub_without_any_link() {
    let text = "# Generated by NetworkManager\nsearch lan\nnameserver 127.0.0.53\noptions edns0 trust-ad\n";
    assert!(resolv_conf_reaches_stub(text, None));
}

#[test]
fn other_nameservers_commented_stub_lines_and_empty_text_do_not_reach_the_stub() {
    assert!(!resolv_conf_reaches_stub("nameserver 192.168.65.7\n", None));
    assert!(!resolv_conf_reaches_stub("nameserver 127.0.0.1\n", None));
    assert!(!resolv_conf_reaches_stub("# nameserver 127.0.0.53\n", None));
    assert!(!resolv_conf_reaches_stub("nameserver 127.0.0.530\n", None));
    assert!(!resolv_conf_reaches_stub("", None));
}

#[test]
fn a_link_to_the_stub_file_reaches_the_stub_whatever_the_text_says() {
    assert!(resolv_conf_reaches_stub(
        "",
        Some("../run/systemd/resolve/stub-resolv.conf")
    ));
    assert!(resolv_conf_reaches_stub(
        "nameserver 10.0.0.1\n",
        Some("/run/systemd/resolve/stub-resolv.conf")
    ));
}

#[test]
fn a_link_to_another_file_does_not_reach_the_stub() {
    assert!(!resolv_conf_reaches_stub(
        "nameserver 10.0.0.1\n",
        Some("../run/systemd/resolve/resolv.conf")
    ));
    assert!(!resolv_conf_reaches_stub(
        "",
        Some("../run/NetworkManager/resolv.conf")
    ));
}

#[test]
fn link_targets_are_resolved_against_etc_lexically() {
    assert_eq!(
        resolved_link_target("../run/systemd/resolve/stub-resolv.conf"),
        "/run/systemd/resolve/stub-resolv.conf"
    );
    assert_eq!(
        resolved_link_target("/run/./systemd//resolve/stub-resolv.conf"),
        "/run/systemd/resolve/stub-resolv.conf"
    );
    assert_eq!(
        resolved_link_target("resolvconf/run/resolv.conf"),
        "/etc/resolvconf/run/resolv.conf"
    );
    assert_eq!(resolved_link_target("../../../../x"), "/x");
}

#[test]
fn a_tool_is_looked_up_in_the_fixed_folders_in_order_and_never_through_path() {
    let present = |wanted: &'static [&'static str]| {
        move |path: &Path| wanted.iter().any(|item| Path::new(item) == path)
    };
    assert_eq!(
        tool_path(
            Tool::Ip,
            present(&["/bin/ip", "/usr/bin/ip", "/usr/sbin/ip"])
        )
        .unwrap(),
        PathBuf::from("/usr/sbin/ip")
    );
    assert_eq!(
        tool_path(Tool::Ip, present(&["/bin/ip", "/sbin/ip"])).unwrap(),
        PathBuf::from("/sbin/ip")
    );
    assert_eq!(
        tool_path(Tool::Resolvectl, present(&["/usr/bin/resolvectl"])).unwrap(),
        PathBuf::from("/usr/bin/resolvectl")
    );
    assert_eq!(
        tool_path(
            Tool::Resolvectl,
            present(&["/opt/bin/resolvectl", "/usr/local/bin/resolvectl"])
        )
        .unwrap_err()
        .kind(),
        ErrorKind::NotFound
    );
}

#[test]
fn a_missing_tool_names_the_tool_and_the_folders_searched() {
    let error = tool_path(Tool::Resolvectl, |_| false).unwrap_err();
    assert_eq!(
        error.to_string(),
        "resolvectl not found in /usr/sbin, /usr/bin, /sbin, /bin"
    );
}

#[test]
fn a_clean_exit_without_stderr_returns_stdout() {
    assert_eq!(
        tool_outcome(Tool::Ip, true, b"line\n", b"").unwrap(),
        "line\n"
    );
}

#[test]
fn a_failure_names_the_tool_and_the_first_line_of_stderr_only() {
    let error = tool_outcome(
        Tool::Ip,
        false,
        b"ignored",
        b"RTNETLINK answers: File exists\nsecond line\n",
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "ip: RTNETLINK answers: File exists");
}

#[test]
fn stderr_with_a_zero_exit_is_still_an_error_and_leading_blank_lines_are_skipped() {
    let error =
        tool_outcome(Tool::Resolvectl, true, b"", b"\n  \nFailed to set DNS\n").unwrap_err();
    assert_eq!(error.to_string(), "resolvectl: Failed to set DNS");
}

#[test]
fn a_failure_without_a_message_is_still_an_error() {
    let error = tool_outcome(Tool::Resolvectl, false, b"", b"").unwrap_err();
    assert_eq!(error.to_string(), "resolvectl failed without a message");
}

#[test]
fn begin_start_validates_names_and_rule_count_before_any_system_check() {
    let bad_name = begin_start(&[rule(&["A.com"], SERVER)]);
    assert_eq!(
        bad_name.err().map(|error| error.kind()),
        Some(ErrorKind::InvalidInput)
    );
    let none = begin_start(&[]);
    assert_eq!(
        none.err().map(|error| error.kind()),
        Some(ErrorKind::InvalidInput)
    );
}

#[test]
fn abandoning_a_start_changes_nothing_and_succeeds() {
    let pending = PendingStart {
        invocations: Vec::new(),
    };
    assert!(pending.abandon().is_ok());
}
