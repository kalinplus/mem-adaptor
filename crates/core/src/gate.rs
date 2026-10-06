//! Detects pinned local secret signatures and assigns the configured disposition before target planning.
//! The same capture spans mask reports; pass policy still allows raw target payloads.
//! Signatures do not cover every provider, token format, or personal-information category.

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

/// Compiles the checked-in rule set once; malformed bundled rules are programming defects, not input errors.
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

/// Rejects unknown allowlist IDs rather than silently treating a misspelled rule as exempt.
pub fn validate_policy(policy: &GatePolicy) -> Result<()> {
    for id in &policy.rule_allowlist {
        ensure!(
            rules().iter().any(|(rule, _)| rule.id == *id),
            "Unknown secret rule in allowlist"
        );
    }
    Ok(())
}

/// Reports captured secret byte spans and policy decisions without embedding the matched values.
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

/// Inspects nested values and keys, retaining only masked key paths and hashes in findings.
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

/// Scans metadata keys separately so a mapped field cannot bypass key detection.
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

/// Replaces signature capture spans, coalescing overlaps while preserving surrounding punctuation.
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

/// Masks JSON string values in place; key masking is handled at the record/report construction boundary.
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

    /// Builds policy variants without allowing an unregistered rule or disabling detection.
    fn policy(action: GateAction, allow: bool) -> GatePolicy {
        GatePolicy {
            secrets: action,
            high_risk_pii: GateAction::Pass,
            rule_allowlist: if allow {
                vec!["openai-api-key".into()]
            } else {
                vec![]
            },
            origin: PolicyOrigin::UserChoice,
            user_selected: true,
        }
    }

    /// Covers supported signature branches and punctuation; synthetic tokens are never printed on failure.
    #[test]
    fn supported_openai_shapes_and_delimiters_are_detected_and_masked() {
        let mut tokens = vec![format!("sk-{}T3BlbkFJ{}", "A".repeat(20), "B".repeat(20))];
        for prefix in ["proj", "svcacct", "admin"] {
            for left in [58, 74] {
                for right in [58, 74] {
                    tokens.push(format!(
                        "sk-{prefix}-{}T3BlbkFJ{}",
                        "A".repeat(left),
                        "B".repeat(right)
                    ));
                }
            }
        }
        // Underscores and hyphens are part of the supported project-token alphabet, including at the end.
        tokens.push(format!(
            "sk-proj-{}T3BlbkFJ{}",
            "a_-A".repeat(18) + "a_",
            "b_-B".repeat(18) + "b-"
        ));
        for token in tokens {
            for delimiter in [
                "", " ", "\n", ",", ")", "]", "}", ".", ":", "'", "\"", ";", "\\n", "\\r",
            ] {
                let text = format!("前缀 ({token}{delimiter}");
                for (action, allow, expected) in [
                    (GateAction::Pass, false, FindingDisposition::Passed),
                    (GateAction::Block, false, FindingDisposition::Blocked),
                    (GateAction::Block, true, FindingDisposition::Allowlisted),
                ] {
                    let findings = scan(&text, "/content", &policy(action, allow));
                    assert_eq!(findings.len(), 1);
                    assert_eq!(findings[0].disposition, expected);
                    let span = &findings[0].byte_span;
                    assert!(text[span.start as usize..span.end as usize] == token);
                    assert_eq!(mask(&text), format!("前缀 ([secret]{delimiter}"));
                }
            }
        }
    }

    /// Prevents matching adjacent token characters, wrong lengths/charset, or ordinary API-key discussion.
    #[test]
    fn openai_non_signatures_are_not_detected_or_masked() {
        let legacy = format!("sk-{}T3BlbkFJ{}", "A".repeat(20), "B".repeat(20));
        let samples = [
            "OpenAI API key sk-proj-placeholder".into(),
            format!("sk-{}T3BlbkFJ{}", "A".repeat(19), "B".repeat(20)),
            format!("sk-{}T3BlbkFJ{}", "A".repeat(20), "B".repeat(21)),
            format!("{legacy}_suffix"),
            format!("{legacy}-suffix"),
            format!("prefix_{legacy}"),
            format!("sk-proj-{}T3BlbkFJ{}", "A".repeat(57), "B".repeat(58)),
            format!("sk-proj-{}!T3BlbkFJ{}", "A".repeat(57), "B".repeat(58)),
            format!("sk-{}", "A".repeat(48)),
            "password handling is documented; no assignment appears here".into(),
        ];
        for text in samples {
            assert!(scan(&text, "/content", &policy(GateAction::Block, false)).is_empty());
            assert!(mask(&text) == text);
        }
    }

    /// Extends the same punctuation boundary to the supported Anthropic shape without matching longer tokens.
    #[test]
    fn anthropic_delimiters_preserve_exact_secret_span() {
        let token = format!("sk-ant-api03-{}AA", "A".repeat(93));
        for delimiter in ["", ",", ")", "]", "}", ".", "\n", "\\n"] {
            let text = format!("{token}{delimiter}");
            let findings = scan(&text, "/content", &policy(GateAction::Block, false));
            assert_eq!(findings.len(), 1);
            assert_eq!(findings[0].rule_id, "anthropic-api-key");
            assert_eq!(findings[0].byte_span.end as usize, token.len());
            assert_eq!(mask(&text), format!("[secret]{delimiter}"));
        }
        for suffix in ["A", "_", "-"] {
            assert!(
                scan(
                    &format!("{token}{suffix}"),
                    "/content",
                    &policy(GateAction::Block, false)
                )
                .is_empty()
            );
        }
    }

    /// Checks all other registered signatures and common benign discussion without claiming complete provider coverage.
    #[test]
    fn remaining_signatures_mask_capture_spans_and_ignore_benign_text() {
        for (rule, text) in [
            ("aws-access-token", format!("AKIA{}", "A".repeat(16))),
            ("github-pat", format!("ghp_TEST{}", "A".repeat(32))),
            (
                "github-fine-grained-pat",
                format!("github_pat_{}", "A".repeat(82)),
            ),
            (
                "private-key",
                format!(
                    "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----",
                    "A".repeat(64)
                ),
            ),
            (
                "password-assignment",
                "password=SyntheticAssignedValue".into(),
            ),
        ] {
            let findings = scan(&text, "/content", &policy(GateAction::Block, false));
            assert_eq!(findings.len(), 1);
            assert_eq!(findings[0].rule_id, rule);
            assert_eq!(findings[0].disposition, FindingDisposition::Blocked);
            let span = &findings[0].byte_span;
            let captured = &text[span.start as usize..span.end as usize];
            assert!(!mask(&text).contains(captured));
        }
        for text in [
            "Use AWS access keys, GitHub tokens and OpenAI API keys carefully.",
            "ghp_placeholder github_pat_placeholder sk-ant-api03-placeholder",
            "-----BEGIN PRIVATE KEY----- placeholder -----END PRIVATE KEY-----",
            "password=short",
        ] {
            assert!(scan(text, "/content", &policy(GateAction::Block, false)).is_empty());
            assert_eq!(mask(text), text);
        }
    }

    /// Covers overlapping password/token findings without leaving any captured suffix in the report.
    #[test]
    fn masking_overlapping_findings_does_not_leave_suffixes() {
        let text = format!("password=ghp_TEST{}", "A".repeat(32));
        let masked = mask(&text);
        assert_eq!(masked, "password=[secret]");
    }
}
