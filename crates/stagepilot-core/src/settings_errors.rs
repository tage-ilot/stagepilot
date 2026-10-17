//! Map `serde_json` deserialisation failures onto pydantic's error objects so the HTTP layer can
//! answer `422` with the same `detail` list shape FastAPI produces (`type`, `loc`, `msg`, `input`).
//!
//! Only the first failure is reported (serde stops at the first error; pydantic reports all).

use crate::config::Detail;
use serde_json::{json, Value};

fn lookup<'a>(root: &'a Value, path: &[Value]) -> Option<&'a Value> {
    let mut node = root;
    for step in path {
        node = match (node, step) {
            (Value::Object(m), Value::String(k)) => m.get(k)?,
            (Value::Array(a), Value::Number(n)) => a.get(n.as_u64()? as usize)?,
            _ => return None,
        };
    }
    Some(node)
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let end = text[start..].find(close)? + start;
    Some(&text[start..end])
}

fn variants(message: &str) -> Vec<String> {
    let tail = message
        .split("expected one of ")
        .nth(1)
        .or_else(|| message.split("expected ").nth(1))
        .unwrap_or("");
    tail.split('`')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, s)| s.to_string())
        .collect()
}

pub fn map_serde_error(
    error: &serde_path_to_error::Error<serde_json::Error>,
    migrated: &Value,
    original: &Value,
) -> Detail {
    let mut loc: Vec<Value> = error
        .path()
        .iter()
        .map(|seg| match seg {
            serde_path_to_error::Segment::Seq { index } => json!(index),
            serde_path_to_error::Segment::Map { key } => json!(key),
            serde_path_to_error::Segment::Enum { variant } => json!(variant),
            serde_path_to_error::Segment::Unknown => json!("?"),
        })
        .collect();
    let message = error.inner().to_string();
    let message = message
        .split(" at line ")
        .next()
        .unwrap_or(&message)
        .to_string();
    let at = |loc: &[Value]| {
        lookup(migrated, loc)
            .or_else(|| lookup(original, loc))
            .cloned()
            .unwrap_or(Value::Null)
    };
    let detail =
        |kind: &str, loc: Vec<Value>, msg: &str, input: Value, ctx: Option<Value>| Detail {
            kind: kind.into(),
            loc,
            msg: msg.into(),
            input,
            ctx,
        };

    if let Some(field) = between(&message, "unknown field `", "`") {
        // serde_path_to_error already ends the path at the unknown key.
        if loc.last() == Some(&json!(field)) {
            loc.pop();
        }
        let input = at(&loc).get(field).cloned().unwrap_or(Value::Null);
        loc.push(json!(field));
        return detail(
            "extra_forbidden",
            loc,
            "Extra inputs are not permitted",
            input,
            None,
        );
    }
    if let Some(field) = between(&message, "missing field `", "`") {
        let input = at(&loc);
        loc.push(json!(field));
        return detail("missing", loc, "Field required", input, None);
    }
    let input = at(&loc);
    if message.starts_with("unknown variant") {
        let names = variants(&message);
        let quoted: Vec<String> = names.iter().map(|n| format!("'{n}'")).collect();
        let list = match quoted.as_slice() {
            [] => String::new(),
            [only] => only.clone(),
            [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
        };
        return detail(
            "enum",
            loc,
            &format!("Input should be {list}"),
            input,
            Some(json!({ "expected": list })),
        );
    }
    if let Some(expected) = message
        .strip_prefix("invalid type: ")
        .and_then(|rest| rest.split(", expected ").nth(1))
    {
        let received_string = message.starts_with("invalid type: string");
        let (kind, msg) = if expected.contains("string") || expected.contains("str") {
            ("string_type", "Input should be a valid string")
        } else if expected.starts_with('u') || expected.starts_with('i') {
            if received_string {
                (
                    "int_parsing",
                    "Input should be a valid integer, unable to parse string as an integer",
                )
            } else {
                ("int_type", "Input should be a valid integer")
            }
        } else if expected.starts_with('f') {
            if received_string {
                (
                    "float_parsing",
                    "Input should be a valid number, unable to parse string as a number",
                )
            } else {
                ("float_type", "Input should be a valid number")
            }
        } else if expected.contains("bool") {
            ("bool_type", "Input should be a valid boolean")
        } else if expected.contains("sequence") || expected.contains("seq") {
            ("list_type", "Input should be a valid list")
        } else if expected.contains("map") || expected.contains("struct") {
            ("dict_type", "Input should be a valid dictionary")
        } else {
            ("value_error", "Input is not valid")
        };
        return detail(kind, loc, msg, input, None);
    }
    if let Some(rest) = message.strip_prefix("invalid value: integer `") {
        let number = rest.split('`').next().unwrap_or("0");
        if number.starts_with('-') {
            return detail(
                "greater_than_equal",
                loc,
                "Input should be greater than or equal to 0",
                input,
                Some(json!({ "ge": 0 })),
            );
        }
        let max = message
            .rsplit("expected ")
            .next()
            .map(|t| match t.trim() {
                "u8" => 255,
                "u16" => 65535,
                _ => 4_294_967_295u64,
            })
            .unwrap_or(255);
        return detail(
            "less_than_equal",
            loc,
            &format!("Input should be less than or equal to {max}"),
            input,
            Some(json!({ "le": max })),
        );
    }
    detail(
        "value_error",
        loc,
        &format!("Value error, {message}"),
        input,
        None,
    )
}
