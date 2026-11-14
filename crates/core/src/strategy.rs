use core::ops::Range;

use crate::segment::Order;

const LABEL_SEPARATOR: u8 = b'.';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitPosition {
    HostStart,
    DomainMid,
    Offset(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strategy {
    pub order: Order,
    pub positions: Vec<SplitPosition>,
}

impl Strategy {
    pub fn stream_offsets(
        &self,
        host_in_stream: Range<usize>,
        host: &[u8],
        matched_domain: Range<usize>,
    ) -> Vec<usize> {
        let mut offsets: Vec<usize> = self
            .positions
            .iter()
            .map(|position| {
                position.stream_offset(host_in_stream.start, host, matched_domain.clone())
            })
            .collect();
        offsets.sort_unstable();
        offsets.dedup();
        offsets
    }
}

impl SplitPosition {
    fn stream_offset(self, host_start: usize, host: &[u8], matched_domain: Range<usize>) -> usize {
        match self {
            SplitPosition::HostStart => host_start,
            SplitPosition::DomainMid => {
                let first_label_len = first_label_len(host, matched_domain.clone());
                host_start
                    .saturating_add(matched_domain.start)
                    .saturating_add(first_label_len / 2)
            }
            SplitPosition::Offset(offset) => offset,
        }
    }
}

fn first_label_len(host: &[u8], matched_domain: Range<usize>) -> usize {
    let domain = host.get(matched_domain).unwrap_or_default();
    domain
        .iter()
        .position(|&byte| byte == LABEL_SEPARATOR)
        .unwrap_or(domain.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::DomainList;

    const HOST_START_IN_STREAM: usize = 100;

    fn strategy(positions: &[SplitPosition]) -> Strategy {
        Strategy {
            order: Order::InSequence,
            positions: positions.to_vec(),
        }
    }

    fn offsets_for(positions: &[SplitPosition], host: &str, listed: &str) -> Vec<usize> {
        let domains = DomainList::new([listed]);
        let matched = domains.matched_domain(host.as_bytes()).unwrap();
        let host_in_stream = HOST_START_IN_STREAM..HOST_START_IN_STREAM + host.len();
        strategy(positions).stream_offsets(host_in_stream, host.as_bytes(), matched)
    }

    #[test]
    fn host_start_is_the_first_byte_of_the_host() {
        let offsets = offsets_for(
            &[SplitPosition::HostStart],
            "a.store.steampowered.com",
            "steampowered.com",
        );
        assert_eq!(offsets, [HOST_START_IN_STREAM]);
    }

    #[test]
    fn domain_mid_lands_in_the_middle_of_the_listed_second_level_label() {
        let host = "a.store.steampowered.com";
        let offsets = offsets_for(&[SplitPosition::DomainMid], host, "steampowered.com");
        let cut = offsets[0] - HOST_START_IN_STREAM;
        assert_eq!(&host[..cut], "a.store.steamp");
        assert_eq!(&host[cut..], "owered.com");
    }

    #[test]
    fn domain_mid_uses_the_first_label_of_the_matched_entry() {
        let host = "a.store.steampowered.com";
        let offsets = offsets_for(&[SplitPosition::DomainMid], host, "store.steampowered.com");
        let cut = offsets[0] - HOST_START_IN_STREAM;
        assert_eq!(&host[..cut], "a.st");
        assert_eq!(&host[cut..], "ore.steampowered.com");
    }

    #[test]
    fn domain_mid_of_a_single_character_label_is_its_start() {
        let host = "www.x.com";
        let offsets = offsets_for(&[SplitPosition::DomainMid], host, "x.com");
        let cut = offsets[0] - HOST_START_IN_STREAM;
        assert_eq!(&host[cut..], "x.com");
        let exact = offsets_for(
            &[SplitPosition::DomainMid, SplitPosition::HostStart],
            "x.com",
            "x.com",
        );
        assert_eq!(exact, [HOST_START_IN_STREAM]);
    }

    #[test]
    fn domain_mid_of_a_listed_name_without_dots_is_its_middle() {
        let host = "intranet";
        let offsets = offsets_for(&[SplitPosition::DomainMid], host, "intranet");
        assert_eq!(offsets, [HOST_START_IN_STREAM + host.len() / 2]);
    }

    #[test]
    fn domain_mid_ignores_a_trailing_dot_on_the_host() {
        let host = "store.steampowered.com.";
        let offsets = offsets_for(&[SplitPosition::DomainMid], host, "steampowered.com");
        let cut = offsets[0] - HOST_START_IN_STREAM;
        assert_eq!(&host[cut..], "owered.com.");
    }

    #[test]
    fn offsets_are_absolute_sorted_and_deduplicated() {
        let host = "a.store.steampowered.com";
        let offsets = offsets_for(
            &[
                SplitPosition::DomainMid,
                SplitPosition::Offset(1),
                SplitPosition::HostStart,
                SplitPosition::Offset(HOST_START_IN_STREAM),
                SplitPosition::Offset(1),
            ],
            host,
            "steampowered.com",
        );
        assert_eq!(
            offsets,
            [1, HOST_START_IN_STREAM, HOST_START_IN_STREAM + 14]
        );
    }

    #[test]
    fn extreme_inputs_never_panic() {
        let positions = [
            SplitPosition::HostStart,
            SplitPosition::DomainMid,
            SplitPosition::Offset(usize::MAX),
        ];
        let resolved =
            strategy(&positions).stream_offsets(usize::MAX..usize::MAX, b"", 5..usize::MAX);
        assert_eq!(resolved, [usize::MAX]);
        let reversed_host = Range { start: 3, end: 1 };
        let reversed_domain = Range { start: 2, end: 0 };
        let resolved = strategy(&positions).stream_offsets(reversed_host, b"ab", reversed_domain);
        assert_eq!(resolved, [3, 5, usize::MAX]);
    }
}
