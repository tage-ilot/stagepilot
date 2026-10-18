//! Validated ProPresenter timer and Look models (`models.py`), including the wrapper shapes the
//! Python validators accept.

use serde_json::{json, Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identifier {
    pub uuid: String,
    pub name: String,
    pub index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Countdown {
    pub duration: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Look {
    pub id: Identifier,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Timer {
    pub id: Identifier,
    pub allows_overrun: bool,
    pub countdown: Option<Countdown>,
    pub count_down_to_time: Option<Map<String, Value>>,
    pub elapsed: Option<Map<String, Value>>,
    pub state: Option<String>,
    pub time: Option<String>,
}

impl Timer {
    /// `timer_type`, used in the "still reported ..." message.
    pub fn timer_type(&self) -> &'static str {
        if self.countdown.is_some() {
            "Countdown Timer"
        } else if self.count_down_to_time.is_some() {
            "Countdown to Time"
        } else if self.elapsed.is_some() {
            "Elapsed Time"
        } else {
            "an unknown timer type"
        }
    }

    /// A countdown definition, replacing any previous timer type.
    pub fn update_payload(&self, duration_seconds: u32) -> Value {
        json!({
            "id": {"uuid": self.id.uuid, "name": self.id.name, "index": self.id.index},
            "allows_overrun": self.allows_overrun,
            "countdown": {"duration": duration_seconds},
        })
    }

    pub fn from_value(value: &Value) -> Option<Timer> {
        let data = normalise_timer(value)?;
        let id = identifier(data.get("id")?)?;
        let allows_overrun = match data.get("allows_overrun") {
            None => false,
            Some(Value::Bool(b)) => *b,
            _ => return None,
        };
        let countdown = match data.get("countdown") {
            None | Some(Value::Null) => None,
            Some(Value::Object(o)) => Some(Countdown {
                duration: non_negative(o.get("duration")?)?,
            }),
            _ => return None,
        };
        let object = |key: &str| -> Result<Option<Map<String, Value>>, ()> {
            match data.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::Object(o)) => Ok(Some(o.clone())),
                _ => Err(()),
            }
        };
        let text = |key: &str| -> Result<Option<String>, ()> {
            match data.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(s)) => Ok(Some(s.clone())),
                _ => Err(()),
            }
        };
        Some(Timer {
            id,
            allows_overrun,
            countdown,
            count_down_to_time: object("count_down_to_time").ok()?,
            elapsed: object("elapsed").ok()?,
            state: text("state").ok()?,
            time: text("time").ok()?,
        })
    }
}

impl Look {
    pub fn from_value(value: &Value) -> Option<Look> {
        let mut data = value.as_object()?.clone();
        if let Some(Value::Object(nested)) = data.get("look") {
            data = nested.clone();
        }
        match data.get("id").cloned() {
            Some(Value::String(uuid)) => {
                data.insert(
                    "id".into(),
                    json!({
                        "uuid": uuid,
                        "name": data.get("name").cloned().unwrap_or(Value::Null),
                        "index": data.get("index").cloned().unwrap_or(json!(0)),
                    }),
                );
            }
            Some(Value::Object(_)) => {}
            _ => {
                if let (Some(Value::String(uuid)), Some(Value::String(name))) =
                    (data.get("uuid"), data.get("name"))
                {
                    let id = json!({
                        "uuid": uuid,
                        "name": name,
                        "index": data.get("index").cloned().unwrap_or(json!(0)),
                    });
                    data.insert("id".into(), id);
                }
            }
        }
        Some(Look {
            id: identifier(data.get("id")?)?,
        })
    }
}

/// pydantic lax `int` with `ge=0`: integers, integral floats and decimal strings.
fn non_negative(value: &Value) -> Option<u32> {
    let n = match value {
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))?,
        Value::String(s) => s.trim().parse::<i64>().ok()?,
        _ => return None,
    };
    u32::try_from(n).ok()
}

fn identifier(value: &Value) -> Option<Identifier> {
    let object = value.as_object()?;
    let uuid = object.get("uuid")?.as_str().filter(|s| !s.is_empty())?;
    let name = object.get("name")?.as_str().filter(|s| !s.is_empty())?;
    Some(Identifier {
        uuid: uuid.to_string(),
        name: name.to_string(),
        index: non_negative(object.get("index")?)?,
    })
}

/// `normalize_legacy_timer_shapes`.
fn normalise_timer(value: &Value) -> Option<Map<String, Value>> {
    let mut data = value.as_object()?.clone();
    if let Some(Value::Object(nested)) = data.get("timer") {
        let mut merged = nested.clone();
        if !merged.contains_key("id") {
            if let Some(id) = data.get("id") {
                merged.insert("id".into(), id.clone());
            }
        }
        if !merged.contains_key("id") {
            if let (Some(Value::String(name)), Some(Value::String(uuid)), Some(index)) =
                (data.get("name"), data.get("uuid"), data.get("index"))
            {
                if index.is_i64() || index.is_u64() {
                    merged.insert(
                        "id".into(),
                        json!({"name": name, "uuid": uuid, "index": index}),
                    );
                }
            }
        }
        data = merged;
    }
    match data.get("id").cloned() {
        Some(Value::String(uuid)) => {
            let name = data.get("name").cloned();
            let index = data.get("index").cloned().unwrap_or(json!(0));
            if let (Some(name @ Value::String(_)), true) = (name, index.is_i64() || index.is_u64())
            {
                data.insert(
                    "id".into(),
                    json!({"uuid": uuid, "name": name, "index": index}),
                );
            }
        }
        Some(Value::Object(id)) => {
            let mut id = id;
            id.entry("name")
                .or_insert_with(|| data.get("name").cloned().unwrap_or(Value::Null));
            id.entry("index")
                .or_insert_with(|| data.get("index").cloned().unwrap_or(json!(0)));
            data.insert("id".into(), Value::Object(id));
        }
        _ => {}
    }
    Some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_and_wrapper_shapes_validate() {
        let documented =
            json!({"id": {"uuid": "u", "name": "T", "index": 2}, "countdown": {"duration": 60}});
        assert_eq!(
            Timer::from_value(&documented)
                .unwrap()
                .countdown
                .unwrap()
                .duration,
            60
        );
        let wrapped = json!({"timer": {"id": {"uuid": "u", "name": "T", "index": 0}, "elapsed": {"start_time": 0}}});
        assert_eq!(
            Timer::from_value(&wrapped).unwrap().timer_type(),
            "Elapsed Time"
        );
        let flat =
            json!({"timer": {"countdown": {"duration": 5}}, "name": "T", "uuid": "u", "index": 3});
        assert_eq!(Timer::from_value(&flat).unwrap().id.index, 3);
        let string_id = json!({"id": "u", "name": "T", "index": 1});
        assert_eq!(Timer::from_value(&string_id).unwrap().id.uuid, "u");
        let missing_index = json!({"id": {"uuid": "u", "name": "T"}});
        assert_eq!(Timer::from_value(&missing_index).unwrap().id.index, 0);
    }

    #[test]
    fn invalid_timers_are_rejected() {
        assert!(Timer::from_value(&json!({"id": {"uuid": "", "name": "T", "index": 0}})).is_none());
        assert!(
            Timer::from_value(&json!({"id": {"uuid": "u", "name": "T", "index": -1}})).is_none()
        );
        assert!(Timer::from_value(
            &json!({"id": {"uuid": "u", "name": "T", "index": 0}, "countdown": {"duration": -1}})
        )
        .is_none());
        assert!(Timer::from_value(
            &json!({"id": {"uuid": "u", "name": "T", "index": 0}, "countdown": 5})
        )
        .is_none());
        assert!(Timer::from_value(&json!({"name": "no id"})).is_none());
        assert!(Timer::from_value(&json!([1])).is_none());
    }

    #[test]
    fn look_shapes() {
        let a =
            Look::from_value(&json!({"id": {"uuid": "l", "name": "Worship", "index": 0}})).unwrap();
        assert_eq!(a.id.name, "Worship");
        let b = Look::from_value(&json!({"look": {"id": "l", "name": "W", "index": 4}})).unwrap();
        assert_eq!((b.id.uuid.as_str(), b.id.index), ("l", 4));
        let c = Look::from_value(&json!({"uuid": "l", "name": "W"})).unwrap();
        assert_eq!(c.id.index, 0);
        assert!(Look::from_value(&json!({"name": "x"})).is_none());
    }

    #[test]
    fn update_payload_keeps_identity_and_replaces_the_type() {
        let elapsed = Timer::from_value(
            &json!({"id": {"uuid": "u", "name": "T", "index": 0}, "elapsed": {"start_time": 0}}),
        )
        .unwrap();
        assert_eq!(
            elapsed.update_payload(253),
            json!({"id": {"uuid": "u", "name": "T", "index": 0}, "allows_overrun": false, "countdown": {"duration": 253}})
        );
    }
}
