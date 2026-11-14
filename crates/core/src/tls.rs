use core::ops::Range;

use crate::inspect::Inspection;
use crate::reader::Reader;

pub fn inspect_client_hello(stream_start: &[u8]) -> Inspection {
    match client_hello_sni(stream_start) {
        Some(host) => Inspection::Host(host),
        None => Inspection::NeedMoreData,
    }
}

const CONTENT_TYPE_HANDSHAKE: u8 = 0x16;
const HANDSHAKE_TYPE_CLIENT_HELLO: u8 = 0x01;
const EXTENSION_SERVER_NAME: u16 = 0x0000;
const SERVER_NAME_TYPE_HOST: u8 = 0x00;

const PROTOCOL_VERSION_LEN: usize = 2;
const RECORD_LENGTH_LEN: usize = 2;
const HANDSHAKE_LENGTH_LEN: usize = 3;
const RANDOM_LEN: usize = 32;
const EXTENSION_HEADER_LEN: usize = 4;
const SERVER_NAME_LIST_LENGTH_LEN: usize = 2;

pub fn client_hello_sni(stream_start: &[u8]) -> Option<Range<usize>> {
    let mut reader = Reader::new(stream_start);
    read_record_header(&mut reader)?;
    read_client_hello_header(&mut reader)?;
    skip_to_extensions(&mut reader)?;
    find_host_name(&mut reader)
}

fn read_record_header(reader: &mut Reader) -> Option<()> {
    reader.expect_u8(CONTENT_TYPE_HANDSHAKE)?;
    reader.skip(PROTOCOL_VERSION_LEN + RECORD_LENGTH_LEN)
}

fn read_client_hello_header(reader: &mut Reader) -> Option<()> {
    reader.expect_u8(HANDSHAKE_TYPE_CLIENT_HELLO)?;
    reader.skip(HANDSHAKE_LENGTH_LEN)
}

fn skip_to_extensions(reader: &mut Reader) -> Option<()> {
    reader.skip(PROTOCOL_VERSION_LEN + RANDOM_LEN)?;
    let _session_id = reader.u8_prefixed()?;
    let _cipher_suites = reader.u16_prefixed()?;
    let _compression_methods = reader.u8_prefixed()?;
    Some(())
}

fn find_host_name(reader: &mut Reader) -> Option<Range<usize>> {
    let extensions_len = usize::from(reader.u16()?);
    let extensions_end = reader.position() + extensions_len;
    while reader.position() + EXTENSION_HEADER_LEN <= extensions_end {
        let extension_type = reader.u16()?;
        let extension_len = usize::from(reader.u16()?);
        if extension_type == EXTENSION_SERVER_NAME {
            return read_host_name(reader);
        }
        reader.skip(extension_len)?;
    }
    None
}

fn read_host_name(reader: &mut Reader) -> Option<Range<usize>> {
    reader.skip(SERVER_NAME_LIST_LENGTH_LEN)?;
    reader.expect_u8(SERVER_NAME_TYPE_HOST)?;
    let host_name = reader.u16_prefixed()?;
    (!host_name.is_empty()).then_some(host_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECORD_HEADER_LEN: usize = 5;
    const RECORD_VERSION_TLS_1_0: [u8; 2] = [0x03, 0x01];
    const CLIENT_VERSION_TLS_1_2: [u8; 2] = [0x03, 0x03];
    const HANDSHAKE_TYPE_SERVER_HELLO: u8 = 0x02;
    const EXTENSION_SUPPORTED_GROUPS: u16 = 0x000a;
    const EXTENSION_EXTENDED_MASTER_SECRET: u16 = 0x0017;
    const GROUP_X25519: u16 = 0x001d;
    const CIPHER_TLS_AES_128_GCM_SHA256: u16 = 0x1301;
    const COMPRESSION_NULL: u8 = 0x00;

    fn u8_prefixed(data: &[u8]) -> Vec<u8> {
        [&[data.len() as u8], data].concat()
    }

    fn u16_prefixed(data: &[u8]) -> Vec<u8> {
        [&(data.len() as u16).to_be_bytes(), data].concat()
    }

    fn u24_prefixed(data: &[u8]) -> Vec<u8> {
        [&(data.len() as u32).to_be_bytes()[1..], data].concat()
    }

    fn extension(extension_type: u16, data: &[u8]) -> Vec<u8> {
        [extension_type.to_be_bytes().as_slice(), &u16_prefixed(data)].concat()
    }

    fn client_hello(host: &str) -> Vec<u8> {
        let server_name = [
            &[SERVER_NAME_TYPE_HOST],
            u16_prefixed(host.as_bytes()).as_slice(),
        ]
        .concat();
        let extensions = [
            extension(
                EXTENSION_SUPPORTED_GROUPS,
                &u16_prefixed(&GROUP_X25519.to_be_bytes()),
            ),
            extension(EXTENSION_SERVER_NAME, &u16_prefixed(&server_name)),
            extension(EXTENSION_EXTENDED_MASTER_SECRET, &[]),
        ]
        .concat();
        let body = [
            CLIENT_VERSION_TLS_1_2.to_vec(),
            vec![0xab; RANDOM_LEN],
            u8_prefixed(&[0xcd; 32]),
            u16_prefixed(&CIPHER_TLS_AES_128_GCM_SHA256.to_be_bytes()),
            u8_prefixed(&[COMPRESSION_NULL]),
            u16_prefixed(&extensions),
        ]
        .concat();
        let handshake = [
            &[HANDSHAKE_TYPE_CLIENT_HELLO],
            u24_prefixed(&body).as_slice(),
        ]
        .concat();
        [
            &[CONTENT_TYPE_HANDSHAKE],
            RECORD_VERSION_TLS_1_0.as_slice(),
            &u16_prefixed(&handshake),
        ]
        .concat()
    }

    struct XorShift(u32);

    impl XorShift {
        fn next(&mut self) -> u32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            self.0
        }
    }

    #[test]
    fn finds_sni() {
        let hello = client_hello("store.steampowered.com");
        let host = client_hello_sni(&hello).unwrap();
        assert_eq!(&hello[host], b"store.steampowered.com");
    }

    #[test]
    fn truncated_input_never_panics_and_needs_whole_name() {
        let hello = client_hello("steamcommunity.com");
        let host = client_hello_sni(&hello).unwrap();
        for cut in 0..hello.len() {
            let expected = (cut >= host.end).then(|| host.clone());
            assert_eq!(client_hello_sni(&hello[..cut]), expected, "cut at {cut}");
        }
    }

    #[test]
    fn rejects_non_client_hello() {
        assert_eq!(client_hello_sni(b"GET / HTTP/1.1\r\n"), None);
        let mut server_hello = client_hello("example.com");
        server_hello[RECORD_HEADER_LEN] = HANDSHAKE_TYPE_SERVER_HELLO;
        assert_eq!(client_hello_sni(&server_hello), None);
    }

    #[test]
    fn random_input_never_panics() {
        let mut rng = XorShift(0x1234_5678);
        for _ in 0..20_000 {
            let len = (rng.next() % 600) as usize;
            let mut input: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            let looks_like_client_hello = len > RECORD_HEADER_LEN && rng.next() % 2 == 0;
            if looks_like_client_hello {
                input[0] = CONTENT_TYPE_HANDSHAKE;
                input[RECORD_HEADER_LEN] = HANDSHAKE_TYPE_CLIENT_HELLO;
            }
            if let Some(host) = client_hello_sni(&input) {
                assert!(host.end <= input.len());
            }
        }
    }
}
