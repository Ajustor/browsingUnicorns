use crate::{SearchEngine, WebAddress};

/// Turns what was typed in the address bar into the address to load:
/// a full address as is, something that looks like a host (`rust-lang.org`,
/// `localhost:3000/docs`) as a web address, anything else as a search.
pub fn resolve_omnibox(input: &str, search: &SearchEngine) -> Option<WebAddress> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if let Ok(address) = WebAddress::parse(input)
        && (input.contains("://") || address.scheme() == "about")
    {
        return Some(address);
    }
    if !input.contains(char::is_whitespace)
        && let Some(scheme) = host_scheme(input)
        && let Ok(address) = WebAddress::parse(&format!("{scheme}://{input}"))
    {
        return Some(address);
    }
    search.results_for(input)
}

/// The scheme to prefix `input` with when it starts with a host name,
/// or `None` when it does not look like one.
fn host_scheme(input: &str) -> Option<&'static str> {
    let authority = input.split(['/', '?', '#']).next().unwrap_or_default();
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    if let Some(port) = port
        && (port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    if host.eq_ignore_ascii_case("localhost") || is_ipv4(host) {
        return Some("http");
    }
    let labels: Vec<&str> = host.split('.').collect();
    let valid_label = |l: &&str| {
        !l.is_empty()
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    let tld = labels.last()?;
    (labels.len() >= 2
        && labels.iter().all(valid_label)
        && tld.len() >= 2
        && tld.bytes().all(|b| b.is_ascii_alphabetic()))
    .then_some("https")
}

fn is_ipv4(host: &str) -> bool {
    host.parse::<std::net::Ipv4Addr>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(input: &str) -> String {
        resolve_omnibox(input, &SearchEngine::duckduckgo())
            .map(|a| a.to_string())
            .unwrap_or_default()
    }

    #[test]
    fn full_addresses_are_kept() {
        assert_eq!(resolve("http://example.com/x"), "http://example.com/x");
        assert_eq!(resolve("  https://a.b  "), "https://a.b/");
        assert_eq!(resolve("about:blank"), "about:blank");
    }

    #[test]
    fn host_names_get_https() {
        assert_eq!(resolve("rust-lang.org"), "https://rust-lang.org/");
        assert_eq!(
            resolve("docs.rs/egui?search=x"),
            "https://docs.rs/egui?search=x"
        );
    }

    #[test]
    fn local_hosts_get_http() {
        assert_eq!(resolve("localhost:3000/a"), "http://localhost:3000/a");
        assert_eq!(resolve("127.0.0.1"), "http://127.0.0.1/");
    }

    #[test]
    fn everything_else_is_a_search() {
        assert_eq!(resolve("rust"), "https://duckduckgo.com/?q=rust");
        assert_eq!(
            resolve("rust.lang book"),
            "https://duckduckgo.com/?q=rust.lang+book"
        );
        assert_eq!(
            resolve("version 1.2"),
            "https://duckduckgo.com/?q=version+1.2"
        );
        assert_eq!(resolve("1.2"), "https://duckduckgo.com/?q=1.2");
        assert_eq!(
            resolve("mailto:a@b.com"),
            "https://duckduckgo.com/?q=mailto%3Aa%40b.com"
        );
    }

    #[test]
    fn empty_input_resolves_to_nothing() {
        assert_eq!(resolve("   "), "");
    }
}
