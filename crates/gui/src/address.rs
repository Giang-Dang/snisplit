const SCHEME_END: &str = "://";
const PATH_STARTS: [char; 3] = ['/', '?', '#'];

pub fn site_address(text: &str) -> Option<String> {
    let host = without_port(without_user(before_path(without_scheme(text.trim()))));
    let host = host.strip_suffix('.').unwrap_or(host);
    let domain = snisplit::parse_domain(host).ok()?;
    let top_label = domain.rsplit('.').next()?;
    let numeric = top_label.bytes().all(|byte| byte.is_ascii_digit());
    (!numeric).then_some(domain)
}

fn without_scheme(text: &str) -> &str {
    match text.split_once(SCHEME_END) {
        Some((scheme, rest)) if is_scheme(scheme) => rest,
        _ => text,
    }
}

fn is_scheme(text: &str) -> bool {
    let mut bytes = text.bytes();
    let starts_with_letter = bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic());
    starts_with_letter
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

fn before_path(text: &str) -> &str {
    text.find(PATH_STARTS).map_or(text, |end| &text[..end])
}

fn without_user(text: &str) -> &str {
    text.rsplit_once('@').map_or(text, |(_, host)| host)
}

fn without_port(text: &str) -> &str {
    match text.rsplit_once(':') {
        Some((host, port)) if port.bytes().all(|byte| byte.is_ascii_digit()) => host,
        _ => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_domain_is_kept_and_lower_cased() {
        assert_eq!(site_address("Example.NET"), Some("example.net".to_string()));
        assert_eq!(
            site_address("  store.steampowered.com  "),
            Some("store.steampowered.com".to_string())
        );
    }

    #[test]
    fn a_pasted_link_loses_scheme_user_port_path_query_fragment_and_trailing_dot() {
        assert_eq!(
            site_address(" https://owner:secret@Example.net.:8443/page/one?x=1&y=2#top "),
            Some("example.net".to_string())
        );
        assert_eq!(
            site_address("http://example.org/"),
            Some("example.org".to_string())
        );
        assert_eq!(
            site_address("example.com?next=http://example.org/"),
            Some("example.com".to_string())
        );
        assert_eq!(
            site_address("example.com#section"),
            Some("example.com".to_string())
        );
        assert_eq!(
            site_address("example.com:"),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn text_that_is_not_a_domain_after_shortening_is_refused() {
        for typed in [
            "my site",
            "",
            "https://",
            "localhost",
            "localhost:8080",
            "192.0.2.10",
            "http://192.0.2.10/",
            "[2001:db8::1]:443",
            "example..com",
            "-example.com",
            "exa_mple.com",
            "example.com:port",
        ] {
            assert_eq!(site_address(typed), None, "{typed:?}");
        }
    }
}
