//! Parses raw UMP JSON before any object member can overwrite an earlier member.
//! The Writer uses this local parser at every target observation; decoded names detect escaped duplicates.
//! Scalar syntax is delegated to serde_json, while all diagnostics omit input names and values.

use anyhow::{Context, bail, ensure};
use mem_adaptor_core::Result;
use serde_json::{Map, Number, Value};

/// Parses one complete JSON value with bounded nesting and duplicate-name rejection in every object.
pub(crate) fn parse(input: &[u8]) -> Result<Value> {
    let mut parser = Parser { input, offset: 0 };
    let value = parser.value(0)?;
    parser.whitespace();
    ensure!(parser.offset == input.len(), "Invalid UMP target array");
    Ok(value)
}

struct Parser<'a> {
    input: &'a [u8],
    offset: usize,
}

impl Parser<'_> {
    /// Parses a value without selecting among duplicate members; nesting is bounded like serde_json.
    fn value(&mut self, depth: usize) -> Result<Value> {
        ensure!(depth < 128, "UMP JSON nesting exceeds the supported limit");
        self.whitespace();
        match self.input.get(self.offset).copied() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b't') => self.literal(b"true", Value::Bool(true)),
            Some(b'f') => self.literal(b"false", Value::Bool(false)),
            Some(b'n') => self.literal(b"null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => bail!("Invalid UMP target array"),
        }
    }

    /// Builds an object only after proving each decoded name is unique within that object.
    fn object(&mut self, depth: usize) -> Result<Value> {
        self.offset += 1;
        let mut members = Map::new();
        self.whitespace();
        if self.consume(b'}') {
            return Ok(Value::Object(members));
        }
        loop {
            self.whitespace();
            let name = self.string()?;
            ensure!(
                !members.contains_key(&name),
                "Duplicate UMP JSON object member"
            );
            self.whitespace();
            ensure!(self.consume(b':'), "Invalid UMP target array");
            let value = self.value(depth)?;
            members.insert(name, value);
            self.whitespace();
            if self.consume(b'}') {
                return Ok(Value::Object(members));
            }
            ensure!(self.consume(b','), "Invalid UMP target array");
        }
    }

    /// Preserves array ordering and rejects missing delimiters or trailing commas.
    fn array(&mut self, depth: usize) -> Result<Value> {
        self.offset += 1;
        let mut values = Vec::new();
        self.whitespace();
        if self.consume(b']') {
            return Ok(Value::Array(values));
        }
        loop {
            values.push(self.value(depth)?);
            self.whitespace();
            if self.consume(b']') {
                return Ok(Value::Array(values));
            }
            ensure!(self.consume(b','), "Invalid UMP target array");
        }
    }

    /// Finds a complete string token, then delegates escapes, Unicode and UTF-8 validation without exposing errors.
    fn string(&mut self) -> Result<String> {
        let start = self.offset;
        ensure!(self.consume(b'"'), "Invalid UMP target array");
        loop {
            let byte = self
                .input
                .get(self.offset)
                .copied()
                .context("Invalid UMP target array")?;
            self.offset += 1;
            match byte {
                b'"' => {
                    return serde_json::from_slice(&self.input[start..self.offset])
                        .map_err(|_| anyhow::anyhow!("Invalid UMP target array"));
                }
                b'\\' => {
                    ensure!(self.offset < self.input.len(), "Invalid UMP target array");
                    self.offset += 1;
                }
                _ => {}
            }
        }
    }

    /// Parses a finite JSON number using serde_json's integer-preserving representation.
    fn number(&mut self) -> Result<Value> {
        let start = self.offset;
        while self
            .input
            .get(self.offset)
            .is_some_and(|byte| matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'))
        {
            self.offset += 1;
        }
        let number: Number = serde_json::from_slice(&self.input[start..self.offset])
            .map_err(|_| anyhow::anyhow!("Invalid UMP target array"))?;
        Ok(Value::Number(number))
    }

    /// Consumes a fixed JSON literal without accepting misspellings or echoing raw tokens.
    fn literal(&mut self, token: &[u8], value: Value) -> Result<Value> {
        ensure!(
            self.input[self.offset..].starts_with(token),
            "Invalid UMP target array"
        );
        self.offset += token.len();
        Ok(value)
    }

    /// Skips only the four whitespace bytes permitted by JSON.
    fn whitespace(&mut self) {
        while self
            .input
            .get(self.offset)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.offset += 1;
        }
    }

    /// Consumes one expected delimiter without advancing on mismatch.
    fn consume(&mut self, token: u8) -> bool {
        if self.input.get(self.offset) == Some(&token) {
            self.offset += 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checks valid grammar independently against serde_json, including escapes, scalars and empty containers.
    #[test]
    fn strict_parser_retains_valid_json_values() {
        for text in [
            r#"{"a":[null,true,false,-2,0.25,1e-3,{},[]],"s":"中文\n\"\\\u0061"}"#,
            r#"[{"\u0061":"\ud83d\ude00"}]"#,
            r#"[1,2,3]"#,
        ] {
            assert_eq!(
                parse(text.as_bytes()).unwrap(),
                serde_json::from_str::<Value>(text).unwrap()
            );
        }
    }

    /// Refuses ambiguous members and invalid grammar without echoing private names or raw payloads.
    #[test]
    fn strict_parser_rejects_duplicate_and_invalid_tokens() {
        for text in [
            r#"{"private-name":1,"private-name":2}"#,
            r#"{"a":1,"\u0061":2}"#,
            r#"{"x":{"a":1,"a":2}}"#,
        ] {
            assert_eq!(
                parse(text.as_bytes()).unwrap_err().to_string(),
                "Duplicate UMP JSON object member"
            );
        }
        for text in [
            r#"{"a":}"#,
            "[1,]",
            "{",
            r#"["bad\q"]"#,
            "[01]",
            "[1] trailing",
            r#"{"a":truefalse}"#,
        ] {
            assert_eq!(
                parse(text.as_bytes()).unwrap_err().to_string(),
                "Invalid UMP target array"
            );
        }
    }
}
