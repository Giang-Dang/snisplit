use std::net::Ipv4Addr;

use crate::reader::Reader;

pub const TYPE_A: u16 = 1;
pub const TYPE_AAAA: u16 = 28;
pub const TYPE_HTTPS: u16 = 65;

const TYPE_CNAME: u16 = 5;
const CLASS_IN: u16 = 1;

const HEADER_LEN: usize = 12;
const FLAG_RESPONSE: u16 = 0x8000;
const FLAG_OPCODE_MASK: u16 = 0x7800;
const FLAG_TRUNCATED: u16 = 0x0200;
const FLAG_RECURSION_DESIRED: u16 = 0x0100;
const FLAG_RECURSION_AVAILABLE: u16 = 0x0080;
const FLAG_RCODE_MASK: u16 = 0x000f;
const RCODE_NO_ERROR: u16 = 0;
const RCODE_NAME_ERROR: u16 = 3;

const LABEL_TAG_MASK: u8 = 0xc0;
const LABEL_TAG_POINTER: u8 = 0xc0;
const LABEL_TAG_PLAIN: u8 = 0x00;
const POINTER_OFFSET_MASK: u8 = 0x3f;
const MAX_LABEL_LEN: usize = 63;
const MAX_NAME_WIRE_LEN: usize = 255;
const FIRST_NAME_BYTE: u8 = 0x21;
const LAST_NAME_BYTE: u8 = 0x7e;
const LABEL_SEPARATOR: u8 = 0x2e;

const POINTER_TO_QUESTION_NAME: u16 = 0xc00c;
const A_RDATA_LEN: usize = 4;
const ONE_QUESTION: u16 = 1;
const RECORD_TTL_LEN: usize = 4;
const HEADER_COUNTS_AFTER_AUTHORITIES_LEN: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub id: u16,
    pub name: String,
    pub qtype: u16,
    pub qclass: u16,
}

pub fn parse_query(packet: &[u8]) -> Option<Question> {
    read_query(packet).map(|query| query.question)
}

pub fn answer_a(packet: &[u8], address: Ipv4Addr, ttl: u32) -> Option<Vec<u8>> {
    let query = read_query(packet)?;
    let mut response = response_start(&query, 1, packet);
    response.extend_from_slice(&POINTER_TO_QUESTION_NAME.to_be_bytes());
    response.extend_from_slice(&TYPE_A.to_be_bytes());
    response.extend_from_slice(&CLASS_IN.to_be_bytes());
    response.extend_from_slice(&ttl.to_be_bytes());
    let rdata_len = u16::try_from(A_RDATA_LEN).ok()?;
    response.extend_from_slice(&rdata_len.to_be_bytes());
    response.extend_from_slice(&address.octets());
    Some(response)
}

pub fn answer_empty(packet: &[u8]) -> Option<Vec<u8>> {
    let query = read_query(packet)?;
    Some(response_start(&query, 0, packet))
}

pub fn build_a_query(id: u16, name: &str) -> Option<Vec<u8>> {
    let encoded_name = encode_name(name)?;
    let mut packet = Vec::with_capacity(HEADER_LEN + encoded_name.len() + 4);
    push_header(&mut packet, id, FLAG_RECURSION_DESIRED, 0);
    packet.extend_from_slice(&encoded_name);
    packet.extend_from_slice(&TYPE_A.to_be_bytes());
    packet.extend_from_slice(&CLASS_IN.to_be_bytes());
    Some(packet)
}

pub fn parse_a_answer(packet: &[u8], id: u16, name: &str) -> Option<Vec<Ipv4Addr>> {
    let wanted = normalized(name);
    let mut reader = Reader::new(packet);
    let header = Header::read(&mut reader)?;
    let rcode = header.flags & FLAG_RCODE_MASK;
    let acceptable = header.id == id
        && header.flags & FLAG_RESPONSE != 0
        && (rcode == RCODE_NO_ERROR || rcode == RCODE_NAME_ERROR)
        && header.flags & FLAG_TRUNCATED == 0
        && header.questions == ONE_QUESTION;
    if !acceptable {
        return None;
    }
    let question_name = read_name(packet, &mut reader, Pointers::Allowed)?;
    let question_type = reader.u16()?;
    let question_class = reader.u16()?;
    if question_name != wanted || question_type != TYPE_A || question_class != CLASS_IN {
        return None;
    }
    if rcode == RCODE_NAME_ERROR {
        return Some(Vec::new());
    }
    let mut answers = Answers::default();
    for _ in 0..header.answers {
        answers.add(packet, read_record(packet, &mut reader)?)?;
    }
    answers.addresses_of(wanted)
}

struct Header {
    id: u16,
    flags: u16,
    questions: u16,
    answers: u16,
    authorities: u16,
}

impl Header {
    fn read(reader: &mut Reader<'_>) -> Option<Self> {
        let header = Self {
            id: reader.u16()?,
            flags: reader.u16()?,
            questions: reader.u16()?,
            answers: reader.u16()?,
            authorities: reader.u16()?,
        };
        reader.skip(HEADER_COUNTS_AFTER_AUTHORITIES_LEN)?;
        Some(header)
    }
}

struct Query {
    question: Question,
    flags: u16,
    question_end: usize,
}

fn read_query(packet: &[u8]) -> Option<Query> {
    let mut reader = Reader::new(packet);
    let header = Header::read(&mut reader)?;
    let standard_query = header.flags & (FLAG_RESPONSE | FLAG_OPCODE_MASK) == 0;
    let bare_question =
        header.questions == ONE_QUESTION && header.answers == 0 && header.authorities == 0;
    if !standard_query || !bare_question {
        return None;
    }
    let name = read_name(packet, &mut reader, Pointers::Forbidden)?;
    let qtype = reader.u16()?;
    let qclass = reader.u16()?;
    Some(Query {
        question: Question {
            id: header.id,
            name,
            qtype,
            qclass,
        },
        flags: header.flags,
        question_end: reader.position(),
    })
}

fn response_start(query: &Query, answers: u16, packet: &[u8]) -> Vec<u8> {
    let flags = FLAG_RESPONSE | FLAG_RECURSION_AVAILABLE | (query.flags & FLAG_RECURSION_DESIRED);
    let mut response = Vec::with_capacity(query.question_end + 16);
    push_header(&mut response, query.question.id, flags, answers);
    response.extend_from_slice(&packet[HEADER_LEN..query.question_end]);
    response
}

fn push_header(packet: &mut Vec<u8>, id: u16, flags: u16, answers: u16) {
    for field in [id, flags, ONE_QUESTION, answers, 0, 0] {
        packet.extend_from_slice(&field.to_be_bytes());
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pointers {
    Allowed,
    Forbidden,
}

struct DecodedName {
    name: String,
    end: usize,
}

fn reader_at(packet: &[u8], position: usize) -> Option<Reader<'_>> {
    let mut reader = Reader::new(packet);
    reader.skip(position)?;
    Some(reader)
}

fn read_name(packet: &[u8], reader: &mut Reader<'_>, pointers: Pointers) -> Option<String> {
    let start = reader.position();
    let decoded = decode_name(packet, start, pointers)?;
    reader.skip(decoded.end.checked_sub(start)?)?;
    Some(decoded.name)
}

fn decode_name(packet: &[u8], start: usize, pointers: Pointers) -> Option<DecodedName> {
    let mut reader = reader_at(packet, start)?;
    let mut name = String::new();
    let mut wire_len = 1;
    let mut end_after_first_pointer = None;
    loop {
        let tag_and_length = reader.u8()?;
        match tag_and_length & LABEL_TAG_MASK {
            LABEL_TAG_PLAIN => {
                let length = usize::from(tag_and_length);
                if length == 0 {
                    break;
                }
                wire_len += length + 1;
                if wire_len > MAX_NAME_WIRE_LEN {
                    return None;
                }
                push_label(&mut name, reader.take(length)?)?;
            }
            LABEL_TAG_POINTER => {
                if pointers == Pointers::Forbidden {
                    return None;
                }
                let pointer_position = reader.position() - 1;
                let low = reader.u8()?;
                let target = u16::from_be_bytes([tag_and_length & POINTER_OFFSET_MASK, low]);
                let target = usize::from(target);
                if target >= pointer_position {
                    return None;
                }
                end_after_first_pointer.get_or_insert(reader.position());
                reader = reader_at(packet, target)?;
            }
            _ => return None,
        }
    }
    let end = end_after_first_pointer.unwrap_or(reader.position());
    Some(DecodedName { name, end })
}

fn is_name_byte(byte: u8) -> bool {
    (FIRST_NAME_BYTE..=LAST_NAME_BYTE).contains(&byte) && byte != LABEL_SEPARATOR
}

fn push_label(name: &mut String, label: &[u8]) -> Option<()> {
    if !label.iter().copied().all(is_name_byte) {
        return None;
    }
    if !name.is_empty() {
        name.push(char::from(LABEL_SEPARATOR));
    }
    name.extend(
        label
            .iter()
            .map(|byte| char::from(byte.to_ascii_lowercase())),
    );
    Some(())
}

fn normalized(name: &str) -> String {
    name.strip_suffix('.').unwrap_or(name).to_ascii_lowercase()
}

fn encode_name(name: &str) -> Option<Vec<u8>> {
    let name = name.strip_suffix('.').unwrap_or(name);
    let mut encoded = Vec::with_capacity(name.len() + 2);
    for label in name.split(char::from(LABEL_SEPARATOR)) {
        let valid =
            !label.is_empty() && label.len() <= MAX_LABEL_LEN && label.bytes().all(is_name_byte);
        if !valid {
            return None;
        }
        encoded.push(u8::try_from(label.len()).ok()?);
        encoded.extend_from_slice(label.as_bytes());
    }
    encoded.push(0);
    (encoded.len() <= MAX_NAME_WIRE_LEN).then_some(encoded)
}

struct Record<'a> {
    owner: String,
    record_type: u16,
    class: u16,
    rdata_start: usize,
    rdata: &'a [u8],
}

fn read_record<'a>(packet: &'a [u8], reader: &mut Reader<'a>) -> Option<Record<'a>> {
    let owner = read_name(packet, reader, Pointers::Allowed)?;
    let record_type = reader.u16()?;
    let class = reader.u16()?;
    reader.skip(RECORD_TTL_LEN)?;
    let rdata_len = usize::from(reader.u16()?);
    let rdata_start = reader.position();
    let rdata = reader.take(rdata_len)?;
    Some(Record {
        owner,
        record_type,
        class,
        rdata_start,
        rdata,
    })
}

#[derive(Default)]
struct Answers {
    aliases: Vec<(String, String)>,
    addresses: Vec<(String, Ipv4Addr)>,
}

impl Answers {
    fn add(&mut self, packet: &[u8], record: Record<'_>) -> Option<()> {
        if record.class != CLASS_IN {
            return Some(());
        }
        match record.record_type {
            TYPE_A => {
                let octets: [u8; A_RDATA_LEN] = record.rdata.try_into().ok()?;
                self.addresses.push((record.owner, Ipv4Addr::from(octets)));
            }
            TYPE_CNAME => {
                let target = decode_name(packet, record.rdata_start, Pointers::Allowed)?;
                if target.end > record.rdata_start + record.rdata.len() {
                    return None;
                }
                self.aliases.push((record.owner, target.name));
            }
            _ => {}
        }
        Some(())
    }

    fn addresses_of(&self, start: String) -> Option<Vec<Ipv4Addr>> {
        let mut current = start;
        for _ in 0..=self.aliases.len() {
            let alias = self.aliases.iter().find(|(owner, _)| *owner == current);
            let Some((_, target)) = alias else {
                return Some(
                    self.addresses
                        .iter()
                        .filter(|(owner, _)| *owner == current)
                        .map(|(_, address)| *address)
                        .collect(),
                );
            };
            current.clone_from(target);
        }
        None
    }
}
