use super::*;
use snisplit_core::first_flight::HttpMethod;
use snisplit_core::strategy::SplitPosition;

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

fn built_in(name: &str) -> ProfileSpec {
    find(name)
        .unwrap_or_else(|| panic!("no built-in {name}"))
        .spec()
        .unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn the_steam_file_parses_to_the_milestone_3_values() {
    let mut tls_ports = vec![443];
    tls_ports.extend(27015..=27030);
    assert_eq!(
        built_in("steam"),
        ProfileSpec {
            name: "steam".to_string(),
            domains: EXPECTED_STEAM_DOMAINS.map(str::to_string).to_vec(),
            tls_ports,
            tls_positions: vec![SplitPosition::Offset(1), SplitPosition::DomainMid],
            http_ports: vec![80],
            http_method: HttpMethod::HostSpell,
        }
    );
}

#[test]
fn the_medium_file_parses_to_the_milestone_3_values() {
    assert_eq!(
        built_in("medium"),
        ProfileSpec {
            name: "medium".to_string(),
            domains: vec!["medium.com".to_string()],
            tls_ports: vec![443],
            tls_positions: vec![SplitPosition::Offset(1)],
            http_ports: vec![80],
            http_method: HttpMethod::HostSpell,
        }
    );
}

#[test]
fn steam_profile_matches_every_listed_domain_and_its_subdomains() {
    let profile = built_in("steam").relay_profile();
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
    let profile = built_in("steam").relay_profile();
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
fn steam_listens_on_https_and_the_client_range_without_its_streaming_port() {
    let spec = built_in("steam");
    for observed_cm_port in [443, 27015, 27018, 27019, 27020, 27024, 27030] {
        assert!(
            spec.tls_ports.contains(&observed_cm_port),
            "{observed_cm_port}"
        );
    }
    for absent in [27014, 27031, 27036, 80] {
        assert!(!spec.tls_ports.contains(&absent), "{absent}");
    }
    assert_eq!(spec.http_ports, vec![80]);
}

#[test]
fn medium_profile_matches_medium_and_its_subdomains_only() {
    let profile = built_in("medium").relay_profile();
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
fn built_in_profiles_are_found_by_exact_name_and_each_file_names_itself() {
    assert_eq!(find("steam").map(|b| b.name), Some("steam"));
    assert_eq!(find("medium").map(|b| b.name), Some("medium"));
    assert!(find("Steam").is_none());
    assert!(find("other").is_none());
    for built_in in &BUILT_IN {
        assert_eq!(
            built_in.spec().map(|spec| spec.name),
            Ok(built_in.name.to_string())
        );
    }
}

#[test]
fn the_embedded_texts_are_the_files_the_names_promise() {
    let steam = find("steam").unwrap().text;
    assert!(steam.starts_with("name steam\ndomains steampowered.com\n"));
    assert!(steam.ends_with(
        "tls-ports 443 27015-27030\ntls-split 1 domain-mid\nhttp-ports 80\nhttp-method host-spell\n"
    ));
    assert_eq!(
        find("medium").unwrap().text,
        "name medium\ndomains medium.com\ntls-ports 443\ntls-split 1\nhttp-ports 80\nhttp-method host-spell\n"
    );
}

#[test]
fn the_embedded_texts_are_plain_ascii_without_comments_or_carriage_returns() {
    for built_in in &BUILT_IN {
        assert!(built_in.text.is_ascii(), "{}", built_in.name);
        assert!(!built_in.text.contains('#'), "{}", built_in.name);
        assert!(!built_in.text.contains('\r'), "{}", built_in.name);
    }
}

struct ScratchDir(std::path::PathBuf);

impl ScratchDir {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("snisplit-profile-test-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_profile_file_on_disk_is_read_and_parsed_even_with_a_byte_order_mark_and_crlf() {
    let dir = ScratchDir::new();
    let path = dir.file(
        "game.profile",
        b"\xef\xbb\xbfname game\r\ndomains game.example.com\r\nhttp-ports none\r\n",
    );
    assert_eq!(
        read_file(&path).unwrap(),
        ProfileSpec {
            name: "game".to_string(),
            domains: vec!["game.example.com".to_string()],
            tls_ports: vec![443],
            tls_positions: vec![SplitPosition::Offset(1)],
            http_ports: vec![],
            http_method: HttpMethod::HostSpell,
        }
    );
}

#[test]
fn a_bad_profile_file_reports_its_path_and_line_as_invalid_data() {
    let dir = ScratchDir::new();
    let path = dir.file(
        "bad.profile",
        b"name game\ndomains good.example.com\ndomains evil'.example.com\n",
    );
    let error = read_file(&path).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(
        error.to_string(),
        format!(
            "{}:3: bad domain \"evil'.example.com\": only a-z, 0-9, '-' and '.' are allowed",
            path.display()
        )
    );
}

#[test]
fn a_missing_or_non_utf8_profile_file_is_an_error_naming_the_path() {
    let dir = ScratchDir::new();
    let missing = dir.0.join("nothing.profile");
    let error = read_file(&missing).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(
        error
            .to_string()
            .starts_with(&format!("cannot read profile file {}: ", missing.display())),
        "{error}"
    );
    let binary = dir.file("binary.profile", b"name game\ndomains \xff\xfe.com\n");
    let error = read_file(&binary).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(
        error
            .to_string()
            .starts_with(&format!("cannot read profile file {}: ", binary.display())),
        "{error}"
    );
}

#[test]
fn a_profile_file_may_not_take_a_built_in_name() {
    for name in ["steam", "medium"] {
        let text = format!("name {name}\ndomains game.example.com\n");
        let error = parse_file("x.profile", &text).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("x.profile:1: bad name \"{name}\": it is the name of a built-in profile")
        );
    }
    assert!(parse_file("x.profile", "name steam2\ndomains game.example.com\n").is_ok());
}
