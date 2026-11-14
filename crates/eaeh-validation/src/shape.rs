//! Human- and model-readable views of a JSON Schema, generated from the schema itself so
//! they can never drift from what validation enforces:
//!
//! * [`example`]: a minimal instance that is **valid** against the schema, with placeholder
//!   values (every property shown, arrays with their minimum number of items);
//! * [`outline`]: one line per field: path, type, whether it is required, allowed values,
//!   and whether other fields are allowed.
//!
//! Supported keywords: `type` (single or list), `properties`, `required`,
//! `additionalProperties`, `items`, `minItems`, `const`, `enum`, `examples`, `pattern` (a
//! simple subset: literals, `(a|b)`, `[0-9]`/`[a-z]`/`[A-Z]` classes and `{n}`, `{n,}`,
//! `{n,m}`, `+`, `*`, `?`), `minLength`, `minimum`. [`example`] returns `None` for anything it
//! cannot satisfy; tests check every shipped agent schema.

use serde_json::{Map, Value, json};

/// Combine an agent's output schema with the output contracts of its skills into the one
/// structure a result must satisfy (all of them at once): `required` lists are united,
/// properties and array items are merged recursively, and a constraint present in only one
/// schema (a type, enum, pattern, `additionalProperties: false` …) is kept.
pub fn merge(base: &Value, extra: &Value) -> Value {
    match (base, extra) {
        (Value::Object(a), Value::Object(b)) => {
            let mut out = a.clone();
            for (k, bv) in b {
                let merged = match (k.as_str(), out.get(k)) {
                    ("required", Some(Value::Array(ar))) => {
                        let mut all = ar.clone();
                        for x in bv.as_array().into_iter().flatten() {
                            if !all.contains(x) {
                                all.push(x.clone());
                            }
                        }
                        Value::Array(all)
                    }
                    ("properties", Some(Value::Object(ap))) => {
                        let mut props = ap.clone();
                        for (pk, pv) in bv.as_object().into_iter().flatten() {
                            let m = match props.get(pk) {
                                Some(existing) => merge(existing, pv),
                                None => pv.clone(),
                            };
                            props.insert(pk.clone(), m);
                        }
                        Value::Object(props)
                    }
                    ("items", Some(ai)) => merge(ai, bv),
                    ("minItems", Some(av)) => {
                        json!(av.as_u64().unwrap_or(0).max(bv.as_u64().unwrap_or(0)))
                    }
                    (_, Some(existing)) => existing.clone(),
                    (_, None) => bv.clone(),
                };
                out.insert(k.clone(), merged);
            }
            Value::Object(out)
        }
        (a, _) => a.clone(),
    }
}

/// A valid example instance of `schema`, or `None` if a construct is not supported.
pub fn example(schema: &Value) -> Option<Value> {
    build(schema, "value", 1)
}

fn first_type(schema: &Value) -> Option<String> {
    match schema.get("type") {
        Some(Value::String(t)) => Some(t.clone()),
        Some(Value::Array(ts)) => ts
            .iter()
            .filter_map(Value::as_str)
            .find(|t| *t != "null")
            .or_else(|| ts.iter().filter_map(Value::as_str).next())
            .map(str::to_string),
        _ => {
            if schema.get("properties").is_some() || schema.get("required").is_some() {
                Some("object".into())
            } else if schema.get("items").is_some() {
                Some("array".into())
            } else {
                None
            }
        }
    }
}

fn singular(name: &str) -> String {
    name.strip_suffix("ies")
        .map(|s| format!("{s}y"))
        .or_else(|| name.strip_suffix('s').map(str::to_string))
        .unwrap_or_else(|| name.to_string())
}

fn build(schema: &Value, name: &str, n: usize) -> Option<Value> {
    if let Some(c) = schema.get("const") {
        return Some(c.clone());
    }
    if let Some(e) = schema.get("enum").and_then(Value::as_array) {
        return e.first().cloned();
    }
    if let Some(x) = schema
        .get("examples")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
    {
        return Some(x.clone());
    }
    match first_type(schema).as_deref() {
        Some("object") | None if schema.get("properties").is_some() || schema.get("required").is_some() => {
            let mut m = Map::new();
            if let Some(props) = schema.get("properties").and_then(Value::as_object) {
                for (k, s) in props {
                    m.insert(k.clone(), build(s, k, n)?);
                }
            }
            // Required fields without a description still have to be present.
            for r in schema
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(k) = r.as_str() {
                    m.entry(k.to_string()).or_insert_with(|| json!(format!("<{k}>")));
                }
            }
            Some(Value::Object(m))
        }
        Some("object") => Some(json!({})),
        Some("array") => {
            let min = schema.get("minItems").and_then(Value::as_u64).unwrap_or(1).max(1) as usize;
            let item = schema.get("items").cloned().unwrap_or(json!({}));
            let one = singular(name);
            (1..=min)
                .map(|i| build(&item, &one, i))
                .collect::<Option<Vec<_>>>()
                .map(Value::Array)
        }
        Some("string") => {
            if let Some(p) = schema.get("pattern").and_then(Value::as_str) {
                return sample_pattern(p, n).map(Value::String);
            }
            let mut s = if n > 1 {
                format!("<{name} {n}>")
            } else {
                format!("<{name}>")
            };
            let min = schema.get("minLength").and_then(Value::as_u64).unwrap_or(0) as usize;
            while s.chars().count() < min {
                s.push('.');
            }
            Some(Value::String(s))
        }
        Some("integer") => Some(json!(
            schema.get("minimum").and_then(Value::as_i64).unwrap_or(0).max(0)
        )),
        Some("number") => Some(json!(
            schema.get("minimum").and_then(Value::as_f64).unwrap_or(0.0)
        )),
        Some("boolean") => Some(json!(false)),
        Some("null") => Some(Value::Null),
        _ => Some(Value::String(format!("<{name}>"))),
    }
}

/// A string matching a simple anchored regular expression; `n` varies the digits.
fn sample_pattern(p: &str, n: usize) -> Option<String> {
    let body = p.strip_prefix('^').unwrap_or(p);
    let body = body.strip_suffix('$').unwrap_or(body);
    let chars: Vec<char> = body.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    // (atom text generator, min repetitions)
    while i < chars.len() {
        let (atom, next): (Box<dyn Fn(usize, usize) -> String>, usize) = match chars[i] {
            '(' => {
                let close = chars[i..].iter().position(|c| *c == ')')? + i;
                let inner: String = chars[i + 1..close].iter().collect();
                let first = inner.split('|').next()?.to_string();
                if first.chars().any(|c| "[](){}*+?.\\".contains(c)) {
                    return None;
                }
                (Box::new(move |_, _| first.clone()), close + 1)
            }
            '[' => {
                let close = chars[i..].iter().position(|c| *c == ']')? + i;
                let class: String = chars[i + 1..close].iter().collect();
                let digit = class.contains("0-9");
                let lower = class.contains("a-z");
                let upper = class.contains("A-Z");
                if !(digit || lower || upper) {
                    return None;
                }
                (
                    Box::new(move |k, total| {
                        if digit {
                            // the last digit carries the sequence number, e.g. 001, 002
                            if k + 1 == total {
                                char::from_digit((n % 10) as u32, 10).unwrap_or('1').to_string()
                            } else {
                                "0".into()
                            }
                        } else if lower {
                            "a".into()
                        } else {
                            "A".into()
                        }
                    }),
                    close + 1,
                )
            }
            '\\' => {
                let c = *chars.get(i + 1)?;
                if c.is_ascii_alphanumeric() {
                    return None; // \d, \w … not supported
                }
                (Box::new(move |_, _| c.to_string()), i + 2)
            }
            c if "]){}*+?.|".contains(c) => return None,
            c => (Box::new(move |_, _| c.to_string()), i + 1),
        };
        // quantifier
        let (count, after) = match chars.get(next) {
            Some('{') => {
                let close = chars[next..].iter().position(|c| *c == '}')? + next;
                let q: String = chars[next + 1..close].iter().collect();
                let min: usize = q.split(',').next()?.trim().parse().ok()?;
                (min, close + 1)
            }
            Some('+') => (1, next + 1),
            Some('*') | Some('?') => (0, next + 1),
            _ => (1, next),
        };
        for k in 0..count {
            out.push_str(&atom(k, count));
        }
        i = after;
    }
    Some(out)
}

/// One line per field of `schema`, e.g. `decisions[].status: one of proposed | accepted (required)`.
pub fn outline(schema: &Value) -> Vec<String> {
    let mut lines = vec![];
    walk(schema, "", true, &mut lines);
    lines.truncate(400);
    lines
}

fn describe(schema: &Value) -> String {
    if let Some(c) = schema.get("const") {
        return format!("exactly {c}");
    }
    if let Some(e) = schema.get("enum").and_then(Value::as_array) {
        return format!(
            "one of {}",
            e.iter()
                .map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))
                .collect::<Vec<_>>()
                .join(" | ")
        );
    }
    let t = match schema.get("type") {
        Some(Value::Array(ts)) => ts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" or "),
        Some(Value::String(t)) => t.clone(),
        _ => first_type(schema).unwrap_or_else(|| "any".into()),
    };
    let t = match t.as_str() {
        "array" => {
            let item = schema.get("items").map(describe).unwrap_or_else(|| "any".into());
            let min = schema
                .get("minItems")
                .and_then(Value::as_u64)
                .map(|m| format!(", at least {m}"))
                .unwrap_or_default();
            format!("array of {item}{min}")
        }
        _ => t,
    };
    match schema.get("pattern").and_then(Value::as_str) {
        Some(p) => format!("{t} matching {p}"),
        None => t,
    }
}

fn walk(schema: &Value, path: &str, required: bool, out: &mut Vec<String>) {
    let label = if path.is_empty() {
        "(the whole result)".to_string()
    } else {
        path.to_string()
    };
    let req = if path.is_empty() {
        ""
    } else if required {
        " (required)"
    } else {
        " (optional)"
    };
    let obj = schema.get("properties").and_then(Value::as_object);
    match obj {
        Some(props) => {
            let reqd: Vec<&str> = schema
                .get("required")
                .and_then(Value::as_array)
                .map(|r| r.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let closed = schema.get("additionalProperties") == Some(&Value::Bool(false));
            out.push(format!(
                "{label}: object{req}; required fields: {}{}",
                if reqd.is_empty() {
                    "none".to_string()
                } else {
                    reqd.join(", ")
                },
                if closed {
                    format!(
                        "; NO other fields allowed (allowed: {})",
                        props.keys().cloned().collect::<Vec<_>>().join(", ")
                    )
                } else {
                    String::new()
                }
            ));
            for (k, s) in props {
                let child = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                walk(s, &child, reqd.contains(&k.as_str()), out);
            }
        }
        None => {
            if let Some(items) = schema.get("items")
                && items.get("properties").is_some()
            {
                let min = schema
                    .get("minItems")
                    .and_then(Value::as_u64)
                    .map(|m| format!(", at least {m}"))
                    .unwrap_or_default();
                out.push(format!("{label}: array of objects{min}{req}"));
                walk(items, &format!("{path}[]"), true, out);
            } else {
                out.push(format!("{label}: {}{req}", describe(schema)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::CompiledSchema;

    #[test]
    fn patterns_are_sampled() {
        assert_eq!(sample_pattern("^REQ-(F|NF)-[0-9]{3,}$", 1).unwrap(), "REQ-F-001");
        assert_eq!(sample_pattern("^REQ-(F|NF)-[0-9]{3,}$", 2).unwrap(), "REQ-F-002");
        assert_eq!(sample_pattern("^BR-[0-9]{3,}$", 1).unwrap(), "BR-001");
        assert_eq!(sample_pattern("^T-[0-9]{3}$", 1).unwrap(), "T-001");
        assert!(sample_pattern("^\\d+$", 1).is_none());
    }

    #[test]
    fn examples_are_valid_and_outlines_name_every_field() {
        let s = json!({
            "type": "object", "additionalProperties": false,
            "required": ["document", "context", "decisions"],
            "properties": {
                "document": {"const": "hld"},
                "context": {"type": "object", "additionalProperties": false, "required": ["system_name"],
                            "properties": {"system_name": {"type": "string"}}},
                "decisions": {"type": "array", "minItems": 2, "items": {
                    "type": "object", "additionalProperties": false, "required": ["id", "status"],
                    "properties": {"id": {"type": "string", "pattern": "^AD-[0-9]{3}$"},
                                   "status": {"enum": ["proposed", "accepted"]},
                                   "note": {"type": ["string", "null"]}}}}
            }
        });
        let ex = example(&s).unwrap();
        assert!(
            CompiledSchema::compile(&s).unwrap().errors(&ex).is_empty(),
            "{ex}"
        );
        assert_eq!(ex["decisions"][1]["id"], "AD-002");
        assert!(ex["context"].is_object());
        let o = outline(&s).join("\n");
        assert!(
            o.contains("context: object (required); required fields: system_name; NO other fields allowed"),
            "{o}"
        );
        assert!(
            o.contains("decisions[].status: one of proposed | accepted (required)"),
            "{o}"
        );
        assert!(
            o.contains("decisions: array of objects, at least 2 (required)"),
            "{o}"
        );
    }
}
