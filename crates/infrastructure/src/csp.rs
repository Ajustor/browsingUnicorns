//! A page's Content Security Policy, opened to the extensions' frames.
//!
//! Chrome lets a content script frame its extension's pages
//! (`chrome-extension://…`) whatever the page's policy says. Here they are
//! web origins (`https://bu-ext.<id>`), which a `frame-src 'self'` blocks:
//! the policy gets them added, and nothing else changes.

/// `header` (a `Content-Security-Policy` value, possibly several policies
/// separated by commas) with `origins` allowed as frames, or `None` when it
/// allows them already.
pub fn allow_frames(header: &str, origins: &[String]) -> Option<String> {
    if origins.is_empty() {
        return None;
    }
    let mut changed = false;
    let policies: Vec<String> = header
        .split(',')
        .map(|policy| match allow_in_policy(policy, origins) {
            Some(p) => {
                changed = true;
                p
            }
            None => policy.trim().to_owned(),
        })
        .collect();
    changed.then(|| policies.join(", "))
}

fn allow_in_policy(policy: &str, origins: &[String]) -> Option<String> {
    let mut directives: Vec<Vec<String>> = policy
        .split(';')
        .map(|d| {
            d.split_ascii_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|d| !d.is_empty())
        .collect();
    let named = |name: &str| {
        directives
            .iter()
            .position(|d| d[0].eq_ignore_ascii_case(name))
    };
    // What decides frames: frame-src, else child-src, else default-src.
    let (index, own) = match named("frame-src") {
        Some(i) => (i, true),
        None => (named("child-src").or_else(|| named("default-src"))?, false),
    };
    let sources = &directives[index][1..];
    let missing: Vec<&String> = origins.iter().filter(|o| !allows(sources, o)).collect();
    if missing.is_empty() {
        return None;
    }
    let mut frame_src: Vec<String> = sources
        .iter()
        .filter(|s| !s.eq_ignore_ascii_case("'none'"))
        .cloned()
        .collect();
    frame_src.extend(missing.into_iter().cloned());
    frame_src.insert(0, "frame-src".to_owned());
    if own {
        directives[index] = frame_src;
    } else {
        // Its own frame-src: the fallback keeps governing the rest.
        directives.push(frame_src);
    }
    Some(
        directives
            .iter()
            .map(|d| d.join(" "))
            .collect::<Vec<_>>()
            .join("; "),
    )
}

/// Whether `sources` let `origin` (`https://host`) be framed.
fn allows(sources: &[String], origin: &str) -> bool {
    sources.iter().any(|s| {
        let s = s.trim_end_matches('/');
        s == "*" || s.eq_ignore_ascii_case("https:") || s.eq_ignore_ascii_case(origin)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext() -> Vec<String> {
        vec!["https://bu-ext.abc".to_owned()]
    }

    #[test]
    fn adds_the_extensions_to_frame_src() {
        assert_eq!(
            allow_frames("default-src 'self'; frame-src 'self'", &ext()).as_deref(),
            Some("default-src 'self'; frame-src 'self' https://bu-ext.abc")
        );
    }

    #[test]
    fn frame_src_none_becomes_the_extensions_only() {
        assert_eq!(
            allow_frames("frame-src 'none'; script-src 'self'", &ext()).as_deref(),
            Some("frame-src https://bu-ext.abc; script-src 'self'")
        );
    }

    #[test]
    fn a_fallback_gets_a_frame_src_of_its_own() {
        assert_eq!(
            allow_frames("default-src 'self' cdn.test", &ext()).as_deref(),
            Some("default-src 'self' cdn.test; frame-src 'self' cdn.test https://bu-ext.abc")
        );
        // child-src comes before default-src.
        assert_eq!(
            allow_frames("default-src 'none'; child-src 'self'", &ext()).as_deref(),
            Some("default-src 'none'; child-src 'self'; frame-src 'self' https://bu-ext.abc")
        );
    }

    #[test]
    fn leaves_alone_what_allows_them_already() {
        for csp in [
            "script-src 'self'",
            "frame-src *",
            "frame-src https:",
            "frame-src https://bu-ext.abc/",
            "frame-ancestors 'self'",
        ] {
            assert_eq!(allow_frames(csp, &ext()), None, "{csp}");
        }
        assert_eq!(allow_frames("frame-src 'self'", &[]), None);
    }

    #[test]
    fn each_policy_of_the_header_is_opened() {
        assert_eq!(
            allow_frames("frame-src 'self', script-src 'none'", &ext()).as_deref(),
            Some("frame-src 'self' https://bu-ext.abc, script-src 'none'")
        );
    }
}
