//! Output contracts of the shipped agents (the HLD failure of 2026-11-13: an external AI
//! client could not find out the exact HLD structure and ran out of corrections).
//!
//! For every agent definition in `config/agents`:
//! * the example generated from its output schema combined with its skill contracts is
//!   valid against that schema **and**
//!   against the output contract of every skill the agent uses (so agent schema and skill
//!   contract cannot contradict each other: requirements, HLD, LLD, delivery);
//! * the outline names every required field;
//! * the labelled design fixtures (`examples/design/*.json`) are valid against the schema
//!   of the agent that produces them.

use eaeh_validation::schema::CompiledSchema;
use eaeh_validation::shape;
use serde_json::Value;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn load(p: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(root().join(p)).unwrap()).unwrap()
}

fn skill_schema(reference: &str) -> Option<Value> {
    let id = reference.split('@').next()?;
    let p = root().join(format!("config/skills/{id}.json"));
    let s: Value = serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
    s.get("output_schema").cloned()
}

fn required_paths(schema: &Value, path: &str, out: &mut Vec<String>) {
    if let Some(props) = schema.get("properties").and_then(Value::as_object) {
        for r in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            out.push(if path.is_empty() {
                r.to_string()
            } else {
                format!("{path}.{r}")
            });
        }
        for (k, s) in props {
            let child = if path.is_empty() {
                k.clone()
            } else {
                format!("{path}.{k}")
            };
            required_paths(s, &child, out);
        }
    } else if let Some(items) = schema.get("items") {
        required_paths(items, &format!("{path}[]"), out);
    }
}

#[test]
fn every_agent_has_a_valid_generated_example_that_also_meets_its_skill_contracts() {
    let mut checked = 0;
    for entry in std::fs::read_dir(root().join("config/agents")).unwrap() {
        let path = entry.unwrap().path();
        let def: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let mut combined = def["output_schema"].clone();
        for s in def["skills"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if let Some(contract) = skill_schema(s) {
                combined = shape::merge(&combined, &contract);
            }
        }
        let schema = &combined;
        let compiled = CompiledSchema::compile(&def["output_schema"]).unwrap();
        let example = shape::example(schema).unwrap_or_else(|| panic!("{name}: no example can be generated"));
        let errs = compiled.errors(&example);
        assert!(
            errs.is_empty(),
            "{name}: generated example invalid: {errs:?}\n{example:#}"
        );
        for s in def["skills"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if let Some(contract) = skill_schema(s) {
                let errs = CompiledSchema::compile(&contract).unwrap().errors(&example);
                assert!(
                    errs.is_empty(),
                    "{name}: agent output schema and skill contract {s} disagree: {errs:?}"
                );
            }
        }
        let outline = shape::outline(schema).join("\n");
        let mut req = vec![];
        required_paths(schema, "", &mut req);
        for r in req {
            let leaf = r.rsplit('.').next().unwrap();
            assert!(
                outline.contains(leaf),
                "{name}: outline does not mention required field {r}"
            );
        }
        checked += 1;
    }
    assert!(checked >= 10, "all shipped agents checked ({checked})");
}

#[test]
fn hld_contract_is_explicit_about_the_fields_that_failed() {
    let schema = load("config/agents/solution-architect.json")["output_schema"].clone();
    let outline = shape::outline(&schema).join("\n");
    // The fields the external client got wrong on 2026-11-13.
    assert!(
        outline.contains("context: object (required); required fields: system_name, description"),
        "{outline}"
    );
    assert!(outline.contains("decisions[]: object (required); required fields: id, title, status, context, decision, alternatives"), "{outline}");
    assert!(
        outline.contains("decisions[].status: one of proposed | accepted (required)"),
        "{outline}"
    );
    assert!(
        outline
            .contains("components[]: object (required); required fields: id, name, kind, responsibilities"),
        "{outline}"
    );
    assert!(
        outline.contains("compliance[]: object (required); required fields: standard, status, justification"),
        "{outline}"
    );
    assert!(outline.contains("NO other fields allowed"), "{outline}");
    let ex = shape::example(&schema).unwrap();
    assert!(ex["context"].is_object());
    assert!(
        ex["decisions"][0]["alternatives"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
    );
    assert!(ex["decisions"][0].get("rationale").is_none() && ex["decisions"][0].get("citations").is_none());
}

#[test]
fn the_failed_hld_shape_is_reported_completely_in_one_pass() {
    let schema =
        CompiledSchema::compile(&load("config/agents/solution-architect.json")["output_schema"]).unwrap();
    // The shape the external client submitted (context as a sentence; decisions with
    // citations/rationale and without status/context/alternatives), for five decisions.
    let mut bad = load("examples/design/hld.json");
    bad["context"] = Value::String("A local-only demonstration REST API.".into());
    let decisions: Vec<Value> = (1..=5)
        .map(|i| serde_json::json!({"id": format!("AD-00{i}"), "title": "t", "decision": "d", "rationale": "r", "citations": []}))
        .collect();
    bad["decisions"] = Value::Array(decisions);
    let (shown, total) = schema.errors_with_total(&bad);
    assert!(total >= 21, "{total}: {shown:?}");
    assert_eq!(shown.len(), total, "every problem is reported, not the first 10");
    let msg = schema.check(&bad, "agent output").unwrap_err().message;
    assert!(msg.contains("problem(s), all shown"), "{msg}");
    assert!(msg.contains("/context") && msg.contains("/decisions/4"), "{msg}");
}

#[test]
fn labelled_design_fixtures_match_the_producing_agents_schemas() {
    for (fixture, agent) in [
        (
            "examples/design/requirements.json",
            "config/agents/requirements-engineer.json",
        ),
        (
            "examples/design/requirements.json",
            "config/agents/business-analyst.json",
        ),
        (
            "examples/design/hld.json",
            "config/agents/solution-architect.json",
        ),
        ("examples/design/lld.json", "config/agents/software-designer.json"),
    ] {
        let schema = CompiledSchema::compile(&load(agent)["output_schema"]).unwrap();
        let errs = schema.errors(&load(fixture));
        assert!(errs.is_empty(), "{fixture} vs {agent}: {errs:?}");
    }
}
