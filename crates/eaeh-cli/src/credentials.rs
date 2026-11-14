//! Local credential store for the CLI and MCP server (developer experience, ADR-0024).
//!
//! * Location: `$EAEH_HOME/credentials.json`, else `$XDG_CONFIG_HOME/eaeh/…`, else
//!   `~/.config/eaeh/…`. Directory `0700`, file `0600`; written atomically; refused if
//!   group/world readable.
//! * One **profile** per identity (e.g. `admin`, `developer`, `steward`, `approver`).
//!   Profiles are bound to the data directory they were created for.
//! * Tokens are never printed or logged; only metadata (principal, roles, expiry) is shown.
//! * Selection order: `--as <profile>` / `EAEH_PROFILE` → `EAEH_TOKEN` → automatic
//!   (only for non-decision operations, only when exactly one stored identity is
//!   authorized) → default profile. Approval decisions are never auto-selected.

use eaeh_core::{HarnessError, ProjectAccess, ProjectId, Result, Role};
use eaeh_governance::authz::{Action, RbacAuthorizer};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Profile {
    pub token: String,
    pub principal: String,
    pub tenant: String,
    pub roles: Vec<Role>,
    pub projects: ProjectAccess,
    pub token_id: String,
    pub expires_at: String,
    /// Absolute data directory this credential belongs to.
    pub data_dir: String,
}

impl std::fmt::Debug for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Profile")
            .field("principal", &self.principal)
            .field("roles", &self.roles)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl Profile {
    pub fn permits(&self, action: Action, project: Option<ProjectId>) -> bool {
        let role_ok = self
            .roles
            .iter()
            .any(|r| RbacAuthorizer::role_permits(*r, action));
        let project_ok = match (project, &self.projects) {
            (None, _) | (_, ProjectAccess::All) => true,
            (Some(p), ProjectAccess::Only(set)) => set.contains(&p),
        };
        role_ok && project_ok
    }

    pub fn expired(&self) -> bool {
        chrono::DateTime::parse_from_rfc3339(&self.expires_at)
            .map(|t| t <= chrono::Utc::now())
            .unwrap_or(false)
    }

    /// Public metadata (never the token).
    pub fn describe(&self, name: &str, default: bool) -> serde_json::Value {
        serde_json::json!({
            "profile": name,
            "default": default,
            "principal": self.principal,
            "roles": self.roles.iter().map(|r| r.as_str()).collect::<Vec<_>>(),
            "projects": self.projects,
            "token_id": self.token_id,
            "expires_at": self.expires_at,
            "expired": self.expired(),
        })
    }
}

pub fn home() -> Result<PathBuf> {
    if let Ok(h) = std::env::var("EAEH_HOME")
        && !h.is_empty()
    {
        return Ok(PathBuf::from(h));
    }
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME")
        && !x.is_empty()
    {
        return Ok(PathBuf::from(x).join("eaeh"));
    }
    let h =
        std::env::var("HOME").map_err(|_| HarnessError::invalid_input("HOME is not set; set EAEH_HOME"))?;
    Ok(PathBuf::from(h).join(".config").join("eaeh"))
}

fn file() -> Result<PathBuf> {
    Ok(home()?.join("credentials.json"))
}

#[cfg(unix)]
fn check_mode(p: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(p)?.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(HarnessError::forbidden(format!(
            "credential store {} is accessible by other users (mode {mode:o}); run `chmod 600 {}`",
            p.display(),
            p.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_mode(_p: &Path) -> Result<()> {
    Ok(())
}

impl Store {
    pub fn load() -> Result<Self> {
        let p = file()?;
        if !p.exists() {
            return Ok(Self::default());
        }
        check_mode(&p)?;
        serde_json::from_slice(&std::fs::read(&p)?)
            .map_err(|e| HarnessError::integrity(format!("credential store {}: {e}", p.display())))
    }

    pub fn save(&self) -> Result<()> {
        let dir = home()?;
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let tmp = dir.join(format!(".credentials.{}.tmp", std::process::id()));
        {
            let mut o = std::fs::OpenOptions::new();
            o.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                o.mode(0o600);
            }
            use std::io::Write;
            let mut f = o.open(&tmp)?;
            f.write_all(&serde_json::to_vec_pretty(self)?)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, file()?)?;
        Ok(())
    }

    /// Profiles that belong to this installation.
    pub fn for_data_dir<'a>(&'a self, data_dir: &'a str) -> impl Iterator<Item = (&'a String, &'a Profile)> {
        self.profiles.iter().filter(move |(_, p)| p.data_dir == data_dir)
    }
}

/// How an identity is required for a command.
#[derive(Debug, Clone, Copy)]
pub enum Need {
    /// Any authenticated identity (reads; the services authorize precisely).
    Any,
    /// An identity holding `action` (auto-selected when unambiguous).
    Action(Action, Option<ProjectId>),
    /// A human approval decision: never auto-selected.
    Decision,
}

/// Choose a stored profile for `need`. Returns `(profile name, reason)` or a diagnostic.
pub fn select(
    store: &Store,
    data_dir: &str,
    explicit: Option<&str>,
    need: Need,
) -> Result<Option<(String, String)>> {
    if let Some(name) = explicit {
        return match store.profiles.get(name) {
            Some(p) if p.data_dir == data_dir => Ok(Some((name.to_string(), "--as".into()))),
            Some(_) => Err(HarnessError::unauthenticated(format!(
                "profile '{name}' belongs to another EAEH installation (data directory differs)"
            ))),
            None => Err(HarnessError::unauthenticated(format!(
                "no profile named '{name}'"
            ))),
        };
    }
    let mine: Vec<(&String, &Profile)> = store.for_data_dir(data_dir).collect();
    if mine.is_empty() {
        return Ok(None);
    }
    let default = store
        .default
        .as_ref()
        .filter(|d| mine.iter().any(|(n, _)| n == d))
        .cloned();
    match need {
        Need::Any => Ok(default
            .map(|d| (d, "default profile".into()))
            .or_else(|| (mine.len() == 1).then(|| (mine[0].0.clone(), "only profile".into())))),
        Need::Decision => {
            let d = default.and_then(|d| store.profiles[&d].roles.contains(&Role::Approver).then_some(d));
            match d {
                Some(d) => Ok(Some((d, "default profile (approver)".into()))),
                None => {
                    let approvers: Vec<&String> = mine
                        .iter()
                        .filter(|(_, p)| p.roles.contains(&Role::Approver))
                        .map(|(n, _)| *n)
                        .collect();
                    Err(HarnessError::unauthenticated(format!(
                        "approval decisions require an explicit approver identity: add `--as <profile>`{}",
                        if approvers.is_empty() {
                            " (no stored profile has the approver role; `eaeh auth login --profile approver --token-file <file>`)".to_string()
                        } else {
                            format!(
                                " (approver profiles: {})",
                                approvers
                                    .iter()
                                    .map(|s| s.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        }
                    )))
                }
            }
        }
        Need::Action(action, project) => {
            let ok: Vec<&String> = mine
                .iter()
                .filter(|(_, p)| p.permits(action, project) && !p.expired())
                .map(|(n, _)| *n)
                .collect();
            if let Some(d) = &default
                && ok.contains(&d)
            {
                return Ok(Some((d.clone(), "default profile".into())));
            }
            match ok.as_slice() {
                [one] => Ok(Some((
                    (*one).clone(),
                    format!(
                        "automatically selected: only stored identity permitted to {}",
                        action.as_str()
                    ),
                ))),
                [] => {
                    let roles: Vec<&str> = Role::ALL
                        .iter()
                        .filter(|r| RbacAuthorizer::role_permits(**r, action))
                        .map(|r| r.as_str())
                        .collect();
                    Err(HarnessError::forbidden(format!(
                        "no stored identity may perform {} here; it requires one of the roles [{}] with access to the project. Run `eaeh auth status` to see your profiles",
                        action.as_str(),
                        roles.join(", ")
                    )))
                }
                many => Err(HarnessError::unauthenticated(format!(
                    "several stored identities may perform {}: {}; choose one with `--as <profile>` or set a default with `eaeh auth use <profile>`",
                    action.as_str(),
                    many.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                ))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prof(roles: &[Role], dir: &str) -> Profile {
        Profile {
            token: "eaeh_secret".into(),
            principal: "p".into(),
            tenant: "t".into(),
            roles: roles.to_vec(),
            projects: ProjectAccess::All,
            token_id: "x".into(),
            expires_at: "2999-01-01T00:00:00Z".into(),
            data_dir: dir.into(),
        }
    }

    #[test]
    fn selection_rules() {
        let mut s = Store::default();
        s.profiles.insert("admin".into(), prof(&[Role::Admin], "/d"));
        s.profiles
            .insert("developer".into(), prof(&[Role::Operator], "/d"));
        s.profiles
            .insert("approver".into(), prof(&[Role::Approver], "/d"));
        s.profiles
            .insert("other".into(), prof(&[Role::Operator], "/elsewhere"));
        // Unique holder of the permission is chosen automatically.
        let (n, _) = select(&s, "/d", None, Need::Action(Action::WorkflowStart, None))
            .unwrap()
            .unwrap();
        assert_eq!(n, "developer");
        let (n, _) = select(&s, "/d", None, Need::Action(Action::PrincipalManage, None))
            .unwrap()
            .unwrap();
        assert_eq!(n, "admin");
        // Approval decisions are never auto-selected.
        let e = select(&s, "/d", None, Need::Decision).unwrap_err();
        assert!(
            e.message.contains("--as") && e.message.contains("approver"),
            "{}",
            e.message
        );
        s.default = Some("approver".into());
        assert_eq!(
            select(&s, "/d", None, Need::Decision).unwrap().unwrap().0,
            "approver"
        );
        // Nobody may: forbidden with the roles that would grant it.
        let mut only_dev = Store::default();
        only_dev
            .profiles
            .insert("developer".into(), prof(&[Role::Operator], "/d"));
        let e = select(&only_dev, "/d", None, Need::Action(Action::AuditRead, None)).unwrap_err();
        assert!(e.message.contains("admin, auditor"), "{}", e.message);
        // Profiles of other installations are ignored / refused.
        assert!(select(&s, "/d", Some("other"), Need::Any).is_err());
        // Debug output never contains the token.
        assert!(!format!("{:?}", s.profiles["admin"]).contains("eaeh_secret"));
    }
}
