//! pydantic/FastAPI-shaped request validation errors (`{"detail": [...]}`) and small JSON helpers.

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

pub struct Json(pub StatusCode, pub Value);

impl IntoResponse for Json {
    fn into_response(self) -> Response {
        let mut response = (self.0, self.1.to_string()).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        response
    }
}

pub fn ok(value: Value) -> Json {
    Json(StatusCode::OK, value)
}

/// `HTTPException(status, detail)`.
pub fn http_error(status: u16, detail: &str) -> Json {
    Json(
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        json!({ "detail": detail }),
    )
}

pub fn unprocessable(detail: Vec<Value>) -> Json {
    Json(
        StatusCode::UNPROCESSABLE_ENTITY,
        json!({ "detail": detail }),
    )
}

fn missing(loc: Vec<Value>, input: &Value) -> Value {
    json!({"type": "missing", "loc": loc, "msg": "Field required", "input": input})
}

fn body_loc(field: &str) -> Vec<Value> {
    vec![json!("body"), json!(field)]
}

/// A body that is not a JSON object (or not JSON at all).
pub fn invalid_body(raw: &[u8], parsed: Option<&Value>) -> Vec<Value> {
    match parsed {
        None if raw.is_empty() => vec![
            json!({"type": "missing", "loc": ["body"], "msg": "Field required", "input": null}),
        ],
        None => vec![json!({
            "type": "json_invalid", "loc": ["body", 0],
            "msg": "JSON decode error", "input": {}, "ctx": {"error": "Expecting value"}
        })],
        Some(value) => vec![json!({
            "type": "model_attributes_type", "loc": ["body"],
            "msg": "Input should be a valid dictionary or object to extract fields from",
            "input": value
        })],
    }
}

/// `{"pin": str(4..=64)}`.
pub fn dashboard_login(raw: &[u8]) -> Result<String, Vec<Value>> {
    let parsed: Option<Value> = serde_json::from_slice(raw).ok();
    let Some(Value::Object(map)) = &parsed else {
        return Err(invalid_body(raw, parsed.as_ref()));
    };
    let input = Value::Object(map.clone());
    match map.get("pin") {
        None => Err(vec![missing(body_loc("pin"), &input)]),
        Some(Value::String(pin)) => {
            let chars = pin.chars().count();
            if chars < 4 {
                Err(vec![
                    json!({"type": "string_too_short", "loc": ["body", "pin"],
                    "msg": "String should have at least 4 characters", "input": pin,
                    "ctx": {"min_length": 4}}),
                ])
            } else if chars > 64 {
                Err(vec![
                    json!({"type": "string_too_long", "loc": ["body", "pin"],
                    "msg": "String should have at most 64 characters", "input": pin,
                    "ctx": {"max_length": 64}}),
                ])
            } else {
                Ok(pin.clone())
            }
        }
        Some(other) => Err(vec![json!({"type": "string_type", "loc": ["body", "pin"],
            "msg": "Input should be a valid string", "input": other})]),
    }
}

pub struct DashboardAccess {
    pub enabled: bool,
    pub pin: Option<String>,
}

/// `{"enabled": bool, "pin": SecretStr | None (4..=64)}`. FastAPI rejects a short `SecretStr`
/// with a `too_short` error (length checked after validation), so the messages differ from the
/// plain string case above.
pub fn dashboard_access(raw: &[u8]) -> Result<DashboardAccess, Vec<Value>> {
    let parsed: Option<Value> = serde_json::from_slice(raw).ok();
    let Some(Value::Object(map)) = &parsed else {
        return Err(invalid_body(raw, parsed.as_ref()));
    };
    let input = Value::Object(map.clone());
    let mut errors = Vec::new();
    let enabled = match map.get("enabled") {
        None => {
            errors.push(missing(body_loc("enabled"), &input));
            false
        }
        Some(Value::Bool(b)) => *b,
        Some(other) => {
            errors.push(json!({"type": "bool_parsing", "loc": ["body", "enabled"],
                "msg": "Input should be a valid boolean, unable to interpret input", "input": other}));
            false
        }
    };
    let pin = match map.get("pin") {
        None | Some(Value::Null) => None,
        Some(Value::String(pin)) => {
            let chars = pin.chars().count();
            if chars < 4 {
                errors.push(json!({"type": "too_short", "loc": ["body", "pin"],
                    "msg": format!("Value should have at least 4 items after validation, not {chars}"),
                    "input": pin,
                    "ctx": {"field_type": "Value", "min_length": 4, "actual_length": chars}}));
                None
            } else if chars > 64 {
                errors.push(json!({"type": "too_long", "loc": ["body", "pin"],
                    "msg": format!("Value should have at most 64 items after validation, not {chars}"),
                    "input": pin,
                    "ctx": {"field_type": "Value", "max_length": 64, "actual_length": chars}}));
                None
            } else {
                Some(pin.clone())
            }
        }
        Some(other) => {
            errors.push(json!({"type": "string_type", "loc": ["body", "pin"],
                "msg": "Input should be a valid string", "input": other}));
            None
        }
    };
    if errors.is_empty() {
        Ok(DashboardAccess { enabled, pin })
    } else {
        Err(errors)
    }
}
