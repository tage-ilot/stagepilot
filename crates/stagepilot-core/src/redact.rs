//! Credential redaction for structured log data: the port of `redact_secrets` / `_redact` in
//! `stagepilot/core/logging.py`. Rust logs go through `tracing`, which has no processor chain, so
//! callers that log arbitrary fields run them through [`redact_value`] first.

use regex::Regex;
use serde_json::{Map, Value};
use std::sync::OnceLock;

pub const REDACTED: &str = "[REDACTED]";

fn sensitive_key() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(?:authorization|cookie|credential|password|secret|token)").unwrap()
    })
}

fn sensitive_text() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(bearer\s+|basic\s+|(?:password|secret|token|api[_-]?key)\s*[=:]\s*)[^\s,;&]+",
        )
        .unwrap()
    })
}

/// Redact credentials inside free text (`Bearer x`, `token=x`, ...).
pub fn redact_text(text: &str) -> String {
    sensitive_text()
        .replace_all(text, |caps: &regex::Captures<'_>| {
            format!("{}{REDACTED}", &caps[1])
        })
        .into_owned()
}

/// `_redact(value, key)`: a sensitive key hides the whole value; objects and arrays are walked
/// (array items carry no key, as in Python); strings are scrubbed; everything else is kept.
pub fn redact_value(value: &Value, key: &str) -> Value {
    if sensitive_key().is_match(key) {
        return Value::String(REDACTED.into());
    }
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), redact_value(v, k)))
                .collect::<Map<_, _>>(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(|v| redact_value(v, "")).collect()),
        Value::String(text) => Value::String(redact_text(text)),
        other => other.clone(),
    }
}

/// `redact_secrets`: redact every top-level field of a log event.
pub fn redact_secrets(event: &Map<String, Value>) -> Map<String, Value> {
    event
        .iter()
        .map(|(k, v)| (k.clone(), redact_value(v, k)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    /// `test_structured_log_redaction_covers_keys_nested_values_and_messages`
    #[test]
    fn redaction_covers_keys_nested_values_and_messages() {
        let event = redact_secrets(&object(json!({
            "event": "request bearer abc123 token=hidden okay",
            "authorization": "Bearer private",
            "nested": {"password": "private", "safe": "visible"},
            "items": [{"connector_token": "private"}],
        })));
        assert_eq!(event["authorization"], "[REDACTED]");
        assert_eq!(
            event["nested"],
            json!({"password": "[REDACTED]", "safe": "visible"})
        );
        assert_eq!(event["items"], json!([{"connector_token": "[REDACTED]"}]));
        assert_eq!(
            event["event"],
            "request bearer [REDACTED] token=[REDACTED] okay"
        );
    }

    #[test]
    fn text_patterns_match_python_including_case_and_separators() {
        assert_eq!(redact_text("Basic dXNlcjpwdw=="), "Basic [REDACTED]");
        assert_eq!(
            redact_text("API_KEY: k1, next"),
            "API_KEY: [REDACTED], next"
        );
        assert_eq!(redact_text("api-key=k1&x=1"), "api-key=[REDACTED]&x=1");
        assert_eq!(
            redact_text("nothing secret-looking here"),
            "nothing secret-looking here"
        );
    }

    #[test]
    fn non_string_scalars_and_keyless_list_strings() {
        assert_eq!(redact_value(&json!(7), "count"), json!(7));
        // List items are walked without their parent key, exactly as in Python.
        assert_eq!(
            redact_value(&json!(["token=a", "b"]), "lines"),
            json!(["token=[REDACTED]", "b"])
        );
        // A sensitive key hides even a non-string value.
        assert_eq!(
            redact_value(&json!({"x": 1}), "Cookie"),
            json!("[REDACTED]")
        );
    }
}
