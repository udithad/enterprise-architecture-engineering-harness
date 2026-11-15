//! EAEH workspace and artifact management (master §21).
//!
//! * [`WorkspaceManager`] / [`Workspace`] – per-scope directories with strict path
//!   confinement (no absolute paths, no `..`, no symlinks anywhere on the path, final
//!   component opened with `O_NOFOLLOW`), size limits, atomic writes, and
//!   ownership locks that serialize concurrent modification.
//! * [`ArtifactStore`] – immutable, versioned, content-addressed (SHA-256) artifacts with
//!   provenance, integrity verification on read, audit and `ArtifactGenerated` events.

mod artifacts;
mod locks;
mod materialize;
mod workspace;

pub use artifacts::{ArtifactMeta, ArtifactStore, Provenance};
pub use locks::LockInfo;
pub use materialize::{
    AttemptRecord, FileAction, FileCheck, INDEX_FILE, MaterializationStatus, MaterializeReport,
    MaterializeRequest, Materializer, WorkspaceRegistration,
};
pub use workspace::{DirEntry, Workspace, WorkspaceManager};

/// Validate a workspace-relative path and return its normal components.
pub fn validate_relative(rel: &str) -> eaeh_core::Result<Vec<String>> {
    use eaeh_core::HarnessError;
    use std::path::{Component, Path};
    if rel.is_empty() || rel.len() > 1024 || rel.contains('\0') {
        return Err(HarnessError::invalid_input(
            "path must be 1..=1024 bytes without NUL",
        ));
    }
    let mut parts = vec![];
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(s) => {
                let s = s
                    .to_str()
                    .ok_or_else(|| HarnessError::invalid_input("path must be UTF-8"))?;
                if s.starts_with(".eaeh-") {
                    return Err(HarnessError::forbidden("reserved path component"));
                }
                parts.push(s.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(HarnessError::forbidden(format!("path traversal rejected: {rel}")));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(HarnessError::forbidden(format!("absolute path rejected: {rel}")));
            }
        }
    }
    if parts.is_empty() {
        return Err(HarnessError::invalid_input("path resolves to workspace root"));
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_validation() {
        assert_eq!(validate_relative("src/main.rs").unwrap(), vec!["src", "main.rs"]);
        assert_eq!(validate_relative("./a").unwrap(), vec!["a"]);
        for bad in ["../x", "a/../../x", "/etc/passwd", "", "a\0b", ".eaeh-tmp"] {
            assert!(validate_relative(bad).is_err(), "{bad}");
        }
    }
}
