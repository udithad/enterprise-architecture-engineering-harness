//! Deterministic Mermaid diagram generation from the typed models.
//!
//! Model text is untrusted: identifiers are reduced to `[A-Za-z0-9_]` and labels are
//! entity-encoded (`#`, `"`, `<`, `>`, `;`, `|`, backtick, newlines), so model content can
//! never break out of a label to add directives (`%%{init}%%`), `click` handlers, links or
//! new statements.

use crate::model::*;
use std::collections::BTreeSet;
use std::fmt::Write;

/// Safe node identifier.
pub fn ident(prefix: &str, s: &str) -> String {
    let body: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{prefix}_{body}")
}

/// Safe label text (for use inside double quotes or after `:` / `as`).
pub fn label(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '#' => o.push_str("#35;"),
            '"' => o.push_str("#quot;"),
            '<' => o.push_str("#lt;"),
            '>' => o.push_str("#gt;"),
            ';' => o.push_str("#59;"),
            '|' => o.push_str("#124;"),
            '`' => o.push_str("#96;"),
            '%' => o.push_str("#37;"),
            '{' => o.push_str("#123;"),
            '}' => o.push_str("#125;"),
            '\r' | '\n' | '\t' => o.push(' '),
            c if c.is_control() => {}
            c => o.push(c),
        }
    }
    o.trim().to_string()
}

fn node_shape(id: &str, kind: ComponentKind, text: &str) -> String {
    let t = label(text);
    match kind {
        ComponentKind::Database | ComponentKind::Storage => format!("{id}[(\"{t}\")]"),
        ComponentKind::Queue => format!("{id}>\"{t}\"]"),
        ComponentKind::External => format!("{id}{{{{\"{t}\"}}}}"),
        ComponentKind::Cache => format!("{id}[/\"{t}\"/]"),
        _ => format!("{id}[\"{t}\"]"),
    }
}

/// System context: actors around the system.
pub fn context(h: &Hld) -> String {
    let mut o = String::from("flowchart LR\n");
    let sys = "sys_system";
    let _ = writeln!(o, "  {sys}[[\"{}\"]]", label(&h.context.system_name));
    let comps: BTreeSet<&str> = h.components.iter().map(|c| c.id.as_str()).collect();
    for a in &h.context.actors {
        let kind = match a.kind {
            ActorKind::Person => "person",
            ActorKind::ExternalSystem => "external system",
        };
        let _ = writeln!(o, "  {}([\"{} ({kind})\"])", ident("a", &a.id), label(&a.name));
    }
    let mut edges = BTreeSet::new();
    for f in &h.data_flows {
        let from_actor = h.context.actors.iter().any(|a| a.id == f.from);
        let to_actor = h.context.actors.iter().any(|a| a.id == f.to);
        if from_actor && comps.contains(f.to.as_str()) {
            edges.insert((ident("a", &f.from), sys.to_string(), label(&f.data)));
        } else if to_actor && comps.contains(f.from.as_str()) {
            edges.insert((sys.to_string(), ident("a", &f.to), label(&f.data)));
        }
    }
    for i in &h.interfaces {
        for c in &i.consumers {
            if h.context.actors.iter().any(|a| &a.id == c) {
                edges.insert((ident("a", c), sys.to_string(), label(&i.protocol)));
            }
        }
    }
    if edges.is_empty() {
        for a in &h.context.actors {
            edges.insert((ident("a", &a.id), sys.to_string(), String::new()));
        }
    }
    for (f, t, l) in edges {
        if l.is_empty() {
            let _ = writeln!(o, "  {f} --> {t}");
        } else {
            let _ = writeln!(o, "  {f} -->|\"{l}\"| {t}");
        }
    }
    o
}

/// Components, interfaces (solid) and data flows (dashed).
pub fn components(h: &Hld) -> String {
    let mut o = String::from("flowchart TB\n");
    for a in &h.context.actors {
        let _ = writeln!(o, "  {}([\"{}\"])", ident("a", &a.id), label(&a.name));
    }
    let _ = writeln!(
        o,
        "  subgraph sys_boundary[\"{}\"]",
        label(&h.context.system_name)
    );
    for c in h.components.iter().filter(|c| c.kind != ComponentKind::External) {
        let _ = writeln!(
            o,
            "    {}",
            node_shape(
                &ident("c", &c.id),
                c.kind,
                &format!("{} ({})", c.name, c.kind.label())
            )
        );
    }
    o.push_str("  end\n");
    for c in h.components.iter().filter(|c| c.kind == ComponentKind::External) {
        let _ = writeln!(o, "  {}", node_shape(&ident("c", &c.id), c.kind, &c.name));
    }
    let node = |id: &str| {
        if h.context.actors.iter().any(|a| a.id == id) {
            ident("a", id)
        } else {
            ident("c", id)
        }
    };
    for i in &h.interfaces {
        for c in &i.consumers {
            let _ = writeln!(
                o,
                "  {} -->|\"{}\"| {}",
                node(c),
                label(&i.protocol),
                ident("c", &i.provider)
            );
        }
    }
    for f in &h.data_flows {
        let _ = writeln!(
            o,
            "  {} -.->|\"{}\"| {}",
            node(&f.from),
            label(&f.data),
            node(&f.to)
        );
    }
    o
}

/// Deployment nodes with hosted components (node-qualified ids keep them unique).
pub fn deployment(nodes: &[DeploymentNode], name_of: &dyn Fn(&str) -> String) -> String {
    let mut o = String::from("flowchart TB\n");
    for n in nodes {
        let tech = if n.technology.trim().is_empty() {
            String::new()
        } else {
            format!(" – {}", n.technology)
        };
        let env = if n.environment.trim().is_empty() {
            String::new()
        } else {
            format!(" [{}]", n.environment)
        };
        let _ = writeln!(
            o,
            "  subgraph {}[\"{}\"]",
            ident("n", &n.id),
            label(&format!("{}{tech}{env}", n.name))
        );
        for h in &n.hosts {
            let _ = writeln!(
                o,
                "    {}[\"{}\"]",
                ident("d", &format!("{}__{}", n.id, h)),
                label(&name_of(h))
            );
        }
        o.push_str("  end\n");
    }
    o
}

/// Sequence diagram.
pub fn sequence(q: &Sequence, name_of: &dyn Fn(&str) -> String) -> String {
    let mut o = String::from("sequenceDiagram\n");
    let mut seen = vec![];
    for st in &q.steps {
        for p in [&st.from, &st.to] {
            if !seen.contains(p) {
                seen.push(p.clone());
                let _ = writeln!(o, "  participant {} as {}", ident("p", p), label(&name_of(p)));
            }
        }
    }
    for st in &q.steps {
        let op = st
            .operation
            .as_deref()
            .map(|x| format!(" [{x}]"))
            .unwrap_or_default();
        let _ = writeln!(
            o,
            "  {}->>{}: {}",
            ident("p", &st.from),
            ident("p", &st.to),
            label(&format!("{}{op}", st.message))
        );
    }
    o
}

/// Entity–relationship diagram.
pub fn er(entities: &[Entity]) -> String {
    let mut o = String::from("erDiagram\n");
    for e in entities {
        let _ = writeln!(o, "  {} {{", ident("E", &e.name));
        for a in &e.attributes {
            let _ = writeln!(
                o,
                "    {} {}{}",
                ident("t", &a.kind).trim_start_matches("t_"),
                ident("f", &a.name).trim_start_matches("f_"),
                if a.required { " \"required\"" } else { "" }
            );
        }
        o.push_str("  }\n");
    }
    for e in entities {
        for r in &e.relationships {
            let card = match r.cardinality {
                Cardinality::OneToOne => "||--||",
                Cardinality::OneToMany => "||--o{",
                Cardinality::ManyToOne => "}o--||",
                Cardinality::ManyToMany => "}o--o{",
            };
            let text = if r.description.trim().is_empty() {
                "relates".to_string()
            } else {
                label(&r.description)
            };
            let _ = writeln!(
                o,
                "  {} {card} {} : \"{text}\"",
                ident("E", &e.name),
                ident("E", &r.target)
            );
        }
    }
    o
}

/// Internal structure of designed components (modules).
pub fn modules(l: &Lld, name_of: &dyn Fn(&str) -> String) -> String {
    let mut o = String::from("flowchart TB\n");
    for c in &l.components {
        let _ = writeln!(
            o,
            "  subgraph {}[\"{}\"]",
            ident("c", &c.component),
            label(&name_of(&c.component))
        );
        for (i, m) in c.modules.iter().enumerate() {
            let _ = writeln!(
                o,
                "    {}[\"{}\"]",
                ident("m", &format!("{}__{i}", c.component)),
                label(&m.name)
            );
        }
        o.push_str("  end\n");
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_identifiers_cannot_break_out() {
        let evil = "x\"]\n%%{init: {'theme':'dark'}}%%\nclick c_a href \"javascript:alert(1)\" ; A-->B | <b>";
        let l = label(evil);
        // `;` may only appear as the terminator of an entity code such as `#quot;`.
        let mut stripped = l.clone();
        for code in [
            "#35;", "#quot;", "#lt;", "#gt;", "#59;", "#124;", "#96;", "#37;", "#123;", "#125;",
        ] {
            stripped = stripped.replace(code, "");
        }
        assert!(
            !stripped.contains('"') && !stripped.contains('\n') && !stripped.contains(';'),
            "{l}"
        );
        assert!(
            !stripped.contains('<')
                && !stripped.contains('%')
                && !stripped.contains('|')
                && !stripped.contains('{'),
            "{l}"
        );
        assert!(!stripped.contains('#'), "{l}");
        assert_eq!(ident("c", "a\"]; click"), "c_a____click");
        assert_eq!(label("a # b"), "a #35; b");
    }
}
