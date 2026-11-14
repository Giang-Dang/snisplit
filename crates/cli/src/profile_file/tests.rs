use super::*;

const SOURCE: &str = "game.profile";
const RESERVED: [&str; 2] = ["steam", "medium"];

fn parsed(text: &str) -> Result<ProfileSpec, ProfileError> {
    parse(SOURCE, text, &RESERVED)
}

fn spec(text: &str) -> ProfileSpec {
    parsed(text).unwrap_or_else(|error| panic!("{error}"))
}

fn failure(text: &str) -> (Option<usize>, Problem) {
    match parsed(text) {
        Ok(spec) => panic!("expected an error, parsed {spec:?}"),
        Err(error) => {
            assert_eq!(error.source, SOURCE);
            (error.line, error.problem)
        }
    }
}

fn with_domain(extra: &str) -> String {
    format!("name game\ndomains game.example.com\n{extra}\n")
}

fn long_label(length: usize) -> String {
    "a".repeat(length)
}

fn text(value: &str) -> String {
    value.to_string()
}

#[test]
fn only_name_and_domains_are_needed_and_the_rest_defaults() {
    assert_eq!(
        spec("name game\ndomains game.example.com\n"),
        ProfileSpec {
            name: text("game"),
            domains: vec![text("game.example.com")],
            tls_ports: vec![443],
            tls_positions: vec![SplitPosition::Offset(1)],
            http_ports: vec![80],
            http_method: HttpMethod::HostSpell,
        }
    );
}

#[test]
fn every_key_is_read_with_its_own_meaning() {
    let source = "name my-game2\n\
                  domains one.example.com Two.Example.COM\n\
                  domains three.example.org\n\
                  tls-ports 8443 27015-27017 9000\n\
                  tls-split 2 host-start domain-mid 40\n\
                  http-ports 8080 8000-8001\n\
                  http-method pass\n";
    assert_eq!(
        spec(source),
        ProfileSpec {
            name: text("my-game2"),
            domains: vec![
                text("one.example.com"),
                text("two.example.com"),
                text("three.example.org"),
            ],
            tls_ports: vec![8443, 27015, 27016, 27017, 9000],
            tls_positions: vec![
                SplitPosition::Offset(2),
                SplitPosition::HostStart,
                SplitPosition::DomainMid,
                SplitPosition::Offset(40),
            ],
            http_ports: vec![8080, 8000, 8001],
            http_method: HttpMethod::PassThrough,
        }
    );
}

#[test]
fn comments_blank_lines_indentation_tabs_crlf_and_a_byte_order_mark_are_layout_only() {
    let source = "\u{feff}# my game\r\n\r\n  name\tgame  \r\n\t# note\r\ndomains   a.example.com \t b.example.com\r\n   \r\n";
    let result = spec(source);
    assert_eq!(result.name, "game");
    assert_eq!(result.domains, ["a.example.com", "b.example.com"]);
}

#[test]
fn a_comment_mark_inside_a_line_is_not_a_comment() {
    assert_eq!(
        failure("name game\ndomains game.example.com # all of it\n"),
        (Some(2), Problem::DomainCharacters(text("#")))
    );
}

#[test]
fn none_disables_a_protocol_and_frees_its_ports_for_the_other() {
    let tls_off = spec(&with_domain("tls-ports none\nhttp-ports 443"));
    assert_eq!((tls_off.tls_ports, tls_off.http_ports), (vec![], vec![443]));
    let http_off = spec(&with_domain("http-ports none"));
    assert_eq!(
        (http_off.tls_ports, http_off.http_ports),
        (vec![443], vec![])
    );
}

#[test]
fn the_relay_profile_carries_name_ports_positions_method_and_matches_subdomains() {
    let source = "name game\ndomains game.example.com\ntls-ports 8443\ntls-split host-start 3\nhttp-ports 8080\nhttp-method pass\n";
    let profile = spec(source).relay_profile();
    assert_eq!(profile.name, "game");
    assert_eq!(profile.tls_ports, [8443]);
    assert_eq!(
        profile.tls_positions,
        [SplitPosition::HostStart, SplitPosition::Offset(3)]
    );
    assert_eq!(profile.http_ports, [8080]);
    assert_eq!(profile.http_method, HttpMethod::PassThrough);
    assert_eq!(
        profile.domains.matched_domain(b"cdn.game.example.com"),
        Some(4..20)
    );
    assert_eq!(profile.domains.matched_domain(b"example.com"), None);
}

#[test]
fn structural_mistakes_name_the_line_and_the_problem() {
    let cases = [
        (
            "name game\ndomains a.example.com\nbogus 1\n",
            Some(3),
            Problem::UnknownKey(text("bogus")),
        ),
        ("Name game\n", Some(1), Problem::UnknownKey(text("Name"))),
        (
            "name game\ndomains a.example.com\nname other\n",
            Some(3),
            Problem::RepeatedKey(text("name")),
        ),
        (
            "name game\ndomains a.example.com\ntls-ports 443\ntls-ports 8443\n",
            Some(4),
            Problem::RepeatedKey(text("tls-ports")),
        ),
        (
            "name game\ndomains a.example.com\nhttp-ports 80\nhttp-ports 81\n",
            Some(4),
            Problem::RepeatedKey(text("http-ports")),
        ),
        (
            "name game\ndomains a.example.com\ntls-split 1\ntls-split 2\n",
            Some(4),
            Problem::RepeatedKey(text("tls-split")),
        ),
        (
            "name game\ndomains a.example.com\nhttp-method pass\nhttp-method pass\n",
            Some(4),
            Problem::RepeatedKey(text("http-method")),
        ),
        (
            "name game\ndomains\n",
            Some(2),
            Problem::NoValue(text("domains")),
        ),
        (
            "name game\ndomains a.example.com\ntls-ports\n",
            Some(3),
            Problem::NoValue(text("tls-ports")),
        ),
        (
            "name game other\n",
            Some(1),
            Problem::ExtraValue(text("name")),
        ),
        (
            "name game\ndomains a.example.com\nhttp-method pass host-spell\n",
            Some(3),
            Problem::ExtraValue(text("http-method")),
        ),
        ("domains a.example.com\n", None, Problem::MissingKey("name")),
        ("name game\n", None, Problem::MissingKey("domains")),
        ("# only a comment\n\n", None, Problem::MissingKey("name")),
    ];
    for (source, line, problem) in cases {
        assert_eq!(failure(source), (line, problem), "{source:?}");
    }
}

#[test]
fn the_error_text_says_the_source_the_line_and_what_is_wrong() {
    let at_line = parse("C:\\p\\game.profile", "name game\nbogus 1\n", &RESERVED).unwrap_err();
    assert_eq!(
        at_line.to_string(),
        "C:\\p\\game.profile:2: unknown key \"bogus\""
    );
    let whole_file = parse("built-in x", "name game\n", &RESERVED).unwrap_err();
    assert_eq!(
        whole_file.to_string(),
        "built-in x: missing required key domains"
    );
}

#[test]
fn control_characters_in_a_bad_value_are_escaped_in_the_error_text() {
    let error = parse(SOURCE, "name game\ndomains a\u{1b}[31m.com\n", &RESERVED).unwrap_err();
    assert!(!error.to_string().contains('\u{1b}'), "{error}");
    assert!(error.to_string().contains("\\u{1b}"), "{error}");
}

#[test]
fn names_are_one_to_thirty_two_lowercase_alphanumerics_or_hyphens_and_not_built_in() {
    let thirty_two = long_label(32);
    let accepted = spec(&format!("name {thirty_two}\ndomains a.example.com\n"));
    assert_eq!(accepted.name, thirty_two);
    let thirty_three = long_label(33);
    let cases = [
        (thirty_three.clone(), Problem::NameLength(thirty_three)),
        (text("Game"), Problem::NameCharacters(text("Game"))),
        (text("my_game"), Problem::NameCharacters(text("my_game"))),
        (text("a'b"), Problem::NameCharacters(text("a'b"))),
        (text("a;b"), Problem::NameCharacters(text("a;b"))),
        (text("$(x)"), Problem::NameCharacters(text("$(x)"))),
        (text("a.b"), Problem::NameCharacters(text("a.b"))),
        (text("\u{1ee9}"), Problem::NameCharacters(text("\u{1ee9}"))),
        (text("steam"), Problem::NameReserved(text("steam"))),
        (text("medium"), Problem::NameReserved(text("medium"))),
    ];
    for (name, problem) in cases {
        let source = format!("name {name}\ndomains a.example.com\n");
        assert_eq!(failure(&source), (Some(1), problem), "{name:?}");
    }
}

#[test]
fn a_built_in_name_is_only_refused_when_the_caller_reserves_it() {
    let result = parse("built-in steam", "name steam\ndomains a.example.com\n", &[]);
    assert_eq!(result.unwrap().name, "steam");
}

#[test]
fn valid_domains_are_lowercased_and_the_length_limits_are_inclusive() {
    let label63 = long_label(63);
    let name253 = format!("{label63}.{label63}.{label63}.{}", long_label(61));
    assert_eq!(name253.len(), 253);
    let with_long_label = format!("{label63}.com");
    let accepted = [
        ("WWW.Example.COM", "www.example.com"),
        ("xn--bcher-kva.example", "xn--bcher-kva.example"),
        ("steam-chat.com", "steam-chat.com"),
        ("a.b", "a.b"),
        ("1.2", "1.2"),
        (with_long_label.as_str(), with_long_label.as_str()),
        (name253.as_str(), name253.as_str()),
    ];
    for (input, expected) in accepted {
        assert_eq!(parse_domain(input), Ok(text(expected)), "{input}");
    }
}

#[test]
fn hostile_domains_with_forbidden_characters_are_rejected() {
    let inputs = [
        "evil.com'",
        "a'b.com",
        "a\"b.com",
        "a;b.com",
        "$(x).com",
        "a$(calc).com",
        "a`b.com",
        "a b.com",
        "a\tb.com",
        "a\nb.com",
        "a\rb.com",
        "a\0b.com",
        "a\\b.com",
        "a/b.com",
        "../a.com",
        "a_b.com",
        "*.example.com",
        "a%b.com",
        "\u{1ee9}.com",
        "caf\u{e9}.com",
        "\u{212a}.com",
        "a.com\u{200b}",
        "a\u{ff0e}com",
        "a\u{ff07}.com",
    ];
    for input in inputs {
        assert_eq!(
            parse_domain(input),
            Err(Problem::DomainCharacters(text(input))),
            "{input:?}"
        );
    }
}

#[test]
fn hostile_domains_with_bad_structure_are_rejected_for_the_right_reason() {
    let label64 = long_label(64);
    let name254 = format!("{0}.{0}.{0}.{1}", long_label(63), long_label(62));
    assert_eq!(name254.len(), 254);
    let with_long_label = format!("{label64}.com");
    let cases = [
        ("a..com", Problem::DomainEmptyLabel(text("a..com"))),
        (
            ".example.com",
            Problem::DomainEmptyLabel(text(".example.com")),
        ),
        (
            "example.com.",
            Problem::DomainEmptyLabel(text("example.com.")),
        ),
        ("..", Problem::DomainEmptyLabel(text(".."))),
        ("a.b..", Problem::DomainEmptyLabel(text("a.b.."))),
        ("-a.com", Problem::DomainHyphenAtEdge(text("-a.com"))),
        ("a-.com", Problem::DomainHyphenAtEdge(text("a-.com"))),
        ("a.com-", Problem::DomainHyphenAtEdge(text("a.com-"))),
        ("a.-b.com", Problem::DomainHyphenAtEdge(text("a.-b.com"))),
        ("com", Problem::DomainSingleLabel(text("com"))),
        ("localhost", Problem::DomainSingleLabel(text("localhost"))),
        (
            with_long_label.as_str(),
            Problem::DomainLabelTooLong(with_long_label.clone()),
        ),
        (name254.as_str(), Problem::DomainTooLong(name254.clone())),
    ];
    for (input, problem) in cases {
        assert_eq!(parse_domain(input), Err(problem), "{input:?}");
    }
}

#[test]
fn hostile_domains_in_a_file_are_rejected_at_their_line() {
    let cases = [
        (
            "name game\ndomains a'b.com\n",
            2,
            Problem::DomainCharacters(text("a'b.com")),
        ),
        (
            "name game\n\ndomains good.com a\"b.com\n",
            3,
            Problem::DomainCharacters(text("a\"b.com")),
        ),
        (
            "name game\ndomains x.com;calc.com\n",
            2,
            Problem::DomainCharacters(text("x.com;calc.com")),
        ),
        (
            "name game\ndomains $(calc).com\n",
            2,
            Problem::DomainCharacters(text("$(calc).com")),
        ),
        (
            "name game\ndomains a`b.com\n",
            2,
            Problem::DomainCharacters(text("a`b.com")),
        ),
        (
            "name game\ndomains exam ple.com\n",
            2,
            Problem::DomainSingleLabel(text("exam")),
        ),
        (
            "name game\ndomains evil.com\r.example.com\n",
            2,
            Problem::DomainCharacters(text("evil.com\r.example.com")),
        ),
        (
            "name game\ndomains good.com\n$(calc).com\n",
            3,
            Problem::UnknownKey(text("$(calc).com")),
        ),
        (
            "name game\ndomains good.com\n.example.com\n",
            3,
            Problem::UnknownKey(text(".example.com")),
        ),
    ];
    for (source, line, problem) in cases {
        assert_eq!(failure(source), (Some(line), problem), "{source:?}");
    }
}

#[test]
fn a_domain_listed_twice_in_one_file_is_an_error_even_in_another_case() {
    assert_eq!(
        failure("name game\ndomains a.example.com b.example.com\ndomains A.EXAMPLE.com\n"),
        (Some(3), Problem::DuplicateDomain(text("a.example.com")))
    );
    assert_eq!(
        failure("name game\ndomains a.example.com a.example.com\n"),
        (Some(2), Problem::DuplicateDomain(text("a.example.com")))
    );
}

#[test]
fn bad_port_values_are_rejected_for_the_right_reason() {
    let cases = [
        ("0", Problem::PortOutOfRange(text("0"))),
        ("65536", Problem::PortOutOfRange(text("65536"))),
        (
            "99999999999999999999",
            Problem::PortOutOfRange(text("99999999999999999999")),
        ),
        ("0-10", Problem::PortOutOfRange(text("0-10"))),
        ("65000-70000", Problem::PortOutOfRange(text("65000-70000"))),
        ("+443", Problem::PortNotNumber(text("+443"))),
        ("44a", Problem::PortNotNumber(text("44a"))),
        ("-5", Problem::PortNotNumber(text("-5"))),
        ("80-", Problem::PortNotNumber(text("80-"))),
        ("1-2-3", Problem::PortNotNumber(text("1-2-3"))),
        ("http", Problem::PortNotNumber(text("http"))),
        ("53", Problem::PortIsDns(text("53"))),
        ("50-60", Problem::PortIsDns(text("50-60"))),
        ("1-65535", Problem::PortIsDns(text("1-65535"))),
        ("53-53", Problem::PortIsDns(text("53-53"))),
        ("30-20", Problem::ReversedRange(text("30-20"))),
        ("443 443", Problem::DuplicatePort(443)),
        ("100-200 150", Problem::DuplicatePort(150)),
        ("100-200 190-210", Problem::DuplicatePort(190)),
        ("none 443", Problem::NoneWithPorts),
        ("443 none", Problem::NoneWithPorts),
    ];
    for (value, problem) in cases {
        for key in ["tls-ports", "http-ports"] {
            let source = with_domain(&format!("{key} {value}"));
            assert_eq!(
                failure(&source),
                (Some(3), problem.clone()),
                "{key} {value}"
            );
        }
    }
}

#[test]
fn the_port_limits_are_inclusive() {
    let result = spec(&with_domain(
        "tls-ports 1 65535 54-56 52 51\nhttp-ports 8080",
    ));
    assert_eq!(result.tls_ports, [1, 65535, 54, 55, 56, 52, 51]);
}

#[test]
fn a_port_in_both_lists_is_an_error_reported_at_the_later_key() {
    assert_eq!(
        failure(&with_domain("tls-ports 8080\nhttp-ports 7000 8080")),
        (Some(4), Problem::PortInBothLists(8080))
    );
    assert_eq!(
        failure(&with_domain("http-ports 8080\ntls-ports 8000-8100")),
        (Some(4), Problem::PortInBothLists(8080))
    );
}

#[test]
fn a_port_that_clashes_with_the_other_lists_default_is_an_error_at_the_given_key() {
    assert_eq!(
        failure(&with_domain("tls-ports 80")),
        (Some(3), Problem::PortInBothLists(80))
    );
    assert_eq!(
        failure(&with_domain("http-ports 443")),
        (Some(3), Problem::PortInBothLists(443))
    );
}

#[test]
fn both_protocols_disabled_is_an_error_at_the_later_key() {
    assert_eq!(
        failure(&with_domain("tls-ports none\nhttp-ports none")),
        (Some(4), Problem::BothProtocolsDisabled)
    );
    assert_eq!(
        failure(&with_domain("http-ports none\ntls-ports none")),
        (Some(4), Problem::BothProtocolsDisabled)
    );
}

#[test]
fn split_positions_are_positive_offsets_or_the_two_named_places() {
    let values = [
        "0",
        "-1",
        "+1",
        "1.5",
        "middle",
        "HOST-START",
        "none",
        "99999999999999999999999",
    ];
    for value in values {
        let source = with_domain(&format!("tls-split 1 {value}"));
        assert_eq!(
            failure(&source),
            (Some(3), Problem::BadPosition(text(value))),
            "{value}"
        );
    }
}

#[test]
fn the_http_method_is_host_spell_or_pass_and_nothing_else() {
    assert_eq!(
        spec(&with_domain("http-method host-spell")).http_method,
        HttpMethod::HostSpell
    );
    assert_eq!(
        spec(&with_domain("http-method pass")).http_method,
        HttpMethod::PassThrough
    );
    for value in ["Pass", "spell", "none", "host-spell,pass"] {
        let source = with_domain(&format!("http-method {value}"));
        assert_eq!(
            failure(&source),
            (Some(3), Problem::BadMethod(text(value))),
            "{value}"
        );
    }
}
