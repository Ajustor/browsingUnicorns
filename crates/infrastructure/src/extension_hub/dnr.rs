//! `declarativeNetRequest`: the rulesets enabled, the dynamic and session
//! rules.

use domain::ExtensionId;
use serde_json::{Map, Value, json};

use super::{Answer, Call, Hub, HubHost, unsupported};
use crate::extensions::state::ExtensionState;

impl Hub {
    pub(super) fn dnr_call(&mut self, call: &Call<'_>, host: &mut dyn HubHost) -> Answer {
        match call.op {
            "dnr.getEnabledRulesets"
            | "dnr.updateEnabledRulesets"
            | "dnr.getRules"
            | "dnr.updateRules" => {
                let ext = call.ext?;
                Some(self.dnr(ext, call.op, call.raw, host))
            }
            op => unsupported(op),
        }
    }

    fn dnr(
        &mut self,
        ext: &ExtensionId,
        op: &str,
        raw: &Map<String, Value>,
        host: &mut dyn HubHost,
    ) -> Result<Value, String> {
        let parsed = self.catalog.get(ext);
        let mut state = ExtensionState::load(&self.state_dir, ext);
        let enabled = state.enabled_rulesets.clone().unwrap_or_else(|| {
            parsed
                .as_ref()
                .map(|p| p.default_rulesets())
                .unwrap_or_default()
        });
        let session = raw.get("kind").and_then(Value::as_str) == Some("session");
        match op {
            "dnr.getEnabledRulesets" => return Ok(json!(enabled)),
            "dnr.getRules" => {
                let list = if session {
                    &state.session_rules
                } else {
                    &state.dynamic_rules
                };
                let ids: Option<Vec<u64>> = raw
                    .get("ids")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_u64).collect());
                return Ok(Value::Array(
                    list.iter()
                        .filter(|r| {
                            ids.as_ref().is_none_or(|ids| {
                                r.get("id")
                                    .and_then(Value::as_u64)
                                    .is_some_and(|i| ids.contains(&i))
                            })
                        })
                        .cloned()
                        .collect(),
                ));
            }
            "dnr.updateEnabledRulesets" => {
                let options = raw.get("options").cloned().unwrap_or(Value::Null);
                let list = |k: &str| -> Vec<String> {
                    options
                        .get(k)
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let all = parsed
                    .as_ref()
                    .map(|p| p.all_rulesets())
                    .unwrap_or_default();
                let mut now = enabled;
                now.retain(|r| !list("disableRulesetIds").contains(r));
                for r in list("enableRulesetIds") {
                    if !all.contains(&r) {
                        return Err(format!("Invalid ruleset id: {r}."));
                    }
                    if !now.contains(&r) {
                        now.push(r);
                    }
                }
                state.enabled_rulesets = Some(now);
            }
            _ => {
                let options = raw.get("options").cloned().unwrap_or(Value::Null);
                let remove: Vec<u64> = options
                    .get("removeRuleIds")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_u64).collect())
                    .unwrap_or_default();
                let add = options
                    .get("addRules")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let list = if session {
                    &mut state.session_rules
                } else {
                    &mut state.dynamic_rules
                };
                list.retain(|r| {
                    !r.get("id")
                        .and_then(Value::as_u64)
                        .is_some_and(|i| remove.contains(&i))
                });
                for r in add {
                    let rid = r
                        .get("id")
                        .and_then(Value::as_u64)
                        .ok_or("a rule needs an id")?;
                    if list
                        .iter()
                        .any(|e| e.get("id").and_then(Value::as_u64) == Some(rid))
                    {
                        return Err(format!("Rule with id {rid} does not have a unique ID."));
                    }
                    list.push(r);
                }
            }
        }
        state.save(&self.state_dir, ext)?;
        host.extension_changed(ext);
        Ok(Value::Null)
    }
}
