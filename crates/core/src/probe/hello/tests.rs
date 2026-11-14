use super::*;
use crate::inspect::Inspection;
use crate::tls::inspect_client_hello;

const FIXED: HelloRandom = HelloRandom {
    random: [0x11; RANDOM_LEN],
    session_id: [0x22; RANDOM_LEN],
    key_share: [0x33; RANDOM_LEN],
};

fn expected_hello_for_ab_cd() -> Vec<u8> {
    let record_header: &[u8] = &[0x16, 0x03, 0x01, 0x00, 0xd4];
    let handshake_header: &[u8] = &[0x01, 0x00, 0x00, 0xd0];
    let legacy_version: &[u8] = &[0x03, 0x03];
    let random: &[u8] = &[0x11; 32];
    let session_id_length: &[u8] = &[0x20];
    let session_id: &[u8] = &[0x22; 32];
    let cipher_suites: &[u8] = &[
        0x00, 0x12, 0x13, 0x01, 0x13, 0x02, 0x13, 0x03, 0xc0, 0x2b, 0xc0, 0x2f, 0xc0, 0x2c, 0xc0,
        0x30, 0xcc, 0xa9, 0xcc, 0xa8,
    ];
    let compression_methods: &[u8] = &[0x01, 0x00];
    let extensions_length: &[u8] = &[0x00, 0x75];
    let server_name: &[u8] = &[
        0x00, 0x00, 0x00, 0x0a, 0x00, 0x08, 0x00, 0x00, 0x05, b'a', b'b', b'.', b'c', b'd',
    ];
    let supported_groups: &[u8] = &[
        0x00, 0x0a, 0x00, 0x08, 0x00, 0x06, 0x00, 0x1d, 0x00, 0x17, 0x00, 0x18,
    ];
    let signature_algorithms: &[u8] = &[
        0x00, 0x0d, 0x00, 0x12, 0x00, 0x10, 0x04, 0x03, 0x08, 0x04, 0x04, 0x01, 0x05, 0x03, 0x08,
        0x05, 0x05, 0x01, 0x08, 0x06, 0x06, 0x01,
    ];
    let alpn: &[u8] = &[
        0x00, 0x10, 0x00, 0x0e, 0x00, 0x0c, 0x02, b'h', b'2', 0x08, b'h', b't', b't', b'p', b'/',
        b'1', b'.', b'1',
    ];
    let supported_versions: &[u8] = &[0x00, 0x2b, 0x00, 0x05, 0x04, 0x03, 0x04, 0x03, 0x03];
    let key_share_header: &[u8] = &[0x00, 0x33, 0x00, 0x26, 0x00, 0x24, 0x00, 0x1d, 0x00, 0x20];
    let key_share: &[u8] = &[0x33; 32];
    [
        record_header,
        handshake_header,
        legacy_version,
        random,
        session_id_length,
        session_id,
        cipher_suites,
        compression_methods,
        extensions_length,
        server_name,
        supported_groups,
        signature_algorithms,
        alpn,
        supported_versions,
        key_share_header,
        key_share,
    ]
    .concat()
}

#[test]
fn the_hello_for_a_short_host_matches_the_rfc_layout_byte_for_byte() {
    let expected = expected_hello_for_ab_cd();
    assert_eq!(expected.len(), 217);
    assert_eq!(client_hello("ab.cd", &FIXED), Some(expected));
}

#[test]
fn the_tls_inspector_finds_the_host_right_after_the_server_name_headers() {
    let host = "store.steampowered.com";
    let hello = client_hello(host, &FIXED).unwrap();
    assert_eq!(
        inspect_client_hello(&hello),
        Inspection::Host(109..109 + host.len())
    );
    assert_eq!(&hello[109..131], host.as_bytes());
}

#[test]
fn every_length_field_grows_with_the_host() {
    let host = "a".repeat(MAX_HOST_LEN - 4) + ".com";
    let hello = client_hello(&host, &FIXED).unwrap();
    assert_eq!(hello.len(), 253 + 212);
    let record_length = u16::from_be_bytes([hello[3], hello[4]]);
    assert_eq!(usize::from(record_length), 253 + 207);
    assert_eq!(&hello[6..9], &[0x00, 0x01, 0xc8]);
    let extensions_length = u16::from_be_bytes([hello[98], hello[99]]);
    assert_eq!(usize::from(extensions_length), 253 + 112);
    let server_name_length = u16::from_be_bytes([hello[102], hello[103]]);
    assert_eq!(usize::from(server_name_length), 253 + 5);
    assert_eq!(
        inspect_client_hello(&hello),
        Inspection::Host(109..109 + 253)
    );
}

#[test]
fn an_empty_or_over_long_host_builds_no_hello() {
    assert_eq!(client_hello("", &FIXED), None);
    let too_long = "a".repeat(MAX_HOST_LEN - 3) + ".com";
    assert_eq!(too_long.len(), 254);
    assert_eq!(client_hello(&too_long, &FIXED), None);
}

#[test]
fn the_secrets_land_in_the_random_session_id_and_key_share_fields() {
    let secrets = HelloRandom {
        random: [0xa1; RANDOM_LEN],
        session_id: [0xb2; RANDOM_LEN],
        key_share: [0xc3; RANDOM_LEN],
    };
    let hello = client_hello("ab.cd", &secrets).unwrap();
    assert_eq!(&hello[11..43], &[0xa1; 32]);
    assert_eq!(hello[43], 32);
    assert_eq!(&hello[44..76], &[0xb2; 32]);
    assert_eq!(&hello[217 - 32..], &[0xc3; 32]);
}
