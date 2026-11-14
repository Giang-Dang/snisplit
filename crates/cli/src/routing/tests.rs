use std::io::ErrorKind;
use std::net::Ipv4Addr;

use super::*;

const SERVER: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 2);

#[test]
fn install_script_adds_dotted_and_bare_rule_per_suffix() {
    let script = install_script(&["medium.com", "steam-chat.com"], SERVER).unwrap();
    assert_eq!(
        script,
        "$ErrorActionPreference = 'Stop'; foreach ($namespace in '.medium.com', 'medium.com', '.steam-chat.com', 'steam-chat.com') { Add-DnsClientNrptRule -Namespace $namespace -NameServers '127.0.0.2' -Comment 'snisplit' | Out-Null }"
    );
}

#[test]
fn install_script_formats_server_address() {
    let script = install_script(&["a.com"], Ipv4Addr::new(10, 1, 2, 3)).unwrap();
    assert_eq!(
        script,
        "$ErrorActionPreference = 'Stop'; foreach ($namespace in '.a.com', 'a.com') { Add-DnsClientNrptRule -Namespace $namespace -NameServers '10.1.2.3' -Comment 'snisplit' | Out-Null }"
    );
}

#[test]
fn install_rejects_empty_list() {
    let error = install_script(&[], SERVER).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
}

#[test]
fn rejected_names_never_reach_a_script() {
    let too_long_label = "a".repeat(64);
    let label_63 = "a".repeat(63);
    let too_long_name = [label_63.as_str(); 4].join(".");
    let rejected = [
        "",
        ".medium.com",
        "medium.com.",
        "a..com",
        "Medium.com",
        "medium.com'; Remove-Item C:\\ #",
        "a b.com",
        "a_b.com",
        "caf\u{e9}.com",
        "a.com\n",
        too_long_label.as_str(),
        too_long_name.as_str(),
    ];
    assert_eq!(too_long_name.len(), 255);
    for name in rejected {
        let error = install_script(&["good.com", name], SERVER).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput, "name {name:?}");
    }
}

#[test]
fn accepted_names_at_the_limits() {
    let label_63 = "a".repeat(63);
    let last_label = "a".repeat(61);
    let name_253 = [
        label_63.as_str(),
        label_63.as_str(),
        label_63.as_str(),
        last_label.as_str(),
    ]
    .join(".");
    assert_eq!(name_253.len(), 253);
    let name_254 = format!("{name_253}a");
    for name in [label_63.as_str(), name_253.as_str(), "0-9.x--y", "a"] {
        assert!(install_script(&[name], SERVER).is_ok(), "name {name:?}");
    }
    assert!(install_script(&[name_254.as_str()], SERVER).is_err());
}

#[test]
fn remove_script_filters_on_comment() {
    assert_eq!(
        remove_script(),
        "$ErrorActionPreference = 'Stop'; Get-DnsClientNrptRule | Where-Object { $_.Comment -eq 'snisplit' } | Remove-DnsClientNrptRule -Force"
    );
}

#[test]
fn count_script_counts_only_comment_matches() {
    assert_eq!(
        count_script(),
        "$ErrorActionPreference = 'Stop'; @(Get-DnsClientNrptRule | Where-Object { $_.Comment -eq 'snisplit' }).Count"
    );
}

#[test]
fn flush_script_clears_cache() {
    assert_eq!(
        flush_script(),
        "$ErrorActionPreference = 'Stop'; Clear-DnsClientCache"
    );
}

#[test]
fn dns_servers_script_restricts_to_up_adapters() {
    assert_eq!(
        dns_servers_script(),
        "$ErrorActionPreference = 'Stop'; $up = @(Get-NetAdapter | Where-Object { $_.Status -eq 'Up' } | ForEach-Object { $_.ifIndex }); Get-DnsClientServerAddress -AddressFamily IPv4 | Where-Object { $up -contains $_.InterfaceIndex } | ForEach-Object { $_.ServerAddresses }"
    );
}

#[test]
fn parse_count_reads_windows_line_ending() {
    assert_eq!(parse_count("28\r\n").unwrap(), 28);
    assert_eq!(parse_count("0\r\n").unwrap(), 0);
}

#[test]
fn parse_count_rejects_garbage() {
    assert_eq!(parse_count("").unwrap_err().kind(), ErrorKind::InvalidData);
    assert_eq!(
        parse_count("three").unwrap_err().kind(),
        ErrorKind::InvalidData
    );
}

#[test]
fn parse_dns_servers_reads_real_output() {
    let real = "1.1.1.1\r\n1.0.0.1\r\n";
    assert_eq!(
        parse_dns_servers(real).unwrap(),
        [Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(1, 0, 0, 1)]
    );
}

#[test]
fn parse_dns_servers_drops_loopback_and_duplicates_keeping_order() {
    let output = "192.168.1.1\n127.0.0.1\n1.1.1.1\n127.0.0.2\n192.168.1.1\n\n1.1.1.1\n";
    assert_eq!(
        parse_dns_servers(output).unwrap(),
        [Ipv4Addr::new(192, 168, 1, 1), Ipv4Addr::new(1, 1, 1, 1)]
    );
}

#[test]
fn parse_dns_servers_empty_output_is_empty_list() {
    assert!(parse_dns_servers("").unwrap().is_empty());
    assert!(parse_dns_servers("\r\n\r\n").unwrap().is_empty());
}

#[test]
fn parse_dns_servers_rejects_non_address_line() {
    let error = parse_dns_servers("1.1.1.1\nfe80::1\n").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidData);
}

#[test]
fn outcome_success_returns_stdout() {
    assert_eq!(interpret_outcome(true, b"5\r\n", b"").unwrap(), "5\r\n");
}

#[test]
fn outcome_failure_carries_powershell_message() {
    let error = interpret_outcome(false, b"", b"Access is denied.\r\n").unwrap_err();
    assert_eq!(error.to_string(), "powershell: Access is denied.");
}

#[test]
fn outcome_stderr_with_zero_exit_is_still_an_error() {
    let error = interpret_outcome(true, b"ok", b"warning text").unwrap_err();
    assert_eq!(error.to_string(), "powershell: warning text");
}

#[test]
fn outcome_failure_without_message_is_still_an_error() {
    let error = interpret_outcome(false, b"out", b"").unwrap_err();
    assert_eq!(
        error.to_string(),
        "powershell: powershell failed without a message"
    );
}

#[cfg(not(windows))]
#[test]
fn operations_are_unsupported_off_windows() {
    assert_eq!(remove_all().unwrap_err().kind(), ErrorKind::Unsupported);
    assert_eq!(count().unwrap_err().kind(), ErrorKind::Unsupported);
    assert_eq!(flush_cache().unwrap_err().kind(), ErrorKind::Unsupported);
    assert_eq!(
        system_dns_servers().unwrap_err().kind(),
        ErrorKind::Unsupported
    );
    assert_eq!(
        install(&["a.com"], SERVER).unwrap_err().kind(),
        ErrorKind::Unsupported
    );
}

#[cfg(not(windows))]
#[test]
fn install_validates_names_before_platform_check() {
    assert_eq!(
        install(&["A.com"], SERVER).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
}
