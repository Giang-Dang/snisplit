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
mod tests;
