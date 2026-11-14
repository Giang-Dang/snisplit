pub const RANDOM_LEN: usize = 32;
pub const MAX_HOST_LEN: usize = 253;

const CONTENT_TYPE_HANDSHAKE: u8 = 0x16;
const RECORD_VERSION: u16 = 0x0301;
const HANDSHAKE_TYPE_CLIENT_HELLO: u8 = 0x01;
const LEGACY_VERSION: u16 = 0x0303;
const COMPRESSION_NULL: u8 = 0x00;
const SERVER_NAME_TYPE_HOST: u8 = 0x00;

const EXTENSION_SERVER_NAME: u16 = 0x0000;
const EXTENSION_SUPPORTED_GROUPS: u16 = 0x000a;
const EXTENSION_SIGNATURE_ALGORITHMS: u16 = 0x000d;
const EXTENSION_ALPN: u16 = 0x0010;
const EXTENSION_SUPPORTED_VERSIONS: u16 = 0x002b;
const EXTENSION_KEY_SHARE: u16 = 0x0033;

const GROUP_X25519: u16 = 0x001d;
const GROUP_SECP256R1: u16 = 0x0017;
const GROUP_SECP384R1: u16 = 0x0018;

const CIPHER_SUITES: [u16; 9] = [
    0x1301, 0x1302, 0x1303, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8,
];
const GROUPS: [u16; 3] = [GROUP_X25519, GROUP_SECP256R1, GROUP_SECP384R1];
const SIGNATURE_ALGORITHMS: [u16; 8] = [
    0x0403, 0x0804, 0x0401, 0x0503, 0x0805, 0x0501, 0x0806, 0x0601,
];
const ALPN_PROTOCOLS: [&[u8]; 2] = [b"h2", b"http/1.1"];
const SUPPORTED_VERSIONS: [u16; 2] = [0x0304, 0x0303];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelloRandom {
    pub random: [u8; RANDOM_LEN],
    pub session_id: [u8; RANDOM_LEN],
    pub key_share: [u8; RANDOM_LEN],
}

pub fn client_hello(host: &str, secrets: &HelloRandom) -> Option<Vec<u8>> {
    if host.is_empty() || host.len() > MAX_HOST_LEN {
        return None;
    }
    let mut out = Prefixed::default();
    out.u8(CONTENT_TYPE_HANDSHAKE);
    out.u16(RECORD_VERSION);
    out.with_length(LengthWidth::Two, |record| {
        record.u8(HANDSHAKE_TYPE_CLIENT_HELLO);
        record.with_length(LengthWidth::Three, |hello| {
            hello_body(hello, host.as_bytes(), secrets)
        });
    });
    Some(out.bytes)
}

fn hello_body(hello: &mut Prefixed, host: &[u8], secrets: &HelloRandom) {
    hello.u16(LEGACY_VERSION);
    hello.slice(&secrets.random);
    hello.with_length(LengthWidth::One, |id| id.slice(&secrets.session_id));
    hello.with_length(LengthWidth::Two, |suites| suites.u16s(&CIPHER_SUITES));
    hello.with_length(LengthWidth::One, |methods| methods.u8(COMPRESSION_NULL));
    hello.with_length(LengthWidth::Two, |extensions| {
        extensions.extension(EXTENSION_SERVER_NAME, |data| server_name(data, host));
        extensions.extension(EXTENSION_SUPPORTED_GROUPS, |data| {
            data.with_length(LengthWidth::Two, |groups| groups.u16s(&GROUPS));
        });
        extensions.extension(EXTENSION_SIGNATURE_ALGORITHMS, |data| {
            data.with_length(LengthWidth::Two, |schemes| {
                schemes.u16s(&SIGNATURE_ALGORITHMS)
            });
        });
        extensions.extension(EXTENSION_ALPN, |data| {
            data.with_length(LengthWidth::Two, |names| {
                for protocol in ALPN_PROTOCOLS {
                    names.with_length(LengthWidth::One, |name| name.slice(protocol));
                }
            });
        });
        extensions.extension(EXTENSION_SUPPORTED_VERSIONS, |data| {
            data.with_length(LengthWidth::One, |versions| {
                versions.u16s(&SUPPORTED_VERSIONS)
            });
        });
        extensions.extension(EXTENSION_KEY_SHARE, |data| {
            data.with_length(LengthWidth::Two, |shares| {
                shares.u16(GROUP_X25519);
                shares.with_length(LengthWidth::Two, |key| key.slice(&secrets.key_share));
            });
        });
    });
}

fn server_name(data: &mut Prefixed, host: &[u8]) {
    data.with_length(LengthWidth::Two, |names| {
        names.u8(SERVER_NAME_TYPE_HOST);
        names.with_length(LengthWidth::Two, |name| name.slice(host));
    });
}

#[derive(Debug, Clone, Copy)]
enum LengthWidth {
    One = 1,
    Two = 2,
    Three = 3,
}

#[derive(Debug, Default)]
struct Prefixed {
    bytes: Vec<u8>,
}

impl Prefixed {
    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn u16s(&mut self, values: &[u16]) {
        for &value in values {
            self.u16(value);
        }
    }

    fn slice(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    fn extension(&mut self, extension_type: u16, fill: impl FnOnce(&mut Self)) {
        self.u16(extension_type);
        self.with_length(LengthWidth::Two, fill);
    }

    fn with_length(&mut self, width: LengthWidth, fill: impl FnOnce(&mut Self)) {
        let width = width as usize;
        let length_at = self.bytes.len();
        self.bytes.resize(length_at + width, 0);
        fill(self);
        let length = self.bytes.len() - length_at - width;
        let big_endian = length.to_be_bytes();
        let low_bytes = &big_endian[big_endian.len() - width..];
        self.bytes[length_at..length_at + width].copy_from_slice(low_bytes);
    }
}

#[cfg(test)]
mod tests;
