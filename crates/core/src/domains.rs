use core::iter;
use core::ops::Range;

const LABEL_SEPARATOR: u8 = b'.';

#[derive(Debug, Clone, Default)]
pub struct DomainList {
    domains: Vec<Vec<u8>>,
}

impl DomainList {
    pub fn new<I: IntoIterator<Item = S>, S: AsRef<str>>(domains: I) -> Self {
        Self {
            domains: domains
                .into_iter()
                .filter_map(|domain| normalised(domain.as_ref()))
                .collect(),
        }
    }

    pub fn matched_domain(&self, host: &[u8]) -> Option<Range<usize>> {
        let name = without_trailing_dot(host);
        label_starts(name)
            .find(|&start| {
                name.get(start..)
                    .is_some_and(|suffix| self.contains(suffix))
            })
            .map(|start| start..name.len())
    }

    fn contains(&self, name: &[u8]) -> bool {
        self.domains
            .iter()
            .any(|domain| domain.eq_ignore_ascii_case(name))
    }
}

fn normalised(domain: &str) -> Option<Vec<u8>> {
    let domain = without_trailing_dot(domain.as_bytes());
    (!domain.is_empty() && domain.is_ascii()).then(|| domain.to_ascii_lowercase())
}

fn without_trailing_dot(name: &[u8]) -> &[u8] {
    name.strip_suffix(&[LABEL_SEPARATOR]).unwrap_or(name)
}

fn label_starts(name: &[u8]) -> impl Iterator<Item = usize> + '_ {
    let after_separators = name
        .iter()
        .enumerate()
        .filter(|&(_, &byte)| byte == LABEL_SEPARATOR)
        .map(|(separator, _)| separator + 1);
    iter::once(0).chain(after_separators)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEAM: &str = "steampowered.com";
    const STEAM_STORE: &str = "store.steampowered.com";

    fn matched<'a>(list: &DomainList, host: &'a str) -> Option<&'a str> {
        list.matched_domain(host.as_bytes())
            .map(|range| host.get(range).unwrap())
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
    fn exact_host_matches() {
        let list = DomainList::new([STEAM]);
        assert_eq!(list.matched_domain(STEAM.as_bytes()), Some(0..STEAM.len()));
    }

    #[test]
    fn subdomain_matches_with_range_of_listed_domain() {
        let list = DomainList::new([STEAM]);
        let host = "sub.steampowered.com";
        assert_eq!(list.matched_domain(host.as_bytes()), Some(4..host.len()));
        assert_eq!(matched(&list, "a.store.steampowered.com"), Some(STEAM));
    }

    #[test]
    fn matches_only_at_label_boundaries() {
        let list = DomainList::new([STEAM]);
        assert_eq!(matched(&list, "notsteampowered.com"), None);
        assert_eq!(matched(&list, "steampowered.com.evil"), None);
        assert_eq!(matched(&list, "powered.com"), None);
        assert_eq!(matched(&list, "com"), None);
    }

    #[test]
    fn longest_listed_domain_wins() {
        let list = DomainList::new([STEAM, STEAM_STORE]);
        assert_eq!(
            matched(&list, "a.store.steampowered.com"),
            Some(STEAM_STORE)
        );
        assert_eq!(matched(&list, "a.cdn.steampowered.com"), Some(STEAM));
        let reversed = DomainList::new([STEAM_STORE, STEAM]);
        assert_eq!(
            matched(&reversed, "a.store.steampowered.com"),
            Some(STEAM_STORE)
        );
    }

    #[test]
    fn matching_ignores_ascii_case() {
        let list = DomainList::new(["SteamPowered.COM"]);
        assert_eq!(
            matched(&list, "STORE.steampowered.Com"),
            Some("steampowered.Com")
        );
        assert_eq!(matched(&list, "steampowered.com"), Some(STEAM));
    }

    #[test]
    fn one_trailing_dot_is_ignored_on_host_and_entry() {
        let list = DomainList::new(["steampowered.com."]);
        assert_eq!(matched(&list, "store.steampowered.com."), Some(STEAM));
        assert_eq!(matched(&list, STEAM_STORE), Some(STEAM));
        assert_eq!(matched(&list, "store.steampowered.com.."), None);
    }

    #[test]
    fn empty_and_non_ascii_entries_are_ignored() {
        let list = DomainList::new(["", ".", "b\u{fc}cher.de", "com."]);
        assert_eq!(matched(&list, ""), None);
        assert_eq!(matched(&list, "."), None);
        assert_eq!(matched(&list, "x.de"), None);
        assert_eq!(matched(&list, "example.com"), Some("com"));
    }

    #[test]
    fn unusual_hosts_never_match_or_panic() {
        let list = DomainList::new([STEAM]);
        for host in [
            b"".as_slice(),
            b".",
            b"..",
            b"\xff\xfe",
            b"\xffsteampowered.com",
        ] {
            assert_eq!(list.matched_domain(host), None, "{host:?}");
        }
        assert_eq!(
            list.matched_domain(b".steampowered.com"),
            Some(1..1 + STEAM.len())
        );
    }

    #[test]
    fn random_hosts_never_panic() {
        let list = DomainList::new([STEAM, STEAM_STORE, "a.b"]);
        let alphabet = b"ab.steampowered.comAB\xff";
        let mut rng = XorShift(0x0bad_cafe);
        for _ in 0..20_000 {
            let len = (rng.next() % 40) as usize;
            let host: Vec<u8> = (0..len)
                .map(|_| alphabet[rng.next() as usize % alphabet.len()])
                .collect();
            if let Some(range) = list.matched_domain(&host) {
                assert!(range.end <= host.len() && range.start < range.end);
            }
        }
    }
}
