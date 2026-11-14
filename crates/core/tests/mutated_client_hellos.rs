mod common;

use std::ops::Range;

use common::{fixture_stems, read_stream};
use snisplit_core::inspect::Inspection;
use snisplit_core::tls::inspect_client_hello;

const XORSHIFT_SEED: u32 = 0x2545_f491;
const RANDOM_OVERWRITES_PER_FIXTURE: usize = 3_000;
const MAX_BYTES_PER_RANDOM_OVERWRITE: usize = 6;
const BITS_PER_BYTE: u8 = 8;
const BITS_PER_LENGTH_BYTE: usize = 8;

const TLS_RECORD_LENGTH_AT: usize = 3;
const HANDSHAKE_TYPE_LEN: usize = 1;
const HANDSHAKE_LENGTH_WIDTH: usize = 3;
const PROTOCOL_VERSION_LEN: usize = 2;
const RANDOM_LEN: usize = 32;
const EXTENSION_TYPE_LEN: usize = 2;
const EXTENSION_SERVER_NAME: usize = 0;
const SERVER_NAME_TYPE_LEN: usize = 1;

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

    fn byte(&mut self) -> u8 {
        self.next().to_le_bytes()[0]
    }
}

#[derive(Debug, Clone, Copy)]
struct LengthField {
    offset: usize,
    width: usize,
}

impl LengthField {
    fn end(self) -> usize {
        self.offset + self.width
    }

    fn max_value(self) -> usize {
        (1usize << (self.width * BITS_PER_LENGTH_BYTE)) - 1
    }

    fn read(self, bytes: &[u8]) -> usize {
        bytes[self.offset..self.end()]
            .iter()
            .fold(0, |value, &byte| {
                (value << BITS_PER_LENGTH_BYTE) | usize::from(byte)
            })
    }

    fn write(self, bytes: &mut [u8], value: usize) {
        let slots = bytes[self.offset..self.end()].iter_mut().rev();
        for (index, slot) in slots.enumerate() {
            *slot = (value >> (index * BITS_PER_LENGTH_BYTE)) as u8;
        }
    }

    fn corrupt_values(self, original: usize) -> [usize; 4] {
        [
            0,
            self.max_value(),
            original.saturating_sub(1),
            (original + 1).min(self.max_value()),
        ]
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
    length_fields: Vec<LengthField>,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            position: 0,
            length_fields: Vec::new(),
        }
    }

    fn skip(&mut self, len: usize) {
        self.position += len;
    }

    fn read(&mut self, width: usize) -> usize {
        let field = LengthField {
            offset: self.position,
            width,
        };
        self.skip(width);
        field.read(self.bytes)
    }

    fn length(&mut self, width: usize) -> usize {
        self.length_fields.push(LengthField {
            offset: self.position,
            width,
        });
        self.read(width)
    }

    fn length_then_skip(&mut self, width: usize) {
        let len = self.length(width);
        self.skip(len);
    }
}

fn locate_length_fields(hello: &[u8]) -> Vec<LengthField> {
    let mut cursor = Cursor::new(hello);
    cursor.skip(TLS_RECORD_LENGTH_AT);
    cursor.length(2);
    cursor.skip(HANDSHAKE_TYPE_LEN);
    cursor.length(HANDSHAKE_LENGTH_WIDTH);
    cursor.skip(PROTOCOL_VERSION_LEN + RANDOM_LEN);
    cursor.length_then_skip(1);
    cursor.length_then_skip(2);
    cursor.length_then_skip(1);
    let extensions_len = cursor.length(2);
    let extensions_end = cursor.position + extensions_len;
    while cursor.position < extensions_end {
        let extension_type = cursor.read(EXTENSION_TYPE_LEN);
        if extension_type == EXTENSION_SERVER_NAME {
            cursor.length(2);
            cursor.length(2);
            cursor.skip(SERVER_NAME_TYPE_LEN);
        }
        cursor.length_then_skip(2);
    }
    cursor.length_fields
}

fn assert_host_inside_input(stem: &str, input: &[u8], context: &str) {
    if let Inspection::Host(host) = inspect_client_hello(input) {
        assert!(
            host.start < host.end && host.end <= input.len(),
            "{stem}: {context} gave host {host:?} for input of {} bytes",
            input.len()
        );
    }
}

fn streams() -> Vec<(String, Vec<u8>)> {
    fixture_stems()
        .into_iter()
        .map(|stem| {
            let stream = read_stream(&stem);
            (stem, stream)
        })
        .collect()
}

fn host_of(stem: &str, stream: &[u8]) -> Range<usize> {
    match inspect_client_hello(stream) {
        Inspection::Host(host) => host,
        other => panic!("{stem}: unmutated stream gave {other:?}"),
    }
}

#[test]
fn truncation_at_every_length_never_misreports_the_host() {
    for (stem, stream) in streams() {
        let host = host_of(&stem, &stream);
        for cut in 0..=stream.len() {
            let result = inspect_client_hello(&stream[..cut]);
            let allowed = result == Inspection::NeedMoreData
                || (result == Inspection::Host(host.clone()) && cut >= host.end);
            assert!(allowed, "{stem}: cut {cut} gave {result:?}");
        }
    }
}

#[test]
fn single_bit_flip_at_every_byte_never_panics() {
    for (stem, mut stream) in streams() {
        for index in 0..stream.len() {
            for bit in 0..BITS_PER_BYTE {
                stream[index] ^= 1 << bit;
                let context = format!("flip bit {bit} of byte {index}");
                assert_host_inside_input(&stem, &stream, &context);
                stream[index] ^= 1 << bit;
            }
        }
    }
}

#[test]
fn located_length_fields_include_the_host_name_length() {
    for (stem, stream) in streams() {
        let host = host_of(&stem, &stream);
        let name_length_field = locate_length_fields(&stream)
            .into_iter()
            .find(|field| field.end() == host.start)
            .unwrap_or_else(|| panic!("{stem}: host name length field not located"));
        assert_eq!(name_length_field.read(&stream), host.len(), "{stem}");
    }
}

#[test]
fn corrupted_length_fields_never_panic() {
    for (stem, stream) in streams() {
        for field in locate_length_fields(&stream) {
            let original = field.read(&stream);
            for value in field.corrupt_values(original) {
                let mut mutated = stream.clone();
                field.write(&mut mutated, value);
                let context = format!("{field:?} set to {value}");
                assert_host_inside_input(&stem, &mutated, &context);
                for cut in 0..=mutated.len() {
                    assert_host_inside_input(&stem, &mutated[..cut], &context);
                }
            }
        }
    }
}

#[test]
fn random_byte_overwrites_never_panic() {
    let mut random = XorShift(XORSHIFT_SEED);
    for (stem, stream) in streams() {
        for round in 0..RANDOM_OVERWRITES_PER_FIXTURE {
            let mut mutated = stream.clone();
            let overwrites = 1 + random.below(MAX_BYTES_PER_RANDOM_OVERWRITE);
            for _ in 0..overwrites {
                let index = random.below(mutated.len());
                mutated[index] = random.byte();
            }
            let cut = random.below(mutated.len() + 1);
            assert_host_inside_input(&stem, &mutated, &format!("round {round}"));
            assert_host_inside_input(&stem, &mutated[..cut], &format!("round {round} cut {cut}"));
        }
    }
}
