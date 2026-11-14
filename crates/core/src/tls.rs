//! TLS ClientHello inspection.

use core::ops::Range;

use crate::reader::Reader;

const CONTENT_HANDSHAKE: u8 = 0x16;
const HANDSHAKE_CLIENT_HELLO: u8 = 0x01;
const EXT_SERVER_NAME: u16 = 0x0000;
const NAME_TYPE_HOST: u8 = 0x00;

/// Finds the SNI host name in the start of a TLS stream.
///
/// `payload` is the first bytes the client sent on a TCP connection; it may be
/// cut short (a ClientHello can span several packets). Returns the byte range
/// of the host name inside `payload`, or `None` if `payload` is not a
/// ClientHello or the host name is not fully contained in it.
///
/// Milestone 0 version: handles one TLS record only. Milestone 1 hardens it.
pub fn client_hello_sni(payload: &[u8]) -> Option<Range<usize>> {
    let mut r = Reader::new(payload);

    // TLS record header.
    if r.u8()? != CONTENT_HANDSHAKE {
        return None;
    }
    r.skip(2)?; // legacy record version
    r.skip(2)?; // record length; ignored because the record may be truncated

    // Handshake header.
    if r.u8()? != HANDSHAKE_CLIENT_HELLO {
        return None;
    }
    r.u24()?; // handshake length; ignored for the same reason

    r.skip(2)?; // legacy version
    r.skip(32)?; // random
    let session_id_len = r.u8()?;
    r.skip(session_id_len.into())?;
    let cipher_suites_len = r.u16()?;
    r.skip(cipher_suites_len.into())?;
    let compression_len = r.u8()?;
    r.skip(compression_len.into())?;
    let extensions_len = usize::from(r.u16()?);
    let extensions_end = r.pos() + extensions_len;

    while r.pos() + 4 <= extensions_end {
        let ext_type = r.u16()?;
        let ext_len = usize::from(r.u16()?);
        if ext_type != EXT_SERVER_NAME {
            r.skip(ext_len)?;
            continue;
        }
        r.u16()?; // server_name_list length
        if r.u8()? != NAME_TYPE_HOST {
            return None;
        }
        let name_len = usize::from(r.u16()?);
        let start = r.pos();
        r.skip(name_len)?;
        if name_len == 0 {
            return None;
        }
        return Some(start..start + name_len);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal ClientHello carrying `host`, with one extension
    /// (supported_groups) before SNI and one after it.
    fn client_hello(host: &str) -> Vec<u8> {
        let mut sni = Vec::new();
        sni.extend_from_slice(&((host.len() + 3) as u16).to_be_bytes());
        sni.push(NAME_TYPE_HOST);
        sni.extend_from_slice(&(host.len() as u16).to_be_bytes());
        sni.extend_from_slice(host.as_bytes());

        let mut exts = Vec::new();
        exts.extend_from_slice(&[0x00, 0x0a, 0x00, 0x04, 0x00, 0x02, 0x00, 0x1d]);
        exts.extend_from_slice(&EXT_SERVER_NAME.to_be_bytes());
        exts.extend_from_slice(&(sni.len() as u16).to_be_bytes());
        exts.extend_from_slice(&sni);
        exts.extend_from_slice(&[0x00, 0x17, 0x00, 0x00]); // extended_master_secret

        let mut body = vec![0x03, 0x03];
        body.extend_from_slice(&[0xab; 32]);
        body.push(32);
        body.extend_from_slice(&[0xcd; 32]);
        body.extend_from_slice(&[0x00, 0x02, 0x13, 0x01]);
        body.extend_from_slice(&[0x01, 0x00]);
        body.extend_from_slice(&(exts.len() as u16).to_be_bytes());
        body.extend_from_slice(&exts);

        let mut hs = vec![HANDSHAKE_CLIENT_HELLO];
        hs.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        hs.extend_from_slice(&body);

        let mut rec = vec![CONTENT_HANDSHAKE, 0x03, 0x01];
        rec.extend_from_slice(&(hs.len() as u16).to_be_bytes());
        rec.extend_from_slice(&hs);
        rec
    }

    #[test]
    fn finds_sni() {
        let hello = client_hello("store.steampowered.com");
        let range = client_hello_sni(&hello).unwrap();
        assert_eq!(&hello[range], b"store.steampowered.com");
    }

    #[test]
    fn truncated_input_never_panics_and_needs_whole_name() {
        let hello = client_hello("steamcommunity.com");
        let full = client_hello_sni(&hello).unwrap();
        for cut in 0..hello.len() {
            let got = client_hello_sni(&hello[..cut]);
            if cut < full.end {
                assert_eq!(got, None, "cut at {cut}");
            } else {
                assert_eq!(got, Some(full.clone()), "cut at {cut}");
            }
        }
    }

    #[test]
    fn rejects_non_client_hello() {
        assert_eq!(client_hello_sni(b"GET / HTTP/1.1\r\n"), None);
        let mut hello = client_hello("example.com");
        hello[5] = 0x02; // ServerHello
        assert_eq!(client_hello_sni(&hello), None);
    }

    #[test]
    fn garbage_never_panics() {
        // Deterministic pseudo-random inputs, some starting like a ClientHello.
        let mut state: u32 = 0x1234_5678;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        for _ in 0..20_000 {
            let len = (next() % 600) as usize;
            let mut buf: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            if len > 6 && next() % 2 == 0 {
                buf[0] = CONTENT_HANDSHAKE;
                buf[5] = HANDSHAKE_CLIENT_HELLO;
            }
            if let Some(r) = client_hello_sni(&buf) {
                assert!(r.end <= buf.len());
            }
        }
    }
}
