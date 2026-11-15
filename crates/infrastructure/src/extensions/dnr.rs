//! An unpacked extension's `declarativeNetRequest` rulesets, read into the
//! domain's [`NetworkRule`]s.

use std::fs;
use std::path::Path;

use domain::{DomainType, NetworkRule, ResourceType, RuleAction, UrlFilter};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Ruleset {
    #[serde(default)]
    enabled: bool,
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleDto {
    #[serde(default = "one")]
    priority: u32,
    action: ActionDto,
    #[serde(default)]
    condition: ConditionDto,
}

fn one() -> u32 {
    1
}

#[derive(Deserialize)]
struct ActionDto {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    redirect: Option<RedirectDto>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RedirectDto {
    #[serde(default)]
    extension_path: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ConditionDto {
    url_filter: Option<String>,
    regex_filter: Option<String>,
    is_url_filter_case_sensitive: bool,
    request_domains: Vec<String>,
    excluded_request_domains: Vec<String>,
    initiator_domains: Vec<String>,
    excluded_initiator_domains: Vec<String>,
    resource_types: Option<Vec<String>>,
    excluded_resource_types: Vec<String>,
    domain_type: Option<String>,
    request_methods: Vec<String>,
    response_headers: Option<Value>,
    excluded_response_headers: Option<Value>,
}

fn types(names: &[String]) -> Vec<ResourceType> {
    names
        .iter()
        .filter_map(|n| ResourceType::parse(n))
        .collect()
}

/// `None` for what the browser cannot apply (regular expressions, header
/// changes, response header conditions).
fn convert(dto: RuleDto) -> Option<NetworkRule> {
    let c = dto.condition;
    if c.regex_filter.is_some()
        || c.response_headers.is_some()
        || c.excluded_response_headers.is_some()
    {
        return None;
    }
    let action = match dto.action.kind.as_str() {
        "block" => RuleAction::Block,
        "allow" => RuleAction::Allow,
        "allowAllRequests" => RuleAction::AllowAllRequests,
        "upgradeScheme" => RuleAction::UpgradeScheme,
        // Only redirects to a file of the extension: the stand-ins.
        "redirect" => match dto.action.redirect.and_then(|r| r.extension_path) {
            Some(path) => RuleAction::Redirect(path),
            None => RuleAction::Block,
        },
        _ => return None,
    };
    Some(NetworkRule {
        priority: dto.priority,
        action,
        url_filter: c
            .url_filter
            .map(|f| UrlFilter::parse(&f, c.is_url_filter_case_sensitive)),
        request_domains: c.request_domains,
        excluded_request_domains: c.excluded_request_domains,
        initiator_domains: c.initiator_domains,
        excluded_initiator_domains: c.excluded_initiator_domains,
        resource_types: c.resource_types.as_deref().map(types),
        excluded_resource_types: types(&c.excluded_resource_types),
        domain_type: match c.domain_type.as_deref() {
            Some("firstParty") => Some(DomainType::FirstParty),
            Some("thirdParty") => Some(DomainType::ThirdParty),
            _ => None,
        },
        request_methods: c
            .request_methods
            .iter()
            .map(|m| m.to_ascii_uppercase())
            .collect(),
    })
}

/// The rules of the extension unpacked in `dir`: its rulesets enabled by
/// default. Also returns how many rules were left out.
pub fn read_rules(dir: &Path) -> Result<(Vec<NetworkRule>, usize), String> {
    let text =
        fs::read_to_string(dir.join("manifest.json")).map_err(|e| format!("manifest.json: {e}"))?;
    let manifest: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("manifest.json: {e}"))?;
    let Some(resources) = manifest
        .pointer("/declarative_net_request/rule_resources")
        .cloned()
    else {
        return Ok((Vec::new(), 0));
    };
    let rulesets: Vec<Ruleset> = serde_json::from_value(resources).map_err(|e| e.to_string())?;
    let mut rules = Vec::new();
    let mut skipped = 0;
    for set in rulesets.into_iter().filter(|r| r.enabled) {
        let path = dir.join(set.path.trim_start_matches('/'));
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", set.path))?;
        let dtos: Vec<Value> =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", set.path))?;
        for dto in dtos {
            match serde_json::from_value(dto).ok().and_then(convert) {
                Some(rule) => rules.push(rule),
                None => skipped += 1,
            }
        }
    }
    Ok((rules, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_enabled_rulesets() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("manifest.json"),
            r#"{"name":"x","declarative_net_request":{"rule_resources":[
                {"id":"a","enabled":true,"path":"/rules/a.json"},
                {"id":"b","enabled":false,"path":"/rules/b.json"}]}}"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("rules")).unwrap();
        fs::write(
            dir.path().join("rules/a.json"),
            r#"[
              {"id":1,"priority":2,"action":{"type":"block"},
               "condition":{"urlFilter":"||ads.com^","resourceTypes":["script","nope"],
                            "domainType":"thirdParty","requestMethods":["get"]}},
              {"id":2,"action":{"type":"redirect","redirect":{"extensionPath":"/war/noop.js"}},
               "condition":{"requestDomains":["g.com"]}},
              {"id":3,"action":{"type":"block"},"condition":{"regexFilter":"^https?://"}},
              {"id":4,"action":{"type":"modifyHeaders"},"condition":{}}
            ]"#,
        )
        .unwrap();
        let (rules, skipped) = read_rules(dir.path()).unwrap();
        assert_eq!(skipped, 2);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].priority, 2);
        assert_eq!(rules[0].resource_types, Some(vec![ResourceType::Script]));
        assert_eq!(rules[0].domain_type, Some(DomainType::ThirdParty));
        assert_eq!(rules[0].request_methods, vec!["GET".to_owned()]);
        assert_eq!(rules[1].action, RuleAction::Redirect("/war/noop.js".into()));
    }

    #[test]
    fn no_rules_without_dnr() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("manifest.json"), r#"{"name":"x"}"#).unwrap();
        assert_eq!(read_rules(dir.path()).unwrap(), (Vec::new(), 0));
    }
}
