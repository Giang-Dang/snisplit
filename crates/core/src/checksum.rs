use std::net::{Ipv4Addr, Ipv6Addr};

const PROTOCOL_TCP: u64 = 6;
const WORD_MASK: u64 = 0xffff;
const WORD_BITS: u32 = 16;

pub(crate) fn checksum(data: &[u8]) -> u16 {
    !ones_complement_sum(data)
}

pub(crate) fn ones_complement_sum(data: &[u8]) -> u16 {
    fold(sum_of_words(data))
}

pub(crate) fn tcp_checksum(pseudo_header_sum: u64, segment: &[u8]) -> u16 {
    !fold(pseudo_header_sum + sum_of_words(segment))
}

pub(crate) fn ipv4_pseudo_header_sum(src: Ipv4Addr, dst: Ipv4Addr, tcp_len: u16) -> u64 {
    sum_of_words(&src.octets()) + sum_of_words(&dst.octets()) + PROTOCOL_TCP + u64::from(tcp_len)
}

pub(crate) fn ipv6_pseudo_header_sum(src: Ipv6Addr, dst: Ipv6Addr, tcp_len: u32) -> u64 {
    sum_of_words(&src.octets())
        + sum_of_words(&dst.octets())
        + sum_of_words(&tcp_len.to_be_bytes())
        + PROTOCOL_TCP
}

fn sum_of_words(data: &[u8]) -> u64 {
    data.chunks(2)
        .map(|word| match *word {
            [high, low] => u64::from(u16::from_be_bytes([high, low])),
            [high] => u64::from(u16::from_be_bytes([high, 0])),
            _ => 0,
        })
        .sum()
}

fn fold(mut sum: u64) -> u16 {
    while sum > WORD_MASK {
        sum = (sum & WORD_MASK) + (sum >> WORD_BITS);
    }
    sum as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    const RFC_1071_EXAMPLE: [u8; 8] = [0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];
    const RFC_1071_EXAMPLE_SUM: u16 = 0xddf2;
    const RFC_1071_EXAMPLE_CHECKSUM: u16 = 0x220d;

    const PUBLISHED_IPV4_HEADER: [u8; 20] = [
        0x45, 0x00, 0x00, 0x73, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0xb8, 0x61, 0xc0, 0xa8, 0x00,
        0x01, 0xc0, 0xa8, 0x00, 0xc7,
    ];
    const PUBLISHED_IPV4_HEADER_CHECKSUM: u16 = 0xb861;
    const IPV4_CHECKSUM_FIELD: core::ops::Range<usize> = 10..12;

    const VALID_DATA_CHECKSUM: u16 = 0;

    #[test]
    fn rfc_1071_example() {
        assert_eq!(ones_complement_sum(&RFC_1071_EXAMPLE), RFC_1071_EXAMPLE_SUM);
        assert_eq!(checksum(&RFC_1071_EXAMPLE), RFC_1071_EXAMPLE_CHECKSUM);
    }

    #[test]
    fn published_ipv4_header() {
        let mut header = PUBLISHED_IPV4_HEADER;
        assert_eq!(checksum(&header), VALID_DATA_CHECKSUM);
        header[IPV4_CHECKSUM_FIELD].fill(0);
        assert_eq!(checksum(&header), PUBLISHED_IPV4_HEADER_CHECKSUM);
    }

    #[test]
    fn odd_length_is_padded_with_a_zero_low_byte() {
        assert_eq!(ones_complement_sum(&[0x01]), 0x0100);
        assert_eq!(checksum(&[0x01]), 0xfeff);
        assert_eq!(ones_complement_sum(&[0x00, 0x01, 0xf2]), 0xf201);
        assert_eq!(checksum(&[0x00, 0x01, 0xf2]), 0x0dfe);
    }

    #[test]
    fn carries_are_folded_back() {
        assert_eq!(ones_complement_sum(&[0xff, 0xff, 0x00, 0x01]), 0x0001);
        assert_eq!(ones_complement_sum(&[0xff; 4]), 0xffff);
        assert_eq!(checksum(&[0xff; 4]), 0x0000);
        assert_eq!(ones_complement_sum(&[0xff; 65534]), 0xffff);
    }

    #[test]
    fn empty_data() {
        assert_eq!(ones_complement_sum(&[]), 0x0000);
        assert_eq!(checksum(&[]), 0xffff);
    }

    #[test]
    fn ipv4_pseudo_header_sum_matches_hand_addition() {
        let src = Ipv4Addr::new(192, 168, 0, 1);
        let dst = Ipv4Addr::new(192, 168, 0, 199);
        let tcp_len = 0x0014;
        let by_hand = 0xc0a8 + 0x0001 + 0xc0a8 + 0x00c7 + 0x0006 + 0x0014;
        assert_eq!(ipv4_pseudo_header_sum(src, dst, tcp_len), by_hand);
    }

    #[test]
    fn ipv6_pseudo_header_sum_matches_hand_addition() {
        let src = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0x0001);
        let dst = Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0x0001);
        let tcp_len = 0x0001_0020;
        let by_hand = 0xfe80 + 0x0001 + 0x2001 + 0x0db8 + 0x0001 + 0x0001 + 0x0020 + 0x0006;
        assert_eq!(ipv6_pseudo_header_sum(src, dst, tcp_len), by_hand);
    }

    #[test]
    fn tcp_checksum_adds_the_pseudo_header_to_the_segment() {
        let pseudo_header_sum = 0x0001_2345;
        let segment = [0x10, 0x00, 0x02];
        let by_hand = !(0x2345 + 0x0001 + 0x1000 + 0x0200);
        assert_eq!(tcp_checksum(pseudo_header_sum, &segment), by_hand);
    }
}
