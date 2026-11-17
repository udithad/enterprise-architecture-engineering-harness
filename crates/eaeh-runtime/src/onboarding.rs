//! Register an existing project by location (PORTAL-OPERATIONS-PLAN, W3): confined
//! directory browsing and read-only detection of stack, Git repository and documentation.
//!
//! Nothing here writes. Registration itself is `eaeh project init`'s service (W4).
//!
//! Path rules: every path is canonicalized (resolving `..` and symlinks) and must stay inside
//! one of the allowed roots; hidden entries are skipped; an entry whose canonical path leaves
//! the roots (a symlink pointing outside) is not listed and cannot be selected.

use eaeh_core::{HarnessError, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Canonical allowed roots (missing roots are dropped).
pub fn canonical_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = roots
        .iter()
        .filter_map(|r| std::fs::canonicalize(r).ok())
        .filter(|r| r.is_dir())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Resolve `path` (absolute, or relative to the first root) inside the allowed roots.
pub fn confine(roots: &[PathBuf], path: &str) -> Result<PathBuf> {
    if roots.is_empty() {
        return Err(HarnessError::forbidden(
            "no workspace roots are configured for this portal (--workspace-root)",
        ));
    }
    let p = path.trim();
    if p.is_empty() || p.contains('\0') {
        return Err(HarnessError::invalid_input("a directory path is required"));
    }
    let raw = Path::new(p);
    let joined = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        roots[0].join(raw)
    };
    let canon = std::fs::canonicalize(&joined).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            HarnessError::not_found(format!("directory not found: {}", joined.display()))
        } else {
            HarnessError::invalid_input(format!("directory not accessible: {} ({e})", joined.display()))
        }
    })?;
    if !roots.iter().any(|r| canon.starts_with(r)) {
        return Err(HarnessError::forbidden(format!(
            "{} is outside the allowed workspace roots ({}); restart the portal with --workspace-root <dir> to allow it",
            canon.display(),
            roots
                .iter()
                .map(|r| r.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if !canon.is_dir() {
        return Err(HarnessError::invalid_input(format!(
            "{} is not a directory",
            canon.display()
        )));
    }
    std::fs::read_dir(&canon).map_err(|e| {
        HarnessError::invalid_input(format!("directory not readable: {} ({e})", canon.display()))
    })?;
    Ok(canon)
}

#[derive(Debug, Clone, Serialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    /// The folder looks like a project (build file, `.git` or an EAEH manifest).
    pub project_hint: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirListing {
    pub path: String,
    /// The parent, when it is still inside a root.
    pub parent: Option<String>,
    pub roots: Vec<String>,
    pub entries: Vec<DirEntry>,
    pub truncated: bool,
}

const BUILD_FILES: &[&str] = &[
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "package.json",
    "pyproject.toml",
    "requirements.txt",
    "go.mod",
    "Cargo.toml",
];
const MAX_ENTRIES: usize = 500;

fn looks_like_project(dir: &Path) -> bool {
    dir.join(".git").exists()
        || dir.join(".eaeh/project.toml").is_file()
        || BUILD_FILES.iter().any(|f| dir.join(f).is_file())
}

/// Sub-directories of `path` (or the roots themselves when `path` is `None`).
pub fn list_dir(roots: &[PathBuf], path: Option<&str>) -> Result<DirListing> {
    let roots_s = roots.iter().map(|r| r.display().to_string()).collect::<Vec<_>>();
    let Some(path) = path.filter(|p| !p.trim().is_empty()) else {
        return Ok(DirListing {
            path: String::new(),
            parent: None,
            entries: roots
                .iter()
                .map(|r| DirEntry {
                    name: r.display().to_string(),
                    path: r.display().to_string(),
                    project_hint: looks_like_project(r),
                })
                .collect(),
            roots: roots_s,
            truncated: false,
        });
    };
    let dir = confine(roots, path)?;
    let mut entries = vec![];
    let mut truncated = false;
    let rd = std::fs::read_dir(&dir)
        .map_err(|e| HarnessError::invalid_input(format!("{}: {e}", dir.display())))?;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let Ok(c) = std::fs::canonicalize(e.path()) else {
            continue;
        };
        if !c.is_dir() || !roots.iter().any(|r| c.starts_with(r)) {
            continue;
        }
        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }
        entries.push(DirEntry {
            name,
            project_hint: looks_like_project(&c),
            path: c.display().to_string(),
        });
    }
    entries.sort_by_key(|a| a.name.to_lowercase());
    let parent = dir
        .parent()
        .filter(|p| roots.iter().any(|r| p.starts_with(r)))
        .map(|p| p.display().to_string());
    Ok(DirListing {
        path: dir.display().to_string(),
        parent,
        roots: roots_s,
        entries,
        truncated,
    })
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct GitInfo {
    pub present: bool,
    pub remote: Option<String>,
    pub head: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Documents {
    pub brd_candidates: Vec<String>,
    pub readme: Option<String>,
    pub architecture: Vec<String>,
    pub adrs: Vec<String>,
    pub standards: Vec<String>,
    pub knowledge: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ExistingRegistration {
    pub project_id: String,
    pub name: String,
}

/// What was detected in a folder (read-only).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Detection {
    pub path: String,
    pub suggested_name: String,
    pub description: String,
    pub language: String,
    pub framework: String,
    pub build_tool: String,
    pub build_files: Vec<String>,
    pub source_dirs: Vec<String>,
    pub git: GitInfo,
    pub documents: Documents,
    /// An EAEH manifest already present in the folder.
    pub manifest: Option<ExistingRegistration>,
    pub manifest_issue: Option<String>,
    pub warnings: Vec<String>,
    /// The folder looks like a project at all.
    pub recognised: bool,
}

fn read_small(p: &Path) -> String {
    match std::fs::metadata(p) {
        Ok(m) if m.len() <= 512 * 1024 => std::fs::read_to_string(p).unwrap_or_default(),
        _ => String::new(),
    }
}

fn stack(root: &Path) -> (String, String, String, Vec<String>) {
    let has = |f: &str| root.join(f).is_file();
    let build_files: Vec<String> = BUILD_FILES
        .iter()
        .filter(|f| has(f))
        .map(|f| f.to_string())
        .collect();
    if has("pom.xml") || has("build.gradle") || has("build.gradle.kts") {
        let (tool, text) = if has("pom.xml") {
            ("Maven", read_small(&root.join("pom.xml")))
        } else if has("build.gradle.kts") {
            ("Gradle", read_small(&root.join("build.gradle.kts")))
        } else {
            ("Gradle", read_small(&root.join("build.gradle")))
        };
        let kotlin = text.contains("kotlin") && root.join("src/main/kotlin").is_dir();
        let lang = if kotlin { "Kotlin" } else { "Java" };
        let fw = if text.contains("spring-boot") || text.contains("org.springframework.boot") {
            "Spring Boot"
        } else if text.contains("quarkus") {
            "Quarkus"
        } else if text.contains("micronaut") {
            "Micronaut"
        } else {
            ""
        };
        return (lang.into(), fw.into(), tool.into(), build_files);
    }
    if has("package.json") {
        let text = read_small(&root.join("package.json"));
        let lang = if has("tsconfig.json") || text.contains("\"typescript\"") {
            "TypeScript"
        } else {
            "JavaScript"
        };
        let fw = [
            ("\"next\"", "Next.js"),
            ("\"@nestjs/core\"", "NestJS"),
            ("\"react\"", "React"),
            ("\"vue\"", "Vue"),
            ("\"express\"", "Express"),
        ]
        .iter()
        .find(|(k, _)| text.contains(k))
        .map(|(_, v)| *v)
        .unwrap_or("");
        let tool = if has("pnpm-lock.yaml") {
            "pnpm"
        } else if has("yarn.lock") {
            "Yarn"
        } else {
            "npm"
        };
        return (lang.into(), fw.into(), tool.into(), build_files);
    }
    if has("pyproject.toml") || has("requirements.txt") {
        let text = format!(
            "{}{}",
            read_small(&root.join("pyproject.toml")),
            read_small(&root.join("requirements.txt"))
        )
        .to_lowercase();
        let fw = [("fastapi", "FastAPI"), ("django", "Django"), ("flask", "Flask")]
            .iter()
            .find(|(k, _)| text.contains(k))
            .map(|(_, v)| *v)
            .unwrap_or("");
        let tool = if text.contains("[tool.poetry]") {
            "Poetry"
        } else if has("pyproject.toml") {
            "pip (pyproject)"
        } else {
            "pip"
        };
        return ("Python".into(), fw.into(), tool.into(), build_files);
    }
    if has("go.mod") {
        return ("Go".into(), String::new(), "Go modules".into(), build_files);
    }
    if has("Cargo.toml") {
        return ("Rust".into(), String::new(), "Cargo".into(), build_files);
    }
    (String::new(), String::new(), String::new(), build_files)
}

/// Markdown and text documents up to `depth` levels below `dir`, as paths relative to `root`.
fn documents_under(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
    if depth == 0 || out.len() >= 300 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.')
            || ["node_modules", "target", "build", "dist", "vendor", "out"].contains(&name.as_str())
        {
            continue;
        }
        let p = e.path();
        // Never follow links out of the project.
        let Ok(c) = std::fs::canonicalize(&p) else {
            continue;
        };
        if !c.starts_with(root) {
            continue;
        }
        if c.is_dir() {
            documents_under(root, &c, depth - 1, out);
        } else if matches!(
            c.extension().and_then(|x| x.to_str()),
            Some("md" | "markdown" | "txt" | "adoc")
        ) && let Ok(rel) = c.strip_prefix(root)
        {
            out.push(rel.display().to_string());
        }
    }
}

fn first_paragraph(md: &str) -> String {
    md.lines()
        .map(str::trim)
        .skip_while(|l| {
            l.is_empty()
                || l.starts_with('#')
                || l.starts_with('[')
                || l.starts_with('!')
                || l.starts_with('<')
        })
        .take_while(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(400)
        .collect()
}

/// Detect a project's structure and documentation (read-only).
pub fn detect(root: &Path) -> Detection {
    let mut d = Detection {
        path: root.display().to_string(),
        suggested_name: root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        ..Default::default()
    };
    let (language, framework, build_tool, build_files) = stack(root);
    d.language = language;
    d.framework = framework;
    d.build_tool = build_tool;
    d.build_files = build_files;
    for s in [
        "src/main/java",
        "src/main/kotlin",
        "src/test/java",
        "src",
        "app",
        "lib",
        "cmd",
        "internal",
    ] {
        if root.join(s).is_dir() {
            d.source_dirs.push(s.into());
        }
    }
    let (remote, head) = crate::project::git_info(root);
    d.git = GitInfo {
        present: root.join(".git").exists(),
        remote,
        head,
    };
    let mut docs = vec![];
    documents_under(root, root, 1, &mut docs);
    for sub in [
        "docs",
        "doc",
        "documentation",
        "requirements",
        "architecture",
        "adr",
        "standards",
    ] {
        let p = root.join(sub);
        if p.is_dir() {
            documents_under(root, &p, 4, &mut docs);
        }
    }
    docs.sort();
    docs.dedup();
    for f in &docs {
        let l = f.to_lowercase();
        let file = l.rsplit('/').next().unwrap_or(&l).to_string();
        if file.starts_with("readme") && !f.contains('/') {
            d.documents.readme = Some(f.clone());
        } else if file.contains("brd")
            || file.contains("business-requirement")
            || file.contains("business_requirement")
            || (l.starts_with("requirements/") && !file.starts_with("readme"))
        {
            d.documents.brd_candidates.push(f.clone());
        } else if l.contains("/adr/")
            || l.contains("/adrs/")
            || l.starts_with("adr/")
            || file.starts_with("adr-")
            || file.starts_with("adr_")
        {
            d.documents.adrs.push(f.clone());
        } else if l.contains("architecture")
            || file.contains("hld")
            || file.contains("lld")
            || file.contains("design")
        {
            d.documents.architecture.push(f.clone());
        } else if file.contains("standard")
            || file.contains("guideline")
            || file.contains("coding")
            || file.contains("style")
            || file.starts_with("contributing")
            || l.starts_with("standards/")
        {
            d.documents.standards.push(f.clone());
        } else if !file.starts_with("changelog") && !file.starts_with("license") && file != "claude.md" {
            d.documents.knowledge.push(f.clone());
        }
    }
    if let Some(r) = &d.documents.readme {
        d.description = first_paragraph(&read_small(&root.join(r)));
    }
    match crate::project::ProjectManifest::load(root) {
        Ok(m) => {
            d.manifest = Some(ExistingRegistration {
                project_id: m.project.id.to_string(),
                name: m.project.name.clone(),
            });
            d.suggested_name = m.project.name.clone();
            if !m.project.description.is_empty() {
                d.description = m.project.description.clone();
            }
        }
        Err(_) if root.join(".eaeh/project.toml").is_file() => {
            d.manifest_issue = Some(".eaeh/project.toml exists but is not a valid EAEH manifest".into());
        }
        Err(_) => {}
    }
    d.recognised = !d.build_files.is_empty() || d.git.present || d.manifest.is_some();
    if !d.recognised {
        d.warnings.push(
            "no build file, Git repository or EAEH manifest found: this does not look like a project folder"
                .into(),
        );
    }
    if d.documents.brd_candidates.is_empty() {
        d.warnings
            .push("no BRD found: choose one before starting a design workflow (Markdown or text)".into());
    }
    if root.join("crates/eaeh-cli/Cargo.toml").is_file() {
        d.warnings
            .push("this is the EAEH repository itself; register your application's folder instead".into());
        d.recognised = false;
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confinement_refuses_paths_and_links_outside_the_roots() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("ws");
        let outside = base.path().join("outside");
        std::fs::create_dir_all(root.join("app/.git")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
        let roots = canonical_roots(std::slice::from_ref(&root));
        assert!(confine(&roots, "app").is_ok());
        assert_eq!(
            confine(&roots, "../outside").unwrap_err().kind,
            eaeh_core::ErrorKind::Forbidden
        );
        assert_eq!(
            confine(&roots, outside.to_str().unwrap()).unwrap_err().kind,
            eaeh_core::ErrorKind::Forbidden
        );
        assert_eq!(
            confine(&roots, "missing").unwrap_err().kind,
            eaeh_core::ErrorKind::NotFound
        );
        #[cfg(unix)]
        assert_eq!(
            confine(&roots, "escape").unwrap_err().kind,
            eaeh_core::ErrorKind::Forbidden
        );
        let l = list_dir(&roots, Some(roots[0].to_str().unwrap())).unwrap();
        let names: Vec<_> = l.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["app"], "hidden and escaping entries are not listed");
        assert!(l.entries[0].project_hint);
        assert!(l.parent.is_none(), "no parent above the root");
        assert_eq!(
            confine(&[], "app").unwrap_err().kind,
            eaeh_core::ErrorKind::Forbidden
        );
    }

    #[test]
    fn detects_a_spring_boot_project_and_its_documents() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        std::fs::write(
            r.join("pom.xml"),
            "<project><parent><artifactId>spring-boot-starter-parent</artifactId></parent></project>",
        )
        .unwrap();
        std::fs::create_dir_all(r.join("src/main/java")).unwrap();
        std::fs::create_dir_all(r.join(".git")).unwrap();
        std::fs::write(r.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(r.join("README.md"), "# Orders\n\nOrder management API.\n").unwrap();
        std::fs::create_dir_all(r.join("docs/adr")).unwrap();
        std::fs::write(r.join("docs/BRD.md"), "# BRD").unwrap();
        std::fs::write(r.join("docs/adr/0001-db.md"), "# ADR").unwrap();
        std::fs::write(r.join("docs/coding-standards.md"), "# Std").unwrap();
        std::fs::write(r.join("docs/architecture-overview.md"), "# Arch").unwrap();
        std::fs::write(r.join("docs/glossary.md"), "# Glossary").unwrap();
        let root = std::fs::canonicalize(r).unwrap();
        let d = detect(&root);
        assert_eq!(
            (d.language.as_str(), d.framework.as_str(), d.build_tool.as_str()),
            ("Java", "Spring Boot", "Maven")
        );
        assert!(d.git.present && d.git.head.as_deref() == Some("refs/heads/main"));
        assert_eq!(d.documents.brd_candidates, vec!["docs/BRD.md"]);
        assert_eq!(d.documents.adrs, vec!["docs/adr/0001-db.md"]);
        assert_eq!(d.documents.standards, vec!["docs/coding-standards.md"]);
        assert_eq!(d.documents.architecture, vec!["docs/architecture-overview.md"]);
        assert_eq!(d.documents.knowledge, vec!["docs/glossary.md"]);
        assert_eq!(d.documents.readme.as_deref(), Some("README.md"));
        assert_eq!(d.description, "Order management API.");
        assert!(d.recognised && d.manifest.is_none());
    }
}
