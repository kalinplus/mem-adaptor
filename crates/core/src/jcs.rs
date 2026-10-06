use anyhow::ensure;
use serde::Serialize;
use serde_json::Value;

const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

pub fn validate_numbers(value: &Value) -> crate::Result<()> {
    match value {
        Value::Number(number) => {
            let safe = if let Some(value) = number.as_u64() {
                value <= MAX_SAFE_INTEGER
            } else if let Some(value) = number.as_i64() {
                value.unsigned_abs() <= MAX_SAFE_INTEGER
            } else {
                let value = number.as_f64().unwrap();
                value.fract() != 0.0 || value.abs() <= MAX_SAFE_INTEGER as f64
            };
            ensure!(safe, "JSON integer exceeds the JCS safe range");
        }
        Value::Array(values) => {
            for value in values {
                validate_numbers(value)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                validate_numbers(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn to_vec(value: &impl Serialize) -> crate::Result<Vec<u8>> {
    let document = serde_json::to_value(value)?;
    validate_numbers(&document)?;
    Ok(serde_jcs::to_vec(&document)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unsafe_integers_are_rejected_without_rounding_or_echoing_values() {
        for number in [
            json!(9007199254740992_u64),
            json!(9007199254740993_u64),
            json!(-9007199254740992_i64),
            json!(1e20),
        ] {
            let document = json!({"nested": [{"count": number}]});
            assert!(to_vec(&document).is_err());
        }
        for number in [
            json!(9007199254740991_u64),
            json!(-9007199254740991_i64),
            json!(0.1),
        ] {
            assert!(to_vec(&json!({"count": number})).is_ok());
        }
    }
}
