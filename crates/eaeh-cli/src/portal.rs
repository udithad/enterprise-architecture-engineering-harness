//! `eaeh portal`: resolves the stored identities (ADR-0024) and runs the local Governance
//! Portal server (`eaeh_api::portal`, ADR-0026).

use crate::credentials::{Need, Store, select};
use crate::{dx, harness};
use eaeh_api::portal::{Acting, PortalState, app};
use eaeh_core::{HarnessError, Principal, Result, Role};
use eaeh_runtime::{Harness, HarnessConfig};
use std::net::SocketAddr;
use std::path::PathBuf;

pub struct PortalArgs {
    pub bind: String,
    pub approver: Option<String>,
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
    );
    let app = app(h.clone(), state.clone());

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
