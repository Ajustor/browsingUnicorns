//! Network rules of extensions (Chrome's `declarativeNetRequest`), and the
//! matcher that decides, for each request a page makes, whether it goes
//! through, is blocked or is redirected to a file of the extension.
//!
//! Used where the web view has no extension engine of its own (Android,
//! WebKit): the rules are read from the extensions' rulesets and applied by
//! the browser.

use std::collections::HashMap;

use crate::ExtensionId;

/// What a request loads (`declarativeNetRequest.ResourceType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceType {
    MainFrame,
    SubFrame,
    Stylesheet,
    Script,
    Image,
    Font,
    Object,
    XmlHttpRequest,
    Ping,
    CspReport,
    Media,
    WebSocket,
    WebTransport,
    WebBundle,
    Other,
}

impl ResourceType {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "main_frame" => Self::MainFrame,
            "sub_frame" => Self::SubFrame,
            "stylesheet" => Self::Stylesheet,
            "script" => Self::Script,
            "image" => Self::Image,
            "font" => Self::Font,
            "object" => Self::Object,
            "xmlhttprequest" => Self::XmlHttpRequest,
            "ping" => Self::Ping,
            "csp_report" => Self::CspReport,
            "media" => Self::Media,
            "websocket" => Self::WebSocket,
            "webtransport" => Self::WebTransport,
            "webbundle" => Self::WebBundle,
            "other" => Self::Other,
            _ => return None,
        })
    }

    /// A best guess from the address and the `Accept` header, for web views
    /// that do not say what a request is for (Android).
    pub fn guess(url: &str, accept: &str) -> Option<Self> {
        let path = url.split(['?', '#']).next().unwrap_or(url);
        let ext = path
            .rsplit('/')
            .next()
            .and_then(|last| last.rsplit_once('.'))
            .map(|(_, e)| e.to_ascii_lowercase());
        let by_ext = match ext.as_deref() {
            Some("js" | "mjs") => Some(Self::Script),
            Some("css") => Some(Self::Stylesheet),
            Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "avif" | "bmp") => {
                Some(Self::Image)
            }
            Some("woff" | "woff2" | "ttf" | "otf" | "eot") => Some(Self::Font),
            Some("mp4" | "webm" | "mp3" | "m3u8" | "ogg" | "wav" | "m4a") => Some(Self::Media),
            _ => None,
        };
        by_ext.or_else(|| {
            let accept = accept.to_ascii_lowercase();
            if accept.starts_with("text/css") {
                Some(Self::Stylesheet)
            } else if accept.starts_with("image/") {
                Some(Self::Image)
            } else if accept.starts_with("text/html") {
                Some(Self::SubFrame)
            } else {
                None
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleAction {
    Block,
    Allow,
    /// Allows the frame and everything it loads.
    AllowAllRequests,
    /// Serves this file of the extension instead (a harmless stand-in).
    Redirect(String),
    UpgradeScheme,
}

impl RuleAction {
    /// Among rules of the same priority, the higher wins.
    fn precedence(&self) -> u8 {
        match self {
            Self::Allow | Self::AllowAllRequests => 3,
            Self::Block => 2,
            Self::UpgradeScheme => 1,
            Self::Redirect(_) => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainType {
    FirstParty,
    ThirdParty,
}

/// One rule, as declared by an extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkRule {
    pub priority: u32,
    pub action: RuleAction,
    pub url_filter: Option<UrlFilter>,
    pub request_domains: Vec<String>,
    pub excluded_request_domains: Vec<String>,
    pub initiator_domains: Vec<String>,
    pub excluded_initiator_domains: Vec<String>,
    /// `None`: every type but the main frame.
    pub resource_types: Option<Vec<ResourceType>>,
    pub excluded_resource_types: Vec<ResourceType>,
    pub domain_type: Option<DomainType>,
    /// Upper case methods; empty for any.
    pub request_methods: Vec<String>,
}

impl NetworkRule {
    pub fn new(action: RuleAction) -> Self {
        Self {
            priority: 1,
            action,
            url_filter: None,
            request_domains: Vec::new(),
            excluded_request_domains: Vec::new(),
            initiator_domains: Vec::new(),
            excluded_initiator_domains: Vec::new(),
            resource_types: None,
            excluded_resource_types: Vec::new(),
            domain_type: None,
            request_methods: Vec::new(),
        }
    }

    fn applies_to_type(&self, ty: Option<ResourceType>) -> bool {
        match ty {
            Some(ty) => {
                if self.excluded_resource_types.contains(&ty) {
                    return false;
                }
                match &self.resource_types {
                    Some(types) => types.contains(&ty),
                    None => ty != ResourceType::MainFrame,
                }
            }
            // Unknown: only rules for any sub-resource. A rule for some types
            // only (pings, scripts) would hit unrelated requests.
            None => self.resource_types.is_none(),
        }
    }

    fn matches(&self, request: &Request<'_>) -> bool {
        if !self.applies_to_type(request.resource_type) {
            return false;
        }
        if !self.request_methods.is_empty()
            && !self
                .request_methods
                .iter()
                .any(|m| m.eq_ignore_ascii_case(request.method))
        {
            return false;
        }
        if !self.request_domains.is_empty()
            && !self
                .request_domains
                .iter()
                .any(|d| is_subdomain(request.host, d))
        {
            return false;
        }
        if self
            .excluded_request_domains
            .iter()
            .any(|d| is_subdomain(request.host, d))
        {
            return false;
        }
        let initiator = request.initiator_host;
        if !self.initiator_domains.is_empty()
            && !initiator.is_some_and(|i| self.initiator_domains.iter().any(|d| is_subdomain(i, d)))
        {
            return false;
        }
        if let Some(i) = initiator
            && self
                .excluded_initiator_domains
                .iter()
                .any(|d| is_subdomain(i, d))
        {
            return false;
        }
        if let Some(kind) = self.domain_type {
            let third = match initiator {
                Some(i) => site(i) != site(request.host),
                None => false,
            };
            if third != (kind == DomainType::ThirdParty) {
                return false;
            }
        }
        match &self.url_filter {
            Some(filter) => filter.matches(request.url, &request.lower, request.host_range),
            None => true,
        }
    }
}

/// `host` is `domain` or one of its subdomains.
pub fn is_subdomain(host: &str, domain: &str) -> bool {
    host.len() >= domain.len()
        && host[host.len() - domain.len()..].eq_ignore_ascii_case(domain)
        && (host.len() == domain.len() || host.as_bytes()[host.len() - domain.len() - 1] == b'.')
}

/// The registrable part of a host, roughly (the last two labels, three for
/// `co.uk`-like suffixes): enough to tell first from third party.
fn site(host: &str) -> &str {
    let labels: Vec<&str> = host.rsplitn(4, '.').collect();
    let keep = match labels.as_slice() {
        [_, second, ..] if labels.len() >= 3 && second.len() <= 3 && labels[0].len() == 2 => 3,
        _ => 2,
    };
    if labels.len() <= keep {
        return host;
    }
    let tail: usize = labels[..keep].iter().map(|l| l.len()).sum::<usize>() + keep - 1;
    &host[host.len() - tail..]
}

/// A `urlFilter`: `||` anchors on a domain, `|` on the start or end, `*` is
/// anything, `^` a separator (not a letter, digit or `_-.%`, or the end).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlFilter {
    pattern: Vec<Token>,
    domain_anchor: bool,
    start_anchor: bool,
    end_anchor: bool,
    case_sensitive: bool,
    /// The longest literal: a cheap test before matching.
    needle: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Literal(String),
    Any,
    Separator,
}

fn is_separator(c: u8) -> bool {
    !(c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.' | b'%'))
}

impl UrlFilter {
    pub fn parse(filter: &str, case_sensitive: bool) -> Self {
        let mut rest = filter;
        let domain_anchor = rest.starts_with("||");
        if domain_anchor {
            rest = &rest[2..];
        }
        let start_anchor = !domain_anchor && rest.starts_with('|');
        if start_anchor {
            rest = &rest[1..];
        }
        let end_anchor = rest.ends_with('|');
        if end_anchor {
            rest = &rest[..rest.len() - 1];
        }
        let mut pattern = Vec::new();
        let mut literal = String::new();
        for c in rest.chars() {
            let token = match c {
                '*' => Token::Any,
                '^' => Token::Separator,
                _ => {
                    literal.push(if case_sensitive {
                        c
                    } else {
                        c.to_ascii_lowercase()
                    });
                    continue;
                }
            };
            if !literal.is_empty() {
                pattern.push(Token::Literal(std::mem::take(&mut literal)));
            }
            if !(token == Token::Any && pattern.last() == Some(&Token::Any)) {
                pattern.push(token);
            }
        }
        if !literal.is_empty() {
            pattern.push(Token::Literal(literal));
        }
        let needle = pattern
            .iter()
            .filter_map(|t| match t {
                Token::Literal(l) => Some(l.as_str()),
                _ => None,
            })
            .max_by_key(|l| l.len())
            .unwrap_or("")
            .to_owned();
        Self {
            pattern,
            domain_anchor,
            start_anchor,
            end_anchor,
            case_sensitive,
            needle,
        }
    }

    /// A word every matching address contains whole (between characters
    /// other than letters and digits), to index the rule by. `None` when
    /// the filter has none (`*`-only, or words that may run on).
    pub fn token(&self) -> Option<&str> {
        const TOO_COMMON: &[&str] = &["http", "https", "www", "com"];
        let last = self.pattern.len().saturating_sub(1);
        let mut best: Option<&str> = None;
        for (i, t) in self.pattern.iter().enumerate() {
            let Token::Literal(l) = t else { continue };
            let left_edge = if i == 0 {
                self.domain_anchor || self.start_anchor
            } else {
                self.pattern[i - 1] == Token::Separator
            };
            let right_edge = if i == last {
                self.end_anchor
            } else {
                self.pattern[i + 1] == Token::Separator
            };
            let bytes = l.as_bytes();
            let mut start = 0;
            while start < bytes.len() {
                if !bytes[start].is_ascii_alphanumeric() {
                    start += 1;
                    continue;
                }
                let mut end = start;
                while end < bytes.len() && bytes[end].is_ascii_alphanumeric() {
                    end += 1;
                }
                let bounded = (start > 0 || left_edge) && (end < bytes.len() || right_edge);
                let word = &l[start..end];
                if bounded
                    && word.len() >= 2
                    && !TOO_COMMON.contains(&word)
                    && best.is_none_or(|b| word.len() > b.len())
                {
                    best = Some(word);
                }
                start = end;
            }
        }
        best
    }

    /// The domain a `||domain^…` filter is anchored on.
    pub fn anchored_domain(&self) -> Option<&str> {
        if !self.domain_anchor {
            return None;
        }
        match self.pattern.as_slice() {
            [Token::Literal(d), Token::Separator, ..] | [Token::Literal(d)]
                if d.contains('.') && !d.contains('/') =>
            {
                Some(d.as_str())
            }
            _ => None,
        }
    }

    pub fn is_case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    /// The filter as written (canonical form), for converters.
    pub fn source(&self) -> String {
        let mut out = String::new();
        if self.domain_anchor {
            out.push_str("||");
        } else if self.start_anchor {
            out.push('|');
        }
        for t in &self.pattern {
            match t {
                Token::Literal(l) => out.push_str(l),
                Token::Any => out.push('*'),
                Token::Separator => out.push('^'),
            }
        }
        if self.end_anchor {
            out.push('|');
        }
        out
    }

    /// `lower` is `url` in lower case; `host` the byte range of the host.
    fn matches(&self, url: &str, lower: &str, host: (usize, usize)) -> bool {
        let url = if self.case_sensitive { url } else { lower };
        if !url.contains(&self.needle) {
            return false;
        }
        let bytes = url.as_bytes();
        if self.domain_anchor {
            // At the start of the host, or right after one of its dots.
            let (start, end) = host;
            (start..end)
                .filter(|&i| i == start || bytes[i - 1] == b'.')
                .any(|i| self.match_from(bytes, i, 0))
        } else if self.start_anchor {
            self.match_from(bytes, 0, 0)
        } else {
            (0..=bytes.len()).any(|i| self.match_from(bytes, i, 0))
        }
    }

    fn match_from(&self, url: &[u8], at: usize, token: usize) -> bool {
        let Some(t) = self.pattern.get(token) else {
            return !self.end_anchor || at == url.len();
        };
        match t {
            Token::Literal(l) => {
                url.get(at..at + l.len()) == Some(l.as_bytes())
                    && self.match_from(url, at + l.len(), token + 1)
            }
            Token::Separator => {
                if at == url.len() {
                    // `^` also matches the end, once.
                    self.match_from(url, at, token + 1)
                } else {
                    is_separator(url[at]) && self.match_from(url, at + 1, token + 1)
                }
            }
            Token::Any => (at..=url.len()).any(|i| self.match_from(url, i, token + 1)),
        }
    }
}

/// A request a page makes.
#[derive(Debug, Clone)]
pub struct Request<'a> {
    pub url: &'a str,
    lower: String,
    pub host: &'a str,
    /// Byte range of `host` in `url`.
    host_range: (usize, usize),
    /// The document that makes the request; `None` for a top-level load.
    pub initiator_host: Option<&'a str>,
    /// `None` when the web view does not tell.
    pub resource_type: Option<ResourceType>,
    pub method: &'a str,
}

impl<'a> Request<'a> {
    /// `None` when `url` has no `scheme://host`.
    pub fn new(
        url: &'a str,
        initiator_host: Option<&'a str>,
        resource_type: Option<ResourceType>,
        method: &'a str,
    ) -> Option<Self> {
        let start = url.find("://")? + 3;
        let rest = &url[start..];
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        let host_start = authority.rfind('@').map_or(0, |i| i + 1);
        let host = &authority[host_start..];
        let host = if host.starts_with('[') {
            host.split_inclusive(']').next().unwrap_or(host)
        } else {
            host.split(':').next().unwrap_or(host)
        };
        if host.is_empty() {
            return None;
        }
        let from = start + host_start;
        Some(Self {
            url,
            lower: url.to_ascii_lowercase(),
            host,
            host_range: (from, from + host.len()),
            initiator_host: initiator_host.filter(|h| !h.is_empty()),
            resource_type,
            method,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Block,
    /// Serve `path` of `extension` instead.
    Redirect {
        extension: ExtensionId,
        path: String,
    },
}

#[derive(Debug, Clone)]
struct Entry {
    rule: NetworkRule,
    extension: usize,
}

/// The rules of every enabled extension, indexed for quick lookups.
#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    extensions: Vec<ExtensionId>,
    rules: Vec<Entry>,
    /// Rules that only apply to some request domains, by domain.
    by_domain: HashMap<String, Vec<usize>>,
    /// The others, by a word of their filter.
    by_token: HashMap<String, Vec<usize>>,
    /// Rules with neither: tried on every request.
    generic: Vec<usize>,
    /// `allowAllRequests` rules, checked against the document.
    allow_all: Vec<usize>,
}

impl RuleSet {
    pub fn new(per_extension: Vec<(ExtensionId, Vec<NetworkRule>)>) -> Self {
        let mut set = Self::default();
        for (id, rules) in per_extension {
            let ext = set.extensions.len();
            set.extensions.push(id);
            for rule in rules {
                let index = set.rules.len();
                if rule.action == RuleAction::AllowAllRequests {
                    set.allow_all.push(index);
                }
                let domains: Vec<String> = if !rule.request_domains.is_empty() {
                    rule.request_domains
                        .iter()
                        .map(|d| d.to_ascii_lowercase())
                        .collect()
                } else if let Some(d) = rule
                    .url_filter
                    .as_ref()
                    .and_then(UrlFilter::anchored_domain)
                {
                    vec![d.to_owned()]
                } else {
                    Vec::new()
                };
                if domains.is_empty() {
                    match rule.url_filter.as_ref().and_then(UrlFilter::token) {
                        Some(token) => set
                            .by_token
                            .entry(token.to_ascii_lowercase())
                            .or_default()
                            .push(index),
                        None => set.generic.push(index),
                    }
                } else {
                    for d in domains {
                        set.by_domain.entry(d).or_default().push(index);
                    }
                }
                set.rules.push(Entry {
                    rule,
                    extension: ext,
                });
            }
        }
        set
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    fn candidates(&self, request: &Request<'_>) -> impl Iterator<Item = usize> + '_ {
        let host = request.host.to_ascii_lowercase();
        let mut found = Vec::new();
        let mut words: Vec<&str> = request
            .lower
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| w.len() >= 2)
            .collect();
        words.sort_unstable();
        words.dedup();
        for w in words {
            if let Some(list) = self.by_token.get(w) {
                found.extend_from_slice(list);
            }
        }
        let mut rest = host.as_str();
        loop {
            if let Some(list) = self.by_domain.get(rest) {
                found.extend_from_slice(list);
            }
            match rest.find('.') {
                Some(i) => rest = &rest[i + 1..],
                None => break,
            }
        }
        found.into_iter().chain(self.generic.iter().copied())
    }

    /// The winning rule for `request`: highest priority, then allow before
    /// block before redirect.
    fn best(&self, request: &Request<'_>) -> Option<&Entry> {
        self.candidates(request)
            .map(|i| &self.rules[i])
            .filter(|e| e.rule.matches(request))
            .max_by_key(|e| (e.rule.priority, e.rule.action.precedence()))
    }

    /// What to do with `request`. `document` is the address of the page that
    /// makes it, for `allowAllRequests` rules.
    pub fn decide(&self, request: &Request<'_>, document: Option<&str>) -> Verdict {
        let Some(best) = self.best(request) else {
            return Verdict::Allow;
        };
        // A page allowed as a whole lets everything it loads through.
        if let Some(doc) =
            document.and_then(|d| Request::new(d, None, Some(ResourceType::MainFrame), "get"))
            && self.allow_all.iter().any(|&i| {
                let e = &self.rules[i];
                e.rule.priority >= best.rule.priority && e.rule.matches(&doc)
            })
        {
            return Verdict::Allow;
        }
        match &best.rule.action {
            RuleAction::Block => Verdict::Block,
            RuleAction::Redirect(path) => Verdict::Redirect {
                extension: self.extensions[best.extension].clone(),
                path: path.clone(),
            },
            RuleAction::Allow | RuleAction::AllowAllRequests | RuleAction::UpgradeScheme => {
                Verdict::Allow
            }
        }
    }

    /// The rule that decides `request`, if any (to explain a decision).
    pub fn deciding_rule(&self, request: &Request<'_>) -> Option<&NetworkRule> {
        self.best(request).map(|e| &e.rule)
    }

    /// Every rule with the extension it comes from (for converters).
    pub fn rules(&self) -> impl Iterator<Item = (&ExtensionId, &NetworkRule)> {
        self.rules
            .iter()
            .map(|e| (&self.extensions[e.extension], &e.rule))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(f: &str) -> UrlFilter {
        UrlFilter::parse(f, false)
    }

    fn matches(f: &str, url: &str) -> bool {
        let r = Request::new(url, None, None, "get").unwrap();
        filter(f).matches(r.url, &r.lower, r.host_range)
    }

    #[test]
    fn url_filters() {
        assert!(matches("||ads.com^", "https://ads.com/x.js"));
        assert!(matches("||ads.com^", "https://cdn.ads.com/x.js"));
        assert!(!matches("||ads.com^", "https://badads.com/x.js"));
        assert!(!matches("||ads.com^", "https://ads.com.evil.org/"));
        assert!(matches("||ads.com^", "https://ads.com"));
        assert!(matches("/banner/*/img^", "https://x.org/banner/a/b/img?x"));
        assert!(!matches("/banner/*/img^", "https://x.org/banner/a/b/imgs"));
        assert!(matches("|https://x.org/a", "https://x.org/abc"));
        assert!(!matches(
            "|https://x.org/a",
            "http://y.org/?https://x.org/a"
        ));
        assert!(matches(".gif|", "https://x.org/a.GIF"));
        assert!(!matches(".gif|", "https://x.org/a.gif?x"));
        assert!(!UrlFilter::parse("/Ads/", true).matches(
            "https://x.org/ads/",
            "https://x.org/ads/",
            (8, 13)
        ));
        assert_eq!(filter("/banner/*/img^").token(), Some("banner"));
        assert_eq!(
            filter("||ads.example.com/x/promo-").token(),
            Some("example")
        );
        assert_eq!(filter("ads*").token(), None);
        assert_eq!(filter("|https://x").token(), None);
        assert_eq!(filter("||ads.com^").anchored_domain(), Some("ads.com"));
        assert_eq!(filter("||ads.com/x").anchored_domain(), None);
        assert_eq!(filter("||a.b^*x|").source(), "||a.b^*x|");
    }

    #[test]
    fn hosts_and_sites() {
        let r = Request::new("https://user@Sub.Example.co.uk:8080/p?q", None, None, "get").unwrap();
        assert_eq!(r.host, "Sub.Example.co.uk");
        assert!(is_subdomain("a.b.example.com", "example.com"));
        assert!(!is_subdomain("badexample.com", "example.com"));
        assert_eq!(site("a.b.example.com"), "example.com");
        assert_eq!(site("www.bbc.co.uk"), "bbc.co.uk");
        assert_eq!(site("localhost"), "localhost");
    }

    fn rule(action: RuleAction, f: &str) -> NetworkRule {
        NetworkRule {
            url_filter: Some(filter(f)),
            ..NetworkRule::new(action)
        }
    }

    fn id() -> ExtensionId {
        ExtensionId::parse("ddkjiahejlhfcafbddmgiahcphecmpfh").unwrap()
    }

    fn decide(set: &RuleSet, url: &str, from: Option<&str>, ty: Option<ResourceType>) -> Verdict {
        let initiator = from
            .and_then(|d| Request::new(d, None, None, "get"))
            .map(|r| r.host);
        set.decide(&Request::new(url, initiator, ty, "get").unwrap(), from)
    }

    #[test]
    fn priorities_and_actions() {
        let mut third = NetworkRule {
            domain_type: Some(DomainType::ThirdParty),
            request_domains: vec!["tracker.net".into()],
            ..NetworkRule::new(RuleAction::Block)
        };
        third.priority = 1;
        let set = RuleSet::new(vec![(
            id(),
            vec![
                rule(RuleAction::Block, "||ads.com^"),
                NetworkRule {
                    priority: 2,
                    ..rule(RuleAction::Allow, "||ads.com/ok.js")
                },
                NetworkRule {
                    resource_types: Some(vec![ResourceType::Script]),
                    ..rule(
                        RuleAction::Redirect("/war/noop.js".into()),
                        "/adsbygoogle.js",
                    )
                },
                third,
                NetworkRule {
                    priority: 5,
                    resource_types: Some(vec![ResourceType::MainFrame]),
                    request_domains: vec!["trusted.org".into()],
                    ..NetworkRule::new(RuleAction::AllowAllRequests)
                },
            ],
        )]);
        let page = Some("https://news.org/");
        let script = Some(ResourceType::Script);
        assert_eq!(
            decide(&set, "https://cdn.ads.com/x.js", page, script),
            Verdict::Block
        );
        assert_eq!(
            decide(&set, "https://ads.com/ok.js", page, script),
            Verdict::Allow
        );
        assert_eq!(
            decide(&set, "https://g.com/adsbygoogle.js", page, script),
            Verdict::Redirect {
                extension: id(),
                path: "/war/noop.js".into()
            }
        );
        assert_eq!(
            decide(
                &set,
                "https://g.com/adsbygoogle.js",
                page,
                Some(ResourceType::Image)
            ),
            Verdict::Allow
        );
        // Unknown type: only rules for any type apply.
        assert_eq!(
            decide(&set, "https://g.com/adsbygoogle.js", page, None),
            Verdict::Allow
        );
        assert_eq!(
            decide(&set, "https://cdn.ads.com/x", page, None),
            Verdict::Block
        );
        // Third party only.
        assert_eq!(
            decide(&set, "https://px.tracker.net/p", page, None),
            Verdict::Block
        );
        assert_eq!(
            decide(
                &set,
                "https://px.tracker.net/p",
                Some("https://www.tracker.net/"),
                None
            ),
            Verdict::Allow
        );
        // Main frames are left alone unless a rule names them.
        assert_eq!(
            decide(
                &set,
                "https://ads.com/",
                None,
                Some(ResourceType::MainFrame)
            ),
            Verdict::Allow
        );
        // An allowed page loads everything.
        assert_eq!(
            decide(
                &set,
                "https://ads.com/x.js",
                Some("https://trusted.org/a"),
                script
            ),
            Verdict::Allow
        );
    }

    #[test]
    fn initiator_conditions() {
        let set = RuleSet::new(vec![(
            id(),
            vec![NetworkRule {
                initiator_domains: vec!["news.org".into()],
                excluded_initiator_domains: vec!["ok.news.org".into()],
                ..rule(RuleAction::Block, "/promo/")
            }],
        )]);
        assert_eq!(
            decide(
                &set,
                "https://x.com/promo/1",
                Some("https://a.news.org/"),
                None
            ),
            Verdict::Block
        );
        assert_eq!(
            decide(
                &set,
                "https://x.com/promo/1",
                Some("https://ok.news.org/"),
                None
            ),
            Verdict::Allow
        );
        assert_eq!(
            decide(
                &set,
                "https://x.com/promo/1",
                Some("https://other.org/"),
                None
            ),
            Verdict::Allow
        );
    }

    #[test]
    fn guesses_types() {
        assert_eq!(
            ResourceType::guess("https://a/x.js?v=1", "*/*"),
            Some(ResourceType::Script)
        );
        assert_eq!(
            ResourceType::guess("https://a/p", "image/webp,*/*"),
            Some(ResourceType::Image)
        );
        assert_eq!(ResourceType::guess("https://a/p", "*/*"), None);
    }
}
