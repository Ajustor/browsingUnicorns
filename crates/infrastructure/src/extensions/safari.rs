//! Network rules as a WebKit content blocker ("content rule list", the
//! Safari format), which WKWebView (macOS) and WebKitGTK (Linux) compile and
//! apply themselves.
//!
//! The format has no redirect, no request-domain condition and a reduced
//! regex syntax (no `|`, no `{n}`), so the conversion is approximate:
//! request domains become URL patterns, redirects are left out (blocking the
//! stand-in scripts breaks pages), rules WebKit cannot express are skipped.

use domain::{DomainType, NetworkRule, ResourceType, RuleAction, RuleSet};
use serde_json::{Value, json};

/// WebKit refuses bigger lists.
pub const MAX_RULES: usize = 150_000;

/// Separator (`^` in a filter): not a letter, digit or `_-.%`.
const SEPARATOR: &str = "[^a-zA-Z0-9_.%-]";
/// `scheme://` and any subdomain, before an anchored domain.
const DOMAIN_PREFIX: &str = "^[^:]+://([^/]*\\.)?";

fn escape(literal: &str) -> String {
    let mut out = String::with_capacity(literal.len());
    for c in literal.chars() {
        if "\\.+?()[]{}|$^*/".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The regex of a filter's body (after its anchors).
fn body_regex(body: &str) -> String {
    let mut out = String::new();
    let mut literal = String::new();
    let chars: Vec<char> = body.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        match c {
            '*' | '^' => {
                out.push_str(&escape(&std::mem::take(&mut literal)));
                if *c == '*' {
                    out.push_str(".*");
                } else if i + 1 == chars.len() {
                    // Also the end of the address.
                    out.push_str(&format!("({SEPARATOR}.*)?$"));
                } else {
                    out.push_str(SEPARATOR);
                }
            }
            c => literal.push(*c),
        }
    }
    out.push_str(&escape(&literal));
    out
}

/// The `url-filter` of a `urlFilter`.
fn filter_regex(source: &str) -> String {
    let (prefix, rest) = if let Some(rest) = source.strip_prefix("||") {
        (DOMAIN_PREFIX, rest)
    } else if let Some(rest) = source.strip_prefix('|') {
        ("^", rest)
    } else {
        ("", source)
    };
    let (rest, end) = match rest.strip_suffix('|') {
        Some(r) => (r, "$"),
        None => (rest, ""),
    };
    let body = body_regex(rest);
    if prefix.is_empty() && body.is_empty() {
        return ".*".into();
    }
    format!("{prefix}{body}{end}")
}

fn domain_regex(domain: &str) -> String {
    format!(
        "{DOMAIN_PREFIX}{}[/:?#]",
        escape(&domain.to_ascii_lowercase())
    )
}

fn webkit_types(rule: &NetworkRule) -> Option<Vec<&'static str>> {
    use ResourceType::*;
    let all = [
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
    ];
    let types: Vec<ResourceType> = match &rule.resource_types {
        Some(t) => t.clone(),
        None => all.into_iter().filter(|t| *t != MainFrame).collect(),
    };
    let mut names: Vec<&'static str> = types
        .into_iter()
        .filter(|t| !rule.excluded_resource_types.contains(t))
        .map(|t| match t {
            MainFrame | SubFrame => "document",
            Stylesheet => "style-sheet",
            Script => "script",
            Image => "image",
            Font => "font",
            Media => "media",
            _ => "raw",
        })
        .collect();
    names.sort_unstable();
    names.dedup();
    (!names.is_empty()).then_some(names)
}

fn domains(list: &[String]) -> Value {
    list.iter()
        .map(|d| Value::String(format!("*{}", d.to_ascii_lowercase())))
        .collect()
}

/// The WebKit rules of one network rule (one per request domain), or none
/// when WebKit cannot express it.
fn convert(rule: &NetworkRule) -> Vec<Value> {
    let action = match &rule.action {
        RuleAction::Block => json!({"type": "block"}),
        RuleAction::Allow | RuleAction::AllowAllRequests => {
            json!({"type": "ignore-previous-rules"})
        }
        RuleAction::UpgradeScheme => json!({"type": "make-https"}),
        RuleAction::Redirect(_) => return Vec::new(),
    };
    // No equivalents: request domains to leave out, include and exclude
    // lists of pages together.
    if !rule.excluded_request_domains.is_empty()
        || (!rule.initiator_domains.is_empty() && !rule.excluded_initiator_domains.is_empty())
    {
        return Vec::new();
    }
    let source = rule.url_filter.as_ref().map(|f| f.source());
    if source.as_deref().is_some_and(|s| !s.is_ascii()) {
        return Vec::new();
    }
    let mut trigger = serde_json::Map::new();
    let all_requests = rule.action == RuleAction::AllowAllRequests;
    if all_requests {
        // The page's own domains: everything it loads is let through.
        let pages: Vec<String> = if !rule.request_domains.is_empty() {
            rule.request_domains.clone()
        } else if let Some(d) = rule.url_filter.as_ref().and_then(|f| f.anchored_domain()) {
            vec![d.to_owned()]
        } else {
            return Vec::new();
        };
        trigger.insert("url-filter".into(), ".*".into());
        trigger.insert("if-domain".into(), domains(&pages));
        return vec![json!({"trigger": trigger, "action": action})];
    }
    let Some(types) = webkit_types(rule) else {
        return Vec::new();
    };
    trigger.insert("resource-type".into(), json!(types));
    if rule
        .url_filter
        .as_ref()
        .is_some_and(|f| f.is_case_sensitive())
    {
        trigger.insert("url-filter-is-case-sensitive".into(), true.into());
    }
    if !rule.initiator_domains.is_empty() {
        trigger.insert("if-domain".into(), domains(&rule.initiator_domains));
    } else if !rule.excluded_initiator_domains.is_empty() {
        trigger.insert(
            "unless-domain".into(),
            domains(&rule.excluded_initiator_domains),
        );
    }
    match rule.domain_type {
        Some(DomainType::ThirdParty) => {
            trigger.insert("load-type".into(), json!(["third-party"]));
        }
        Some(DomainType::FirstParty) => {
            trigger.insert("load-type".into(), json!(["first-party"]));
        }
        None => {}
    }
    let filters: Vec<String> = if rule.request_domains.is_empty() {
        vec![source.as_deref().map_or_else(|| ".*".into(), filter_regex)]
    } else {
        // `scheme://(sub.)domain/` then, when there is one, the filter.
        let tail = match source.as_deref() {
            None => String::new(),
            Some(s) if s.starts_with('|') => return Vec::new(),
            Some(s) => {
                let r = filter_regex(s);
                format!(".*{}", r.trim_start_matches(".*"))
            }
        };
        rule.request_domains
            .iter()
            .filter(|d| d.is_ascii())
            .map(|d| format!("{}{tail}", domain_regex(d)))
            .collect()
    };
    filters
        .into_iter()
        .map(|f| {
            let mut t = trigger.clone();
            t.insert("url-filter".into(), f.into());
            json!({"trigger": t, "action": action})
        })
        .collect()
}

/// The content rule list of `rules`, and how many rules it has. Rules run in
/// order, so they are sorted by priority, an allow after a block of the same
/// priority (it cancels it).
pub fn content_rule_list(rules: &RuleSet) -> (String, usize) {
    let mut sorted: Vec<&NetworkRule> = rules.rules().map(|(_, r)| r).collect();
    sorted.sort_by_key(|r| {
        let allow = matches!(r.action, RuleAction::Allow | RuleAction::AllowAllRequests);
        (r.priority, allow)
    });
    let mut out: Vec<Value> = sorted.into_iter().flat_map(convert).collect();
    if out.len() > MAX_RULES {
        log::warn!(
            "extensions: {} WebKit rules, only the first {MAX_RULES} are kept",
            out.len()
        );
        out.truncate(MAX_RULES);
    }
    let count = out.len();
    (Value::Array(out).to_string(), count)
}

#[cfg(test)]
mod tests {
    use domain::{ExtensionId, UrlFilter};

    use super::*;

    fn rule(action: RuleAction, filter: &str) -> NetworkRule {
        NetworkRule {
            url_filter: Some(UrlFilter::parse(filter, false)),
            ..NetworkRule::new(action)
        }
    }

    #[test]
    fn filters_become_webkit_regexes() {
        assert_eq!(
            filter_regex("||ads.com^"),
            "^[^:]+://([^/]*\\.)?ads\\.com([^a-zA-Z0-9_.%-].*)?$"
        );
        assert_eq!(
            filter_regex("/banner/*/img^x"),
            "\\/banner\\/.*\\/img[^a-zA-Z0-9_.%-]x"
        );
        assert_eq!(filter_regex("|https://a.b/"), "^https:\\/\\/a\\.b\\/");
        assert_eq!(filter_regex(".gif|"), "\\.gif$");
        assert_eq!(filter_regex("*"), ".*");
    }

    #[test]
    fn rule_list() {
        let id = ExtensionId::parse("ddkjiahejlhfcafbddmgiahcphecmpfh").unwrap();
        let set = RuleSet::new(vec![(
            id,
            vec![
                NetworkRule {
                    priority: 2,
                    ..rule(RuleAction::Allow, "||ads.com/ok.js")
                },
                NetworkRule {
                    domain_type: Some(DomainType::ThirdParty),
                    request_domains: vec!["a.com".into(), "b.net".into()],
                    resource_types: Some(vec![ResourceType::Script, ResourceType::XmlHttpRequest]),
                    ..NetworkRule::new(RuleAction::Block)
                },
                NetworkRule {
                    initiator_domains: vec!["news.org".into()],
                    ..rule(RuleAction::Block, "/promo/")
                },
                rule(RuleAction::Redirect("/noop.js".into()), "/ads.js"),
                NetworkRule {
                    excluded_request_domains: vec!["c.com".into()],
                    ..rule(RuleAction::Block, "/x/")
                },
            ],
        )]);
        let (json, count) = content_rule_list(&set);
        let list: Vec<Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(count, 4, "{json}");
        assert_eq!(list.len(), 4);
        // Blocks of priority 1 first, then the priority 2 allow.
        assert_eq!(
            list[0]["trigger"]["url-filter"],
            "^[^:]+://([^/]*\\.)?a\\.com[/:?#]"
        );
        assert_eq!(list[0]["trigger"]["load-type"], json!(["third-party"]));
        assert_eq!(
            list[0]["trigger"]["resource-type"],
            json!(["raw", "script"])
        );
        assert_eq!(
            list[1]["trigger"]["url-filter"],
            "^[^:]+://([^/]*\\.)?b\\.net[/:?#]"
        );
        assert_eq!(list[2]["trigger"]["if-domain"], json!(["*news.org"]));
        assert_eq!(list[3]["action"]["type"], "ignore-previous-rules");
    }
}
