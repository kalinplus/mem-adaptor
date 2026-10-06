use std::sync::OnceLock;

use anyhow::bail;
use jsonschema::{Draft, Registry, Validator};
use serde::Serialize;
use serde_json::Value;

use crate::Result;

const SCHEMAS: [(&str, &str); 5] = [
    (
        "canonical-record",
        include_str!("../../../schema/canonical-record.schema.json"),
    ),
    (
        "plan-report",
        include_str!("../../../schema/plan-report.schema.json"),
    ),
    (
        "receipt-report",
        include_str!("../../../schema/receipt-report.schema.json"),
    ),
    (
        "approval-receipt",
        include_str!("../../../schema/approval-receipt.schema.json"),
    ),
    ("config", include_str!("../../../schema/config.schema.json")),
];

pub fn validate(name: &str, value: &impl Serialize) -> Result<()> {
    static VALIDATORS: OnceLock<std::collections::BTreeMap<&str, Validator>> = OnceLock::new();
    let validators = VALIDATORS.get_or_init(|| {
        let schemas: Vec<_> = SCHEMAS
            .iter()
            .map(|(name, text)| (*name, serde_json::from_str::<Value>(text).unwrap()))
            .collect();
        let mut registry = Registry::new();
        for (_, schema) in &schemas {
            registry = registry
                .add(schema["$id"].as_str().unwrap(), schema.clone())
                .unwrap();
        }
        let registry = registry.prepare().unwrap();
        schemas
            .into_iter()
            .map(|(name, schema)| {
                let validator = jsonschema::options()
                    .with_draft(Draft::Draft202012)
                    .with_registry(&registry)
                    .should_validate_formats(true)
                    .build(&schema)
                    .unwrap();
                (name, validator)
            })
            .collect()
    });
    let document = serde_json::to_value(value)?;
    crate::jcs::validate_numbers(&document)?;
    if let Some(error) = validators[name].iter_errors(&document).next() {
        bail!("Schema {name} violation at {}", error.instance_path());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_runtime_records_are_rejected_without_echoing_values() {
        let vector: Value = serde_json::from_str(include_str!(
            "../../../schema/vectors/valid/canonical-minimal.json"
        ))
        .unwrap();
        let mut record = vector["document"].clone();
        validate("canonical-record", &record).unwrap();
        record["scope"] = Value::String("synthetic-private-value".into());
        let error = validate("canonical-record", &record)
            .unwrap_err()
            .to_string();
        assert!(error.contains("/scope"));
        assert!(!error.contains("synthetic-private-value"));
    }
}
