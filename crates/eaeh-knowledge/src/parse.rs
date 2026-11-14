//! Parsing and chunking. Deterministic: the same content always yields the same chunks.

use crate::model::Format;
use eaeh_core::{HarnessError, Result};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawChunk {
    pub heading: String,
    pub text: String,
}

/// Split content into chunks of at most `max_chars` characters.
pub fn chunk(format: Format, content: &str, max_chars: usize) -> Result<Vec<RawChunk>> {
    let chunks = match format {
        Format::Markdown => markdown(content, max_chars),
        Format::Text => text(content, max_chars, ""),
        Format::Json | Format::Openapi => {
            let v: Value = serde_json::from_str(content)
                .map_err(|e| HarnessError::invalid_input(format!("invalid JSON document: {e}")))?;
            if format == Format::Openapi || v.get("openapi").is_some() {
                openapi(&v, max_chars)?
            } else {
                json(&v, max_chars)
            }
        }
    };
    let chunks: Vec<RawChunk> = chunks.into_iter().filter(|c| !c.text.trim().is_empty()).collect();
    if chunks.is_empty() {
        return Err(HarnessError::invalid_input("document produced no content"));
    }
    Ok(chunks)
}

fn split_long(heading: &str, body: &str, max_chars: usize, out: &mut Vec<RawChunk>) {
    let mut current = String::new();
    for para in body.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        if !current.is_empty() && current.chars().count() + para.chars().count() + 2 > max_chars {
            out.push(RawChunk {
                heading: heading.into(),
                text: std::mem::take(&mut current),
            });
        }
        if para.chars().count() > max_chars {
            // Hard split oversized paragraphs on char boundaries.
            let chars: Vec<char> = para.chars().collect();
            for piece in chars.chunks(max_chars) {
                out.push(RawChunk {
                    heading: heading.into(),
                    text: piece.iter().collect(),
                });
            }
            continue;
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(para);
    }
    if !current.trim().is_empty() {
        out.push(RawChunk {
            heading: heading.into(),
            text: current,
        });
    }
}

fn markdown(content: &str, max_chars: usize) -> Vec<RawChunk> {
    let mut out = vec![];
    let mut path: Vec<(usize, String)> = vec![];
    let mut body = String::new();
    let flush = |path: &Vec<(usize, String)>, body: &mut String, out: &mut Vec<RawChunk>| {
        let heading = path
            .iter()
            .map(|(_, h)| h.as_str())
            .collect::<Vec<_>>()
            .join(" > ");
        split_long(&heading, body, max_chars, out);
        body.clear();
    };
    let mut in_code = false;
    for line in content.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
        }
        let level = line.chars().take_while(|c| *c == '#').count();
        if !in_code && (1..=6).contains(&level) && line.chars().nth(level) == Some(' ') {
            flush(&path, &mut body, &mut out);
            let title = line[level..].trim().to_string();
            path.retain(|(l, _)| *l < level);
            path.push((level, title));
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    flush(&path, &mut body, &mut out);
    out
}

fn text(content: &str, max_chars: usize, heading: &str) -> Vec<RawChunk> {
    let mut out = vec![];
    split_long(heading, content, max_chars, &mut out);
    out
}

fn compact(v: &Value, max_chars: usize) -> String {
    let s = serde_json::to_string(v).unwrap_or_default();
    s.chars().take(max_chars).collect()
}

fn json(v: &Value, max_chars: usize) -> Vec<RawChunk> {
    match v {
        Value::Object(m) => m
            .iter()
            .map(|(k, val)| RawChunk {
                heading: k.clone(),
                text: compact(val, max_chars),
            })
            .collect(),
        other => vec![RawChunk {
            heading: String::new(),
            text: compact(other, max_chars),
        }],
    }
}

fn openapi(v: &Value, max_chars: usize) -> Result<Vec<RawChunk>> {
    let mut out = vec![];
    if let Some(info) = v.get("info") {
        let title = info.get("title").and_then(Value::as_str).unwrap_or("");
        let desc = info.get("description").and_then(Value::as_str).unwrap_or("");
        out.push(RawChunk {
            heading: "info".into(),
            text: format!("{title}\n{desc}").trim().to_string(),
        });
    }
    let paths = v
        .get("paths")
        .and_then(Value::as_object)
        .ok_or_else(|| HarnessError::invalid_input("OpenAPI document has no 'paths' object"))?;
    for (path, item) in paths {
        let Some(ops) = item.as_object() else { continue };
        for (method, op) in ops {
            if !matches!(
                method.as_str(),
                "get" | "put" | "post" | "delete" | "patch" | "head" | "options"
            ) {
                continue;
            }
            let mut text = String::new();
            for key in ["operationId", "summary", "description"] {
                if let Some(s) = op.get(key).and_then(Value::as_str) {
                    text.push_str(&format!("{key}: {s}\n"));
                }
            }
            if let Some(params) = op.get("parameters").and_then(Value::as_array) {
                let names: Vec<&str> = params
                    .iter()
                    .filter_map(|p| p.get("name").and_then(Value::as_str))
                    .collect();
                text.push_str(&format!("parameters: {}\n", names.join(", ")));
            }
            if let Some(resp) = op.get("responses").and_then(Value::as_object) {
                for (code, r) in resp {
                    text.push_str(&format!("response {code}: {}\n", compact(r, 400)));
                }
            }
            out.push(RawChunk {
                heading: format!("{} {path}", method.to_uppercase()),
                text: text.chars().take(max_chars).collect(),
            });
        }
    }
    if let Some(schemas) = v.pointer("/components/schemas").and_then(Value::as_object) {
        for (name, schema) in schemas {
            out.push(RawChunk {
                heading: format!("schema {name}"),
                text: compact(schema, max_chars),
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_chunks_follow_heading_paths_and_skip_code_headings() {
        let md = "# Errors\nIntro.\n\n## Format\nUse problem+json.\n```\n# not a heading\n```\n## Codes\nUse 4xx for client errors.\n";
        let c = chunk(Format::Markdown, md, 1000).unwrap();
        let headings: Vec<&str> = c.iter().map(|c| c.heading.as_str()).collect();
        assert_eq!(headings, vec!["Errors", "Errors > Format", "Errors > Codes"]);
        assert!(c[1].text.contains("# not a heading"));
    }

    #[test]
    fn long_sections_split_and_deterministic() {
        let body = (0..50)
            .map(|i| format!("Paragraph {i} with some words."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let a = chunk(Format::Text, &body, 200).unwrap();
        assert!(a.len() > 5);
        assert!(a.iter().all(|c| c.text.chars().count() <= 200));
        assert_eq!(a, chunk(Format::Text, &body, 200).unwrap());
    }

    #[test]
    fn openapi_operations_and_schemas() {
        let spec = r#"{"openapi":"3.0.0","info":{"title":"Orders"},"paths":{"/orders":{"get":{"operationId":"listOrders","responses":{"200":{"description":"ok"}}},"post":{"summary":"create"}}},"components":{"schemas":{"Order":{"type":"object"}}}}"#;
        let c = chunk(Format::Json, spec, 1000).unwrap();
        let headings: Vec<&str> = c.iter().map(|c| c.heading.as_str()).collect();
        assert_eq!(
            headings,
            vec!["info", "GET /orders", "POST /orders", "schema Order"]
        );
        assert!(chunk(Format::Openapi, r#"{"openapi":"3.0.0"}"#, 100).is_err());
        assert!(chunk(Format::Json, "{not json", 100).is_err());
        assert!(chunk(Format::Markdown, "   \n", 100).is_err());
    }
}
