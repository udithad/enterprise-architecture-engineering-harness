//! `eaeh portal`: resolves the stored identities (ADR-0024) and runs the local Governance
//! Portal server (`eaeh_api::portal`, ADR-0026).

use crate::credentials::{Need, Store, select};
use crate::{dx, harness};
use eaeh_api::portal::{Acting, HostFuture, PortalHost, PortalState, RegisterRequest, app};
use eaeh_core::{HarnessError, Principal, ProjectId, Result, Role};
use eaeh_runtime::{Harness, HarnessConfig};
use std::net::SocketAddr;
use std::path::PathBuf;

pub struct PortalArgs {
    pub bind: String,
    pub approver: Option<String>,
    pub steward: Option<String>,
    pub operator: Option<String>,
    pub admin: Option<String>,
    pub workspace_roots: Vec<PathBuf>,
    pub config_path: PathBuf,
    pub static_dir: PathBuf,
    pub dev_origins: Vec<String>,
    pub open: bool,
}

/// Resolve the viewer identity: `--as`, `EAEH_TOKEN`, or the default/only stored profile.
async fn resolve_viewer(h: &Harness) -> Result<Acting> {
    let explicit = dx::AS_PROFILE.get().cloned().flatten();
    if explicit.is_none()
        && let Ok(tok) = std::env::var("EAEH_TOKEN")
    {
        let p = h
            .identity
            .authenticate(tok.trim())
            .await
            .map_err(|_| HarnessError::unauthenticated("EAEH_TOKEN is invalid, expired or revoked"))?;
        return Ok(Acting::new("EAEH_TOKEN", tok.trim(), p));
    }
    resolve_profile(h, explicit.as_deref(), Need::Any).await
}

async fn resolve_profile(h: &Harness, explicit: Option<&str>, need: Need) -> Result<Acting> {
    let store = Store::load()?;
    let dd = dx::data_dir(h);
    let Some((name, _)) = select(&store, &dd, explicit, need)? else {
        return Err(HarnessError::unauthenticated(
            "no stored identity: run `eaeh auth login --profile <name> --token-file <file>` and start the portal with `--as <name>`",
        ));
    };
    let prof = &store.profiles[&name];
    let p = h.identity.authenticate(prof.token.trim()).await.map_err(|_| {
        HarnessError::unauthenticated(format!(
            "the token of profile '{name}' is expired or revoked; renew it with `eaeh auth renew --profile {name}` or log in again"
        ))
    })?;
    Ok(Acting::new(name, prof.token.trim(), p))
}

/// The approver must be named explicitly and be a human approver (ADR-0024: approval
/// identities are never auto-selected). The backend still enforces role, separation of
/// duties and artifact hash on every decision.
async fn resolve_approver(h: &Harness, name: &str) -> Result<Acting> {
    let a = resolve_profile(h, Some(name), Need::Decision).await?;
    if !a.principal.is_human() || !a.principal.has_role(Role::Approver) {
        return Err(HarnessError::forbidden(format!(
            "profile '{name}' ({}) does not hold the approver role; choose an approver profile (`eaeh auth status`)",
            a.principal.id
        )));
    }
    Ok(a)
}

/// The steward must be named explicitly and hold `skill.manage` (knowledge steward or
/// administrator). It is used only to register skill files; it never decides.
async fn resolve_steward(h: &Harness, name: &str) -> Result<Acting> {
    let a = resolve_profile(
        h,
        Some(name),
        Need::Action(eaeh_governance::authz::Action::SkillManage, None),
    )
    .await?;
    if h.authz
        .authorize(
            &a.principal,
            eaeh_governance::authz::Action::SkillManage,
            eaeh_governance::authz::Target::Tenant(a.principal.tenant),
        )
        .is_err()
        && !a.principal.has_role(Role::KnowledgeSteward)
        && !a.principal.has_role(Role::Admin)
    {
        return Err(HarnessError::forbidden(format!(
            "profile '{name}' ({}) cannot manage skills; choose the steward profile (`eaeh auth status`)",
            a.principal.id
        )));
    }
    Ok(a)
}

/// The operator must be named explicitly and may start workflows (`workflow.start`).
async fn resolve_operator(h: &Harness, name: &str) -> Result<Acting> {
    let a = resolve_profile(
        h,
        Some(name),
        Need::Action(eaeh_governance::authz::Action::WorkflowStart, None),
    )
    .await?;
    let ok = a.principal.roles().iter().any(|r| {
        eaeh_governance::authz::RbacAuthorizer::role_permits(
            *r,
            eaeh_governance::authz::Action::WorkflowStart,
        )
    });
    if !ok {
        return Err(HarnessError::forbidden(format!(
            "profile '{name}' ({}) cannot start workflows; choose the developer/operator profile (`eaeh auth status`)",
            a.principal.id
        )));
    }
    Ok(a)
}

/// The administrator must be named explicitly and hold `principal.manage` (registering a
/// project creates it and grants or creates people).
async fn resolve_admin(h: &Harness, name: &str) -> Result<Acting> {
    let a = resolve_profile(
        h,
        Some(name),
        Need::Action(eaeh_governance::authz::Action::PrincipalManage, None),
    )
    .await?;
    if h.authz
        .authorize(
            &a.principal,
            eaeh_governance::authz::Action::PrincipalManage,
            eaeh_governance::authz::Target::Tenant(a.principal.tenant),
        )
        .is_err()
    {
        return Err(HarnessError::forbidden(format!(
            "profile '{name}' ({}) cannot manage principals; choose the administrator profile (`eaeh auth status`)",
            a.principal.id
        )));
    }
    Ok(a)
}

/// The CLI services the portal process calls (W4): the same code as `eaeh project init`
/// and the worker start of `eaeh workflow run`.
struct CliHost {
    cfg: HarnessConfig,
    config_path: PathBuf,
}

impl PortalHost for CliHost {
    fn register<'a>(
        &'a self,
        h: &'a Harness,
        admin: &'a Principal,
        root: PathBuf,
        r: RegisterRequest,
    ) -> HostFuture<'a, serde_json::Value> {
        Box::pin(async move {
            let a = dx::InitArgs {
                name: r.name,
                description: r.description,
                language: r.language,
                framework: r.framework,
                build_tool: r.build_tool,
                constraints: r.constraints,
                brd: r.brd.map(PathBuf::from),
                knowledge: r.knowledge.into_iter().map(PathBuf::from).collect(),
                standards: r.standards.into_iter().map(PathBuf::from).collect(),
                developer: r.developer,
                steward: r.steward,
                approvers: r.approvers,
                mode: None,
                provider: None,
                clients: vec![],
                build_verification: None,
                build_program: None,
                build_args: vec![],
                workflows: vec![],
                // New people's tokens go to the local credential store, never to the browser.
                store_profiles: true,
                handover_dir: None,
                workspace: root,
                ttl_days: 30,
                config_path: self.config_path.clone(),
            };
            let o = dx::register_project(h, admin, a).await?;
            Ok(serde_json::json!({
                "project": o.project,
                "name": o.name,
                "workspace": o.workspace,
                "actions": o.actions,
                "handover": o.handover,
                "readiness": o.readiness,
            }))
        })
    }

    fn ensure_worker<'a>(
        &'a self,
        h: &'a Harness,
        operator: &'a Principal,
        project: ProjectId,
    ) -> HostFuture<'a, Option<String>> {
        Box::pin(async move {
            let scope = eaeh_core::Scope::new(operator.tenant, project);
            let reg = h.engine.materializer().workspace(scope).await?.ok_or_else(|| {
                HarnessError::conflict("the project has no registered workspace; register it first")
            })?;
            let root = PathBuf::from(&reg.root);
            let m = eaeh_runtime::project::ProjectManifest::load(&root)?;
            if m.project.id != project {
                return Err(HarnessError::conflict(format!(
                    "the manifest in {} belongs to another project",
                    reg.root
                )));
            }
            Ok(crate::worker::ensure_worker(&self.cfg, h, &root, &m).await)
        })
    }
}

fn random_secret() -> Result<String> {
    let b = eaeh_core::hash::random_bytes::<32>()?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

pub async fn serve(cfg: &HarnessConfig, args: PortalArgs) -> Result<()> {
    let addr: SocketAddr = args
        .bind
        .parse()
        .map_err(|_| HarnessError::invalid_input(format!("--bind must be ip:port, got '{}'", args.bind)))?;
    if !addr.ip().is_loopback() {
        return Err(HarnessError::invalid_input(
            "the portal has no authentication and binds loopback addresses only (127.0.0.1 or ::1); see docs/implementation/EAEH-GOVERNANCE-PORTAL.md",
        ));
    }
    let h = harness(cfg).await?;
    let viewer = resolve_viewer(&h).await?;
    let approver = match &args.approver {
        Some(n) => Some(resolve_approver(&h, n).await?),
        None => None,
    };
    let steward = match &args.steward {
        Some(n) => Some(resolve_steward(&h, n).await?),
        None => None,
    };
    let operator = match &args.operator {
        Some(n) => Some(resolve_operator(&h, n).await?),
        None => None,
    };
    let admin = match &args.admin {
        Some(n) => Some(resolve_admin(&h, n).await?),
        None => None,
    };
    // Default browse root: the folder that contains the EAEH repository
    // (`<parent>/<repo>/config/eaeh.toml`).
    let roots = if args.workspace_roots.is_empty() {
        cfg.base_dir
            .parent()
            .and_then(|repo| std::fs::canonicalize(repo).ok())
            .and_then(|repo| repo.parent().map(std::path::Path::to_path_buf))
            .into_iter()
            .collect()
    } else {
        args.workspace_roots.clone()
    };
    // `--static-dir`, else the EAEH repository's `portal/dist` next to the configuration
    // (`<repo>/config/eaeh.toml` ⇒ `<repo>/portal/dist`), so it works from a project folder.
    let beside_config = cfg.base_dir.parent().map(|r| r.join("portal/dist"));
    let static_dir = std::iter::once(args.static_dir.clone())
        .chain(beside_config)
        .find(|d| d.join("index.html").is_file());
    let secret = random_secret()?;
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            HarnessError::unavailable(format!(
                "port {} is already in use – most likely another `eaeh portal` (scripts/demo-portal.sh runs one on its own DEMO installation). \
                 Find it with `lsof -nP -iTCP:{} -sTCP:LISTEN`, stop it, or start this one on another port: --bind 127.0.0.1:<port>",
                addr.port(),
                addr.port()
            ))
        } else {
            HarnessError::unavailable(format!("bind {addr}: {e}"))
        }
    })?;
    let addr = listener
        .local_addr()
        .map_err(|e| HarnessError::internal(format!("local addr: {e}")))?;
    let state = PortalState::new(
        viewer,
        approver,
        secret.clone(),
        addr,
        args.dev_origins,
        static_dir.clone(),
        h.execution_mode.to_string(),
    )
    .with_steward(steward)
    .with_operations(
        operator,
        admin,
        roots,
        Some(std::sync::Arc::new(CliHost {
            cfg: cfg.clone(),
            config_path: args.config_path.clone(),
        })),
    );
    let app = app(h.clone(), state.clone());
    // Skill directories copied into the skills folder appear without a restart (O3).
    let _discovery = eaeh_runtime::project::spawn_skill_discovery(
        h.clone(),
        eaeh_runtime::project::skill_discovery_interval(),
    );

    eprintln!("EAEH Governance Portal (local development only – no authentication)");
    eprintln!(
        "  viewing as : {} ({}, roles: {})",
        state.viewer.label,
        state.viewer.principal.id,
        roles(&state.viewer.principal)
    );
    match &state.approver {
        Some(a) => eprintln!(
            "  deciding as: {} ({}, roles: {})",
            a.label,
            a.principal.id,
            roles(&a.principal)
        ),
        None => eprintln!("  deciding as: nobody – read-only (start with --approver <profile> to decide)"),
    }
    match &state.steward {
        Some(s) => eprintln!(
            "  skills as : {} ({}) – registers skill files only",
            s.label, s.principal.id
        ),
        None => eprintln!(
            "  skills as : nobody – start with --steward <profile> to register skill files in the portal"
        ),
    }
    match &state.operator {
        Some(o) => eprintln!(
            "  operating as: {} ({}) – start, cancel, resume, restart, retry",
            o.label, o.principal.id
        ),
        None => eprintln!("  operating as: nobody – start with --operator <profile> for workflow operations"),
    }
    match &state.admin {
        Some(a) => eprintln!(
            "  admin as   : {} ({}) – register existing projects under {}",
            a.label,
            a.principal.id,
            state
                .workspace_roots
                .iter()
                .map(|r| r.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        None => eprintln!("  admin as   : nobody – start with --admin <profile> to register projects"),
    }
    if static_dir.is_none() {
        eprintln!(
            "  web UI     : not built at {} – run `npm --prefix portal ci && npm --prefix portal run build`, or use the Vite dev server",
            args.static_dir.display()
        );
    }
    eprintln!("  execution  : {}", h.execution_mode);
    eprintln!("  data dir   : {}", dx::data_dir(&h));
    eprintln!("  config dir : {}", cfg.base_dir.display());
    if let Ok(l) = std::env::var("EAEH_INSTALLATION_LABEL") {
        eprintln!("  installation: {l}");
    }
    for w in installation_warnings(&h, &state.viewer.principal).await {
        eprintln!("{w}");
    }
    eprintln!("Open (this link carries a one-time launch secret – do not share it):");
    // stdout: the only place the launch URL is written.
    let url = format!("http://127.0.0.1:{}/?launch={secret}", addr.port());
    println!("{url}");
    if args.open {
        // The secret goes straight to the local browser; no copying needed.
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        match std::process::Command::new(opener).arg(&url).status() {
            Ok(s) if s.success() => {
                eprintln!("Opened in your default browser. Keep this terminal open; Ctrl-C stops the portal.")
            }
            _ => eprintln!("Could not open a browser ({opener}); open the link above manually."),
        }
    } else {
        eprintln!("Keep this terminal open while you use the portal; Ctrl-C stops it.");
    }
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|e| HarnessError::internal(format!("portal server: {e}")))?;
    h.shutdown().await;
    Ok(())
}

/// Warn when the project of the current folder is not registered in this installation:
/// the portal would not show it (CLI and portal on different EAEH installations).
pub(crate) async fn installation_warnings(h: &eaeh_runtime::Harness, viewer: &Principal) -> Vec<String> {
    let Ok(cwd) = std::env::current_dir() else {
        return vec![];
    };
    let Some(root) = eaeh_runtime::project::find_root(&cwd) else {
        return vec![];
    };
    let Ok(m) = eaeh_runtime::project::ProjectManifest::load(&root) else {
        return vec![];
    };
    let known = h
        .identity
        .projects(viewer.tenant)
        .await
        .map(|ps| ps.iter().any(|(id, _)| *id == m.project.id))
        .unwrap_or(false);
    if known && m.project.tenant == viewer.tenant {
        return vec![];
    }
    vec![
        String::new(),
        "  ⚠ WARNING: the project in this folder is NOT registered in this EAEH installation.".into(),
        format!("    folder project : {} ({}), {}", m.project.name, m.project.id, root.display()),
        format!("    this portal    : data dir {}", dx::data_dir(h)),
        "    The portal will not show it. The CLI that created it uses another configuration:".into(),
        "    start the portal with the same configuration (check `echo $EAEH_CONFIG`, or pass --config <file>).".into(),
        String::new(),
    ]
}

fn roles(p: &Principal) -> String {
    p.roles()
        .iter()
        .map(|r| format!("{r:?}").to_lowercase())
        .collect::<Vec<_>>()
        .join(",")
}
