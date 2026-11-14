mod common;

use common::{fixture_stems, read_stream};

const EXTENSION_SERVER_NAME: u16 = 0x0000;
const EXTENSION_PRE_SHARED_KEY: u16 = 0x0029;
const EXTENSION_EARLY_DATA: u16 = 0x002a;
const EXTENSIONS_CARRYING_RESUMPTION_SECRETS: [u16; 2] =
    [EXTENSION_PRE_SHARED_KEY, EXTENSION_EARLY_DATA];

const RECORD_HEADER_LEN: usize = 5;
const HANDSHAKE_HEADER_LEN: usize = 4;
const CLIENT_VERSION_LEN: usize = 2;
const RANDOM_LEN: usize = 32;
const EXTENSION_HEADER_LEN: usize = 4;

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Cursor<'_> {
    fn skip(&mut self, len: usize) {
        self.position += len;
    }

    fn u8(&mut self) -> Option<usize> {
        let value = *self.bytes.get(self.position)?;
        self.position += 1;
        Some(usize::from(value))
    }

    fn u16(&mut self) -> Option<usize> {
        let pair = self.bytes.get(self.position..self.position + 2)?;
        self.position += 2;
        Some(usize::from(u16::from_be_bytes([pair[0], pair[1]])))
    }
}

fn extension_types(client_hello: &[u8]) -> Option<Vec<u16>> {
    let mut cursor = Cursor {
        bytes: client_hello,
        position: RECORD_HEADER_LEN + HANDSHAKE_HEADER_LEN + CLIENT_VERSION_LEN + RANDOM_LEN,
    };
    let session_id_len = cursor.u8()?;
    cursor.skip(session_id_len);
    let cipher_suites_len = cursor.u16()?;
    cursor.skip(cipher_suites_len);
    let compression_methods_len = cursor.u8()?;
    cursor.skip(compression_methods_len);
    let extensions_len = cursor.u16()?;
    let extensions_end = cursor.position + extensions_len;
    let mut types = Vec::new();
    while cursor.position + EXTENSION_HEADER_LEN <= extensions_end {
        types.push(u16::try_from(cursor.u16()?).ok()?);
        let extension_len = cursor.u16()?;
        cursor.skip(extension_len);
    }
    Some(types)
}

#[test]
fn fixtures_carry_no_session_resumption_secrets() {
    for stem in fixture_stems() {
        let types = extension_types(&read_stream(&stem))
            .unwrap_or_else(|| panic!("{stem}: extensions cannot be read"));
        assert!(
            types.contains(&EXTENSION_SERVER_NAME),
            "{stem}: server_name not among {types:?}, so the extension walk went wrong"
        );
        for secret in EXTENSIONS_CARRYING_RESUMPTION_SECRETS {
            assert!(
                !types.contains(&secret),
                "{stem} carries extension {secret:#06x}; remove it from the public fixtures"
            );
        }
    }
}
