use std::sync::OnceLock;

use anyhow::ensure;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use crate::Result;
use crate::governance::*;

#[derive(Deserialize)]
struct Rules {
    rules: Vec<Rule>,
}

#[derive(Deserialize)]
struct Rule {
    id: String,
    regex: String,
    secret_group: usize,
}

fn rules() -> &'static Vec<(Rule, Regex)> {
    static RULES: OnceLock<Vec<(Rule, Regex)>> = OnceLock::new();
    RULES.get_or_init(|| {
        let rules: Rules = toml::from_str(include_str!("../rules/secrets.toml")).unwrap();
        rules
            .rules
            .into_iter()
            .map(|rule| {
                let regex = Regex::new(&rule.regex).unwrap();
                (rule, regex)
            })
            .collect()
    })
}

pub fn validate_policy(policy: &GatePolicy) -> Result<()> {
    for id in &policy.rule_allowlist {
        ensure!(
            rules().iter().any(|(rule, _)| rule.id == *id),
            "Unknown secret rule in allowlist"
        );
    }
    Ok(())
}

pub fn scan(text: &str, field_path: &str, policy: &GatePolicy) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (rule, regex) in rules() {
        for captures in regex.captures_iter(text) {
            let matched = captures.get(rule.secret_group).unwrap();
            let disposition = if policy.rule_allowlist.contains(&rule.id) {
                FindingDisposition::Allowlisted
            } else if policy.secrets == GateAction::Block {
                FindingDisposition::Blocked
            } else {
                FindingDisposition::Passed
            };
            findings.push(Finding {
                rule_id: rule.id.clone(),
                tier: FindingTier::Secret,
                field_path: field_path.into(),
                key_hash: None,
                byte_span: ByteSpan {
                    start: matched.start() as u64,
                    end: matched.end() as u64,
                },
                disposition,
            });
        }
    }
    findings
}

pub fn scan_value(value: &Value, path: &str, policy: &GatePolicy) -> Vec<Finding> {
    match value {
        Value::String(text) => scan(text, path, policy),
        Value::Array(array) => array
            .iter()
            .enumerate()
            .flat_map(|(index, value)| scan_value(value, &format!("{path}/{index}"), policy))
            .collect(),
        Value::Object(object) => object
            .iter()
            .flat_map(|(key, value)| {
                let mut findings = scan(key, &mask(path), policy);
                for finding in &mut findings {
                    finding.key_hash = Some(crate::engine::content_hash(key.as_bytes()));
                }
                let path = format!("{path}/{}", mask(key).replace('~', "~0").replace('/', "~1"));
                findings.extend(scan_value(value, &path, policy));
                if key.eq_ignore_ascii_case("password")
                    && let Some(text) = value.as_str()
                {
                    findings.extend(
                        scan(&format!("password={text}"), &path, policy)
                            .into_iter()
                            .filter(|finding| finding.rule_id == "password-assignment")
                            .map(|mut finding| {
                                finding.byte_span.start -= 9;
                                finding.byte_span.end -= 9;
                                finding
                            }),
                    );
                }
                findings
            })
            .collect(),
        _ => vec![],
    }
}

pub fn scan_keys(value: &Value, path: &str, policy: &GatePolicy) -> Vec<Finding> {
    match value {
        Value::Object(object) => object
            .iter()
            .flat_map(|(key, value)| {
                let mut findings = scan(key, &mask(path), policy);
                for finding in &mut findings {
                    finding.key_hash = Some(crate::engine::content_hash(key.as_bytes()));
                }
                let child = format!("{path}/{}", mask(key).replace('~', "~0").replace('/', "~1"));
                findings.extend(scan_keys(value, &child, policy));
                findings
            })
            .collect(),
        Value::Array(array) => array
            .iter()
            .enumerate()
            .flat_map(|(index, value)| scan_keys(value, &format!("{path}/{index}"), policy))
            .collect(),
        _ => vec![],
    }
}

pub fn mask(text: &str) -> String {
    let mut spans: Vec<_> = rules()
        .iter()
        .flat_map(|(rule, regex)| {
            regex
                .captures_iter(text)
                .map(|captures| {
                    let matched = captures.get(rule.secret_group).unwrap();
                    (matched.start(), matched.end())
                })
                .collect::<Vec<_>>()
        })
        .collect();
    spans.sort_unstable();
    let mut output = String::new();
    let mut end = 0;
    for (start, stop) in spans {
        if start >= end {
            output.push_str(&text[end..start]);
            output.push_str("[secret]");
        }
        end = end.max(stop);
    }
    output.push_str(&text[end..]);
    output
}

pub fn mask_value(value: &mut Value) {
    match value {
        Value::String(text) => *text = mask(text),
        Value::Array(array) => array.iter_mut().for_each(mask_value),
        Value::Object(object) => object.values_mut().for_each(mask_value),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masking_overlapping_findings_does_not_leave_suffixes() {
        let text = format!("password=ghp_TEST{}", "A".repeat(32));
        let masked = mask(&text);
        assert_eq!(masked, "password=[secret]");
    }
}
