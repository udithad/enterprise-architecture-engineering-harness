//! Skill directories (SKILL-ONBOARDING-PLAN, O1/O2/O4).
//!
//! `config/skills/<name>/SKILL.md` (TOML front matter + instructions body, as in ADR-0028)
//! plus any nested files and folders. Every file is captured – relative path, SHA-256,
//! size, kind and (for text) its content – into a [`SkillPackage`] that becomes part of the
//! skill definition, so the approval digest covers **every file**. The package's
//! `tree_hash` identifies the directory content; a change creates a new skill version.

use eaeh_core::hash::sha256_hex;
use eaeh_core::{HarnessError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

pub const MANIFEST: &str = "SKILL.md";
pub const MAX_PACKAGE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_FILE_BYTES: u64 = 512 * 1024;
pub const MAX_FILES: usize = 500;
/// Reference text an agent receives from one skill (O4); further files are listed.
pub const REFERENCE_BUDGET: usize = 64 * 1024;

/// One file of a skill directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillFile {
    /// Path relative to the skill directory, `/`-separated.
    pub path: String,
    pub sha256: String,
    pub size: u64,
    /// `markdown`, `json`, `yaml`, `toml`, `text`, `code` or `binary`.
    pub kind: String,
    /// UTF-8 content; `None` for binary files (hash and size only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

impl SkillFile {
    /// Text reference material an agent may receive (never code or binaries).
    pub fn is_reference_text(&self) -> bool {
        self.path != MANIFEST && matches!(self.kind.as_str(), "markdown" | "json" | "yaml" | "toml" | "text")
    }
}

/// The captured skill directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPackage {
    /// Directory name under the skills folder.
    pub dir: String,
    /// Version written in `SKILL.md`; the registered version may be a later patch (O2).
    pub source_version: semver::Version,
    /// 1 for the manifest version itself, n for the n-th registered content of it.
    pub revision: u32,
    pub tree_hash: String,
    pub files: Vec<SkillFile>,
}

pub fn file_kind(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    match ext.as_str() {
        "md" | "markdown" => "markdown",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "txt" | "csv" | "tsv" | "rst" | "adoc" => "text",
        "rs" | "py" | "js" | "mjs" | "ts" | "tsx" | "jsx" | "java" | "kt" | "go" | "rb" | "sh" | "bash"
        | "zsh" | "ps1" | "sql" | "c" | "h" | "cpp" | "hpp" | "cs" | "php" | "swift" | "scala" | "html"
        | "css" | "xml" | "gradle" | "properties" | "mmd" | "dockerfile" => "code",
        _ => "text",
    }
}

/// Tree hash: SHA-256 over `path \0 sha256 \0 size \n` of every file, sorted by path.
pub fn tree_hash(files: &[SkillFile]) -> String {
    let mut v: Vec<&SkillFile> = files.iter().collect();
    v.sort_by(|a, b| a.path.cmp(&b.path));
    let mut s = String::new();
    for f in v {
        s.push_str(&format!("{}\0{}\0{}\n", f.path, f.sha256, f.size));
    }
    sha256_hex(s.as_bytes())
}

/// Read every file of a skill directory. Symlinks, special files and paths that leave the
/// directory are refused; size and count limits apply. Hidden entries (`.git`, `.DS_Store`)
/// are skipped.
pub fn read_dir_files(root: &Path) -> Result<Vec<SkillFile>> {
    let bad = |m: String| HarnessError::invalid_input(format!("skill directory {}: {m}", root.display()));
    let mut out = vec![];
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&dir)?.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let p = e.path();
            let m = std::fs::symlink_metadata(&p)?;
            if m.file_type().is_symlink() {
                return Err(bad(format!("symlink '{}' is not allowed", p.display())));
            }
            if m.is_dir() {
                stack.push(p);
                continue;
            }
            if !m.is_file() {
                return Err(bad(format!("'{}' is not a regular file", p.display())));
            }
            let rel = p
                .strip_prefix(root)
                .map_err(|_| bad("path outside the skill directory".into()))?;
            if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
                return Err(bad(format!("unsafe path '{}'", rel.display())));
            }
            let rel = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("/");
            if m.len() > MAX_FILE_BYTES {
                return Err(bad(format!("{rel} is larger than {MAX_FILE_BYTES} bytes")));
            }
            total += m.len();
            if total > MAX_PACKAGE_BYTES {
                return Err(bad(format!(
                    "the directory is larger than {MAX_PACKAGE_BYTES} bytes"
                )));
            }
            let bytes = std::fs::read(&p)?;
            let content = String::from_utf8(bytes.clone())
                .ok()
                .filter(|t| !t.contains('\0'));
            let kind = if content.is_some() {
                file_kind(&rel)
            } else {
                "binary"
            };
            out.push(SkillFile {
                path: rel,
                sha256: sha256_hex(&bytes),
                size: m.len(),
                kind: kind.into(),
                content,
            });
            if out.len() > MAX_FILES {
                return Err(bad(format!("more than {MAX_FILES} files")));
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Structural checks of a captured package (also applied to stored definitions).
pub fn validate(p: &SkillPackage) -> Result<()> {
    let bad = |m: String| HarnessError::invalid_input(format!("skill package {}: {m}", p.dir));
    if !p.files.iter().any(|f| f.path == MANIFEST) {
        return Err(bad(format!("{MANIFEST} is missing")));
    }
    if p.files.len() > MAX_FILES || p.files.iter().map(|f| f.size).sum::<u64>() > MAX_PACKAGE_BYTES {
        return Err(bad("size or file-count limit exceeded".into()));
    }
    for f in &p.files {
        if f.path.is_empty()
            || f.path.starts_with('/')
            || f.path.split('/').any(|c| c.is_empty() || c == "." || c == "..")
        {
            return Err(bad(format!("unsafe path '{}'", f.path)));
        }
        if let Some(c) = &f.content
            && sha256_hex(c.as_bytes()) != f.sha256
        {
            return Err(bad(format!("{}: content does not match its hash", f.path)));
        }
    }
    if tree_hash(&p.files) != p.tree_hash {
        return Err(bad("tree hash does not match the files".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(dir: &Path, rel: &str, body: &[u8]) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn nested_files_are_captured_with_kinds_hashes_and_a_stable_tree_hash() {
        let d = tempfile::tempdir().unwrap();
        mk(d.path(), "SKILL.md", b"+++\n+++\nbody");
        mk(d.path(), "references/deep/catalog.md", b"# Catalog");
        mk(d.path(), "examples/a.json", b"{\"a\":1}");
        mk(d.path(), "examples/b.yaml", b"a: 1");
        mk(d.path(), "scripts/check.py", b"print(1)");
        mk(d.path(), "assets/logo.png", &[0x89, b'P', b'N', b'G', 0, 1, 2]);
        mk(d.path(), ".DS_Store", b"x");
        let files = read_dir_files(d.path()).unwrap();
        let got: Vec<(&str, &str)> = files.iter().map(|f| (f.path.as_str(), f.kind.as_str())).collect();
        assert_eq!(
            got,
            vec![
                ("SKILL.md", "markdown"),
                ("assets/logo.png", "binary"),
                ("examples/a.json", "json"),
                ("examples/b.yaml", "yaml"),
                ("references/deep/catalog.md", "markdown"),
                ("scripts/check.py", "code"),
            ]
        );
        assert!(
            files
                .iter()
                .find(|f| f.kind == "binary")
                .unwrap()
                .content
                .is_none()
        );
        let h = tree_hash(&files);
        assert_eq!(
            h,
            tree_hash(&files.iter().rev().cloned().collect::<Vec<_>>()),
            "order-independent"
        );
        mk(d.path(), "references/deep/catalog.md", b"# Catalog v2");
        assert_ne!(
            tree_hash(&read_dir_files(d.path()).unwrap()),
            h,
            "a nested change changes the tree hash"
        );
        assert!(
            files
                .iter()
                .filter(|f| f.is_reference_text())
                .all(|f| f.kind != "code" && f.path != MANIFEST)
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_and_oversized_files_are_refused() {
        let d = tempfile::tempdir().unwrap();
        mk(d.path(), "SKILL.md", b"x");
        std::os::unix::fs::symlink("/etc/hosts", d.path().join("hosts.md")).unwrap();
        assert!(read_dir_files(d.path()).unwrap_err().message.contains("symlink"));
        std::fs::remove_file(d.path().join("hosts.md")).unwrap();
        mk(d.path(), "big.txt", &vec![b'a'; (MAX_FILE_BYTES + 1) as usize]);
        assert!(read_dir_files(d.path()).unwrap_err().message.contains("larger"));
    }
}
