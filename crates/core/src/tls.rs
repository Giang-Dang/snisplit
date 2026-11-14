use core::ops::Range;

use crate::inspect::Inspection;
use crate::reader::Reader;

pub fn inspect_client_hello(stream_start: &[u8]) -> Inspection {
    match find_host_name(stream_start) {
        Ok(host) => Inspection::Host(host),
        Err(Stop::NeedMoreData) => Inspection::NeedMoreData,
        Err(Stop::NoHost) => Inspection::NoHost,
    }
}

pub fn client_hello_sni(stream_start: &[u8]) -> Option<Range<usize>> {
    match inspect_client_hello(stream_start) {
        Inspection::Host(host) => Some(host),
        Inspection::NeedMoreData | Inspection::NoHost => None,
    }
}

const CONTENT_TYPE_HANDSHAKE: u8 = 0x16;
const HANDSHAKE_TYPE_CLIENT_HELLO: u8 = 0x01;
const EXTENSION_SERVER_NAME: u16 = 0x0000;
const SERVER_NAME_TYPE_HOST: u8 = 0x00;
const TLS_MAJOR_VERSION: u8 = 0x03;

const MINOR_VERSION_LEN: usize = 1;
const RANDOM_LEN: usize = 32;
const MAX_SESSION_ID_LEN: usize = 32;
const MAX_RECORD_LEN: usize = 1 << 14;
const STREAM_END: usize = usize::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    NeedMoreData,
    NoHost,
}

type Parsed<T> = Result<T, Stop>;

fn require(condition: bool) -> Parsed<()> {
    condition.then_some(()).ok_or(Stop::NoHost)
}

struct BoundedReader<'a> {
    reader: Reader<'a>,
}

impl<'a> BoundedReader<'a> {
    fn new(stream: &'a [u8]) -> Self {
        Self {
            reader: Reader::new(stream),
        }
    }

    fn position(&self) -> usize {
        self.reader.position()
    }

    fn section(&self, len: usize, end: usize) -> Parsed<usize> {
        let section_end = self.position().checked_add(len).ok_or(Stop::NoHost)?;
        require(section_end <= end)?;
        Ok(section_end)
    }

    fn u16_section(&mut self, end: usize) -> Parsed<usize> {
        let len = self.u16(end)?;
        self.section(len.into(), end)
    }

    fn take(&mut self, len: usize, end: usize) -> Parsed<Range<usize>> {
        let start = self.position();
        let section_end = self.section(len, end)?;
        self.reader.skip(len).ok_or(Stop::NeedMoreData)?;
        Ok(start..section_end)
    }

    fn skip(&mut self, len: usize, end: usize) -> Parsed<()> {
        self.take(len, end).map(drop)
    }

    fn skip_to(&mut self, end: usize) -> Parsed<()> {
        self.skip(end.saturating_sub(self.position()), end)
    }

    fn array<const N: usize>(&mut self, end: usize) -> Parsed<[u8; N]> {
        self.section(N, end)?;
        self.reader.array().ok_or(Stop::NeedMoreData)
    }

    fn u8(&mut self, end: usize) -> Parsed<u8> {
        self.array(end).map(u8::from_be_bytes)
    }

    fn u16(&mut self, end: usize) -> Parsed<u16> {
        self.array(end).map(u16::from_be_bytes)
    }

    fn u24(&mut self, end: usize) -> Parsed<usize> {
        let [high, middle, low] = self.array(end)?;
        Ok((usize::from(high) << 16) | (usize::from(middle) << 8) | usize::from(low))
    }

    fn has_more_before(&self, end: usize) -> bool {
        self.position() < end
    }
}

fn find_host_name(stream: &[u8]) -> Parsed<Range<usize>> {
    let mut reader = BoundedReader::new(stream);
    let record_end = read_record_header(&mut reader)?;
    let hello_end = read_handshake_header(&mut reader, record_end)?;
    skip_to_extensions(&mut reader, hello_end)?;
    let extensions_end = read_extensions_header(&mut reader, hello_end)?;
    let server_name_end = find_server_name_extension(&mut reader, extensions_end)?;
    read_host_name(&mut reader, server_name_end)
}

fn read_record_header(reader: &mut BoundedReader) -> Parsed<usize> {
    require(reader.u8(STREAM_END)? == CONTENT_TYPE_HANDSHAKE)?;
    require(reader.u8(STREAM_END)? == TLS_MAJOR_VERSION)?;
    reader.skip(MINOR_VERSION_LEN, STREAM_END)?;
    let record_len = usize::from(reader.u16(STREAM_END)?);
    require(record_len <= MAX_RECORD_LEN)?;
    reader.section(record_len, STREAM_END)
}

fn read_handshake_header(reader: &mut BoundedReader, record_end: usize) -> Parsed<usize> {
    require(reader.u8(record_end)? == HANDSHAKE_TYPE_CLIENT_HELLO)?;
    let hello_len = reader.u24(record_end)?;
    reader.section(hello_len, record_end)
}

fn skip_to_extensions(reader: &mut BoundedReader, hello_end: usize) -> Parsed<()> {
    require(reader.u8(hello_end)? == TLS_MAJOR_VERSION)?;
    reader.skip(MINOR_VERSION_LEN + RANDOM_LEN, hello_end)?;
    let session_id_len = usize::from(reader.u8(hello_end)?);
    require(session_id_len <= MAX_SESSION_ID_LEN)?;
    reader.skip(session_id_len, hello_end)?;
    let cipher_suites_len = usize::from(reader.u16(hello_end)?);
    reader.skip(cipher_suites_len, hello_end)?;
    let compression_methods_len = usize::from(reader.u8(hello_end)?);
    reader.skip(compression_methods_len, hello_end)
}

fn read_extensions_header(reader: &mut BoundedReader, hello_end: usize) -> Parsed<usize> {
    require(reader.has_more_before(hello_end))?;
    reader.u16_section(hello_end)
}

fn find_server_name_extension(reader: &mut BoundedReader, extensions_end: usize) -> Parsed<usize> {
    loop {
        require(reader.has_more_before(extensions_end))?;
        let extension_type = reader.u16(extensions_end)?;
        let extension_end = reader.u16_section(extensions_end)?;
        if extension_type == EXTENSION_SERVER_NAME {
            return Ok(extension_end);
        }
        reader.skip_to(extension_end)?;
    }
}

fn read_host_name(reader: &mut BoundedReader, server_name_end: usize) -> Parsed<Range<usize>> {
    let name_list_end = reader.u16_section(server_name_end)?;
    loop {
        require(reader.has_more_before(name_list_end))?;
        let name_type = reader.u8(name_list_end)?;
        let name_len = usize::from(reader.u16(name_list_end)?);
        let name = reader.take(name_len, name_list_end)?;
        if name_type == SERVER_NAME_TYPE_HOST {
            require(!name.is_empty())?;
            return Ok(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECORD_HEADER_LEN: usize = 5;
    const HANDSHAKE_HEADER_LEN: usize = 4;
    const PROTOCOL_VERSION_LEN: usize = 2;
    const RECORD_VERSION_TLS_1_0: [u8; 2] = [0x03, 0x01];
    const CLIENT_VERSION_TLS_1_2: [u8; 2] = [0x03, 0x03];
    const HANDSHAKE_TYPE_SERVER_HELLO: u8 = 0x02;
    const CONTENT_TYPE_APPLICATION_DATA: u8 = 0x17;
    const EXTENSION_SUPPORTED_GROUPS: u16 = 0x000a;
    const EXTENSION_EXTENDED_MASTER_SECRET: u16 = 0x0017;
    const EXTENSION_PADDING: u16 = 0x0015;
    const EXTENSION_KEY_SHARE: u16 = 0x0033;
    const EXTENSION_GREASE: u16 = 0x0a0a;
    const SERVER_NAME_TYPE_UNKNOWN: u8 = 0x07;
    const GROUP_X25519: u16 = 0x001d;
    const GROUP_X25519_MLKEM768: u16 = 0x11ec;
    const X25519_MLKEM768_KEY_EXCHANGE_LEN: usize = 1216;
    const KEY_EXCHANGE_FILL: u8 = 0x5a;
    const CIPHER_TLS_AES_128_GCM_SHA256: u16 = 0x1301;
    const COMPRESSION_NULL: u8 = 0x00;
    const SESSION_ID_LEN: usize = 32;
    const CIPHER_SUITES_LEN: usize = 2;
    const COMPRESSION_METHODS_LEN: usize = 1;

    const RECORD_LENGTH_OFFSET: usize = 3;
    const HANDSHAKE_LENGTH_OFFSET: usize = RECORD_HEADER_LEN + 1;
    const SESSION_ID_LENGTH_OFFSET: usize =
        RECORD_HEADER_LEN + HANDSHAKE_HEADER_LEN + PROTOCOL_VERSION_LEN + RANDOM_LEN;
    const CIPHER_SUITES_LENGTH_OFFSET: usize = SESSION_ID_LENGTH_OFFSET + 1 + SESSION_ID_LEN;
    const COMPRESSION_METHODS_LENGTH_OFFSET: usize =
        CIPHER_SUITES_LENGTH_OFFSET + 2 + CIPHER_SUITES_LEN;
    const EXTENSIONS_LENGTH_OFFSET: usize =
        COMPRESSION_METHODS_LENGTH_OFFSET + 1 + COMPRESSION_METHODS_LEN;
    const FIRST_EXTENSION_LENGTH_OFFSET: usize = EXTENSIONS_LENGTH_OFFSET + 2 + 2;
    const HOST_NAME_LENGTH_BACK: usize = 2;
    const SERVER_NAME_LIST_LENGTH_BACK: usize = 5;
    const SERVER_NAME_EXTENSION_LENGTH_BACK: usize = 7;

    const PACKET_BOUNDARY: usize = 1400;
    const LARGE_HELLO_MIN_LEN: usize = 1800;
    const TRAILING_PADDING_LEN: usize = 512;
    const HOST: &str = "store.steampowered.com";

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

    fn server_name_entry(name_type: u8, name: &str) -> Vec<u8> {
        [&[name_type], u16_prefixed(name.as_bytes()).as_slice()].concat()
    }

    fn server_name_extension_with(entries: &[Vec<u8>]) -> Vec<u8> {
        extension(EXTENSION_SERVER_NAME, &u16_prefixed(&entries.concat()))
    }

    fn server_name_extension(host: &str) -> Vec<u8> {
        server_name_extension_with(&[server_name_entry(SERVER_NAME_TYPE_HOST, host)])
    }

    fn supported_groups_extension() -> Vec<u8> {
        extension(
            EXTENSION_SUPPORTED_GROUPS,
            &u16_prefixed(&GROUP_X25519.to_be_bytes()),
        )
    }

    fn extended_master_secret_extension() -> Vec<u8> {
        extension(EXTENSION_EXTENDED_MASTER_SECRET, &[])
    }

    fn key_share_extension(key_exchange_len: usize) -> Vec<u8> {
        let key_exchange = vec![KEY_EXCHANGE_FILL; key_exchange_len];
        let share = [
            GROUP_X25519_MLKEM768.to_be_bytes().as_slice(),
            &u16_prefixed(&key_exchange),
        ]
        .concat();
        extension(EXTENSION_KEY_SHARE, &u16_prefixed(&share))
    }

    fn padding_extension(len: usize) -> Vec<u8> {
        extension(EXTENSION_PADDING, &vec![0; len])
    }

    fn fields_before_extensions() -> Vec<u8> {
        [
            CLIENT_VERSION_TLS_1_2.to_vec(),
            vec![0xab; RANDOM_LEN],
            u8_prefixed(&[0xcd; SESSION_ID_LEN]),
            u16_prefixed(&CIPHER_TLS_AES_128_GCM_SHA256.to_be_bytes()),
            u8_prefixed(&[COMPRESSION_NULL]),
        ]
        .concat()
    }

    fn handshake_record(body: &[u8]) -> Vec<u8> {
        let handshake = [
            &[HANDSHAKE_TYPE_CLIENT_HELLO],
            u24_prefixed(body).as_slice(),
        ]
        .concat();
        [
            &[CONTENT_TYPE_HANDSHAKE],
            RECORD_VERSION_TLS_1_0.as_slice(),
            &u16_prefixed(&handshake),
        ]
        .concat()
    }

    fn client_hello_with(extensions: &[Vec<u8>]) -> Vec<u8> {
        let body = [
            fields_before_extensions(),
            u16_prefixed(&extensions.concat()),
        ]
        .concat();
        handshake_record(&body)
    }

    fn client_hello(host: &str) -> Vec<u8> {
        client_hello_with(&[
            supported_groups_extension(),
            server_name_extension(host),
            extended_master_secret_extension(),
        ])
    }

    fn large_hello_with_key_share_before_host(key_exchange_len: usize) -> Vec<u8> {
        client_hello_with(&[
            supported_groups_extension(),
            key_share_extension(key_exchange_len),
            server_name_extension(HOST),
            padding_extension(TRAILING_PADDING_LEN),
        ])
    }

    fn position_of(haystack: &[u8], needle: &[u8]) -> Range<usize> {
        let start = haystack
            .windows(needle.len())
            .position(|window| window == needle)
            .unwrap();
        start..start + needle.len()
    }

    fn key_exchange_len_placing_host_at(host_start: usize) -> usize {
        let without_key_exchange = large_hello_with_key_share_before_host(0);
        let base = position_of(&without_key_exchange, HOST.as_bytes()).start;
        host_start - base
    }

    fn assert_needs_more_data_until_host_complete(hello: &[u8], host: Range<usize>) {
        for cut in 0..=hello.len() {
            let expected = if cut >= host.end {
                Inspection::Host(host.clone())
            } else {
                Inspection::NeedMoreData
            };
            assert_eq!(
                inspect_client_hello(&hello[..cut]),
                expected,
                "cut at {cut}"
            );
        }
    }

    fn with_length_field(mut hello: Vec<u8>, offset: usize, value: u16) -> Vec<u8> {
        hello[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
        hello
    }

    fn read_u16_at(bytes: &[u8], offset: usize) -> usize {
        usize::from(u16::from_be_bytes([bytes[offset], bytes[offset + 1]]))
    }

    struct XorShift(u32);

    impl XorShift {
        fn next(&mut self) -> u32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            self.0
        }

        fn below(&mut self, bound: usize) -> usize {
            self.next() as usize % bound.max(1)
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

    #[test]
    fn builder_offsets_point_at_length_fields() {
        let hello = large_hello_with_key_share_before_host(X25519_MLKEM768_KEY_EXCHANGE_LEN);
        let host = position_of(&hello, HOST.as_bytes());
        assert_eq!(
            read_u16_at(&hello, RECORD_LENGTH_OFFSET),
            hello.len() - RECORD_HEADER_LEN
        );
        assert_eq!(hello[SESSION_ID_LENGTH_OFFSET], SESSION_ID_LEN as u8);
        assert_eq!(
            read_u16_at(&hello, CIPHER_SUITES_LENGTH_OFFSET),
            CIPHER_SUITES_LEN
        );
        assert_eq!(
            usize::from(hello[COMPRESSION_METHODS_LENGTH_OFFSET]),
            COMPRESSION_METHODS_LEN
        );
        assert_eq!(
            read_u16_at(&hello, EXTENSIONS_LENGTH_OFFSET),
            hello.len() - EXTENSIONS_LENGTH_OFFSET - 2
        );
        assert_eq!(
            read_u16_at(&hello, FIRST_EXTENSION_LENGTH_OFFSET),
            supported_groups_extension().len() - 4
        );
        assert_eq!(
            read_u16_at(&hello, host.start - HOST_NAME_LENGTH_BACK),
            HOST.len()
        );
    }

    #[test]
    fn large_hello_with_host_in_first_packet() {
        let hello = client_hello_with(&[
            supported_groups_extension(),
            server_name_extension(HOST),
            key_share_extension(X25519_MLKEM768_KEY_EXCHANGE_LEN),
            padding_extension(TRAILING_PADDING_LEN),
        ]);
        let host = position_of(&hello, HOST.as_bytes());
        assert!(hello.len() >= LARGE_HELLO_MIN_LEN);
        assert!(host.end < PACKET_BOUNDARY);
        assert_needs_more_data_until_host_complete(&hello, host);
    }

    #[test]
    fn large_hello_with_host_in_second_packet() {
        let hello = large_hello_with_key_share_before_host(X25519_MLKEM768_KEY_EXCHANGE_LEN + 300);
        let host = position_of(&hello, HOST.as_bytes());
        assert!(hello.len() >= LARGE_HELLO_MIN_LEN);
        assert!(host.start > PACKET_BOUNDARY);
        assert_eq!(
            inspect_client_hello(&hello[..PACKET_BOUNDARY]),
            Inspection::NeedMoreData
        );
        assert_needs_more_data_until_host_complete(&hello, host);
    }

    #[test]
    fn large_hello_with_host_straddling_packets() {
        let host_start = PACKET_BOUNDARY - HOST.len() / 2;
        let hello =
            large_hello_with_key_share_before_host(key_exchange_len_placing_host_at(host_start));
        let host = position_of(&hello, HOST.as_bytes());
        assert!(hello.len() >= LARGE_HELLO_MIN_LEN);
        assert!(host.start < PACKET_BOUNDARY && PACKET_BOUNDARY < host.end);
        assert_eq!(
            inspect_client_hello(&hello[..PACKET_BOUNDARY]),
            Inspection::NeedMoreData
        );
        assert_needs_more_data_until_host_complete(&hello, host);
    }

    #[test]
    fn short_prefixes_of_a_handshake_need_more_data() {
        assert_eq!(inspect_client_hello(&[]), Inspection::NeedMoreData);
        assert_eq!(
            inspect_client_hello(&[CONTENT_TYPE_HANDSHAKE]),
            Inspection::NeedMoreData
        );
        assert_eq!(
            inspect_client_hello(&[CONTENT_TYPE_HANDSHAKE, TLS_MAJOR_VERSION]),
            Inspection::NeedMoreData
        );
    }

    #[test]
    fn other_protocols_are_no_host() {
        assert_eq!(inspect_client_hello(b"G"), Inspection::NoHost);
        assert_eq!(
            inspect_client_hello(b"GET / HTTP/1.1\r\n"),
            Inspection::NoHost
        );
        assert_eq!(
            inspect_client_hello(&[CONTENT_TYPE_APPLICATION_DATA]),
            Inspection::NoHost
        );
        assert_eq!(
            inspect_client_hello(&[CONTENT_TYPE_HANDSHAKE, 0x02]),
            Inspection::NoHost
        );
        let mut server_hello = client_hello(HOST);
        server_hello[RECORD_HEADER_LEN] = HANDSHAKE_TYPE_SERVER_HELLO;
        assert_eq!(
            inspect_client_hello(&server_hello[..=RECORD_HEADER_LEN]),
            Inspection::NoHost
        );
    }

    #[test]
    fn extensions_without_server_name_are_no_host() {
        let hello = client_hello_with(&[
            supported_groups_extension(),
            extended_master_secret_extension(),
        ]);
        assert_eq!(inspect_client_hello(&hello), Inspection::NoHost);
        assert_eq!(
            inspect_client_hello(&hello[..hello.len() - 1]),
            Inspection::NeedMoreData
        );
    }

    #[test]
    fn hello_without_extensions_is_no_host() {
        let hello = handshake_record(&fields_before_extensions());
        assert_eq!(inspect_client_hello(&hello), Inspection::NoHost);
    }

    #[test]
    fn unknown_extensions_and_name_types_are_skipped() {
        let hello = client_hello_with(&[
            extension(EXTENSION_GREASE, &[]),
            server_name_extension_with(&[
                server_name_entry(SERVER_NAME_TYPE_UNKNOWN, "ignored"),
                server_name_entry(SERVER_NAME_TYPE_HOST, HOST),
            ]),
        ]);
        let host = position_of(&hello, HOST.as_bytes());
        assert_eq!(inspect_client_hello(&hello), Inspection::Host(host));
    }

    #[test]
    fn empty_host_name_is_no_host() {
        let hello = client_hello("");
        assert_eq!(inspect_client_hello(&hello), Inspection::NoHost);
    }

    #[test]
    fn hello_fragmented_across_records_is_no_host() {
        let hello = client_hello(HOST);
        let first_fragment_len = (hello.len() - RECORD_HEADER_LEN) as u16 / 2;
        let fragmented = with_length_field(hello, RECORD_LENGTH_OFFSET, first_fragment_len);
        assert_eq!(inspect_client_hello(&fragmented), Inspection::NoHost);
    }

    #[test]
    fn oversized_record_is_no_host() {
        let hello = client_hello(HOST);
        let oversized = with_length_field(hello, RECORD_LENGTH_OFFSET, MAX_RECORD_LEN as u16 + 1);
        assert_eq!(
            inspect_client_hello(&oversized[..RECORD_HEADER_LEN]),
            Inspection::NoHost
        );
    }

    #[test]
    fn length_overrunning_its_container_is_no_host_before_the_data_arrives() {
        let hello = large_hello_with_key_share_before_host(X25519_MLKEM768_KEY_EXCHANGE_LEN);
        let host = position_of(&hello, HOST.as_bytes());
        let name_length_offset = host.start - HOST_NAME_LENGTH_BACK;
        let overrunning_name = with_length_field(hello.clone(), name_length_offset, u16::MAX);
        assert_eq!(
            inspect_client_hello(&overrunning_name[..host.start]),
            Inspection::NoHost
        );
        let overrunning_extension =
            with_length_field(hello, FIRST_EXTENSION_LENGTH_OFFSET, u16::MAX);
        assert_eq!(
            inspect_client_hello(&overrunning_extension[..FIRST_EXTENSION_LENGTH_OFFSET + 2]),
            Inspection::NoHost
        );
    }

    #[test]
    fn mutated_hello_never_panics_and_prefixes_agree_with_whole_input() {
        let original = large_hello_with_key_share_before_host(X25519_MLKEM768_KEY_EXCHANGE_LEN);
        let host = position_of(&original, HOST.as_bytes());
        let length_fields = [
            RECORD_LENGTH_OFFSET,
            HANDSHAKE_LENGTH_OFFSET,
            HANDSHAKE_LENGTH_OFFSET + 1,
            SESSION_ID_LENGTH_OFFSET,
            CIPHER_SUITES_LENGTH_OFFSET,
            COMPRESSION_METHODS_LENGTH_OFFSET,
            EXTENSIONS_LENGTH_OFFSET,
            FIRST_EXTENSION_LENGTH_OFFSET,
            host.start - SERVER_NAME_EXTENSION_LENGTH_BACK,
            host.start - SERVER_NAME_LIST_LENGTH_BACK,
            host.start - HOST_NAME_LENGTH_BACK,
        ];
        let mut rng = XorShift(0x9e37_79b9);
        for _ in 0..12_000 {
            let mut input = original.clone();
            for _ in 0..=rng.below(3) {
                let offset = rng.below(input.len());
                input[offset] ^= 1 << rng.below(8);
            }
            if rng.next() % 2 == 0 {
                let offset = length_fields[rng.below(length_fields.len())];
                let length = read_u16_at(&input, offset);
                let corrupted = match rng.below(3) {
                    0 => length.wrapping_add(1 + rng.below(4)),
                    1 => length.wrapping_sub(1 + rng.below(4)),
                    _ => rng.next() as usize,
                };
                input[offset..offset + 2].copy_from_slice(&(corrupted as u16).to_be_bytes());
            }
            if rng.next() % 4 == 0 {
                input.truncate(rng.below(input.len() + 1));
            }
            let whole = inspect_client_hello(&input);
            if let Inspection::Host(host) = &whole {
                assert!(host.end <= input.len());
            }
            for _ in 0..3 {
                let cut = rng.below(input.len() + 1);
                let prefix = inspect_client_hello(&input[..cut]);
                assert!(
                    prefix == Inspection::NeedMoreData || prefix == whole,
                    "cut {cut}: prefix {prefix:?}, whole {whole:?}"
                );
            }
        }
    }
}
