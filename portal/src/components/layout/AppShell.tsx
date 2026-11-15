import { useState, type ReactNode } from "react";
import { NavLink, Outlet } from "react-router";
import { Activity, CheckSquare, FolderKanban, GitBranch, LayoutDashboard, Menu, Settings, ShieldAlert, X } from "lucide-react";
import { useDashboard, useSession } from "@/hooks/queries";
import { cn } from "@/lib/utils";

const nav: { to: string; label: string; icon: ReactNode; end?: boolean }[] = [
  { to: "/", label: "Dashboard", icon: <LayoutDashboard />, end: true },
  { to: "/projects", label: "Projects", icon: <FolderKanban /> },
  { to: "/workflows", label: "Workflows", icon: <GitBranch /> },
  { to: "/approvals", label: "Approvals", icon: <CheckSquare /> },
  { to: "/activity", label: "Activity / Audit", icon: <Activity /> },
  { to: "/settings", label: "Settings", icon: <Settings /> },
];

function Sidebar({ onNavigate }: { onNavigate?: () => void }) {
  const dash = useDashboard();
  const pending = dash.data?.summary.pending_approvals ?? 0;
  return (
    <div className="flex h-full flex-col bg-sidebar text-sidebar-foreground">
      <div className="flex h-14 items-center gap-2.5 border-b border-white/5 px-4">
        <div className="grid size-7 place-items-center rounded-md bg-white/10 text-sm font-bold text-white">E</div>
        <div className="leading-tight">
          <div className="text-sm font-semibold text-white">EAEH</div>
          <div className="text-[11px] text-sidebar-muted">Governance Portal</div>
        </div>
      </div>
      <nav aria-label="Main" className="flex-1 overflow-y-auto px-2 py-3">
        <ul className="flex flex-col gap-0.5">
          {nav.map((n) => (
            <li key={n.to}>
              <NavLink
                to={n.to}
                end={n.end}
                onClick={onNavigate}
                className={({ isActive }) =>
                  cn(
                    "flex items-center gap-2.5 rounded-md px-3 py-2 text-sm transition-colors [&_svg]:size-4",
                    isActive ? "bg-sidebar-active font-medium text-white" : "hover:bg-white/5 hover:text-white",
                  )
                }
              >
                {n.icon}
                <span className="flex-1">{n.label}</span>
                {n.to === "/approvals" && pending > 0 && (
                  <span className="rounded-full bg-amber-500 px-1.5 text-[11px] font-semibold text-slate-950" aria-label={`${pending} pending`}>
                    {pending}
                  </span>
                )}
              </NavLink>
            </li>
          ))}
        </ul>
      </nav>
      <div className="border-t border-white/5 p-3 text-[11px] leading-relaxed text-sidebar-muted">
        Phases 1–3 · local development
      </div>
    </div>
  );
}

function IdentityBar() {
  const s = useSession();
  if (!s.data) return null;
  return (
    <div className="flex min-w-0 items-center gap-3 text-xs text-muted-foreground">
      {s.data.installation.label && (
        <span className="rounded border border-amber-400 bg-amber-100 px-1.5 py-0.5 font-semibold text-amber-950" title={s.data.installation.data_dir} data-testid="installation-badge">
          {s.data.installation.label.split(" ")[0]}
        </span>
      )}
      <span className="hidden max-w-72 truncate rounded border px-1.5 py-0.5 font-mono xl:inline" title={`Data: ${s.data.installation.data_dir}\nConfig: ${s.data.installation.config_dir}`} data-testid="installation">
        {s.data.installation.data_dir}
      </span>
      <span className="hidden truncate md:inline">
        Viewing as <span className="font-medium text-foreground">{s.data.viewer.principal}</span>
      </span>
      <span className="truncate">
        {s.data.approver ? (
          <>
            Deciding as <span className="font-medium text-foreground">{s.data.approver.principal}</span>
          </>
        ) : (
          <span className="rounded border px-1.5 py-0.5">Read-only</span>
        )}
      </span>
      {s.data.execution_mode === "deterministic" ? (
        <span className="rounded border border-amber-300 bg-amber-50 px-1.5 py-0.5 text-amber-900" title="Agent stages replay recorded responses; nothing is AI-generated">
          deterministic mode · recorded responses, not AI
        </span>
      ) : (
        <span className="hidden rounded border px-1.5 py-0.5 lg:inline">mode: {s.data.execution_mode}</span>
      )}
    </div>
  );
}

export function AppShell() {
  const [open, setOpen] = useState(false);
  return (
    <div className="flex min-h-screen">
      <aside className="sticky top-0 hidden h-screen w-60 shrink-0 lg:block">
        <Sidebar />
      </aside>
      {open && (
        <div className="fixed inset-0 z-40 lg:hidden" role="dialog" aria-modal="true" aria-label="Navigation">
          <div className="absolute inset-0 bg-slate-950/40" onClick={() => setOpen(false)} />
          <div className="absolute inset-y-0 left-0 w-64">
            <Sidebar onNavigate={() => setOpen(false)} />
          </div>
          <button className="absolute left-66 top-3 rounded-md bg-card p-1.5" onClick={() => setOpen(false)} aria-label="Close navigation">
            <X className="size-4" />
          </button>
        </div>
      )}
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="sticky top-0 z-30 flex h-14 items-center gap-3 border-b bg-card/95 px-4 backdrop-blur lg:px-8">
          <button className="rounded-md p-1.5 hover:bg-muted lg:hidden" onClick={() => setOpen(true)} aria-label="Open navigation">
            <Menu className="size-5" />
          </button>
          <div className="flex items-center gap-1.5 text-xs text-amber-800" title="No authentication: trusted local development only">
            <ShieldAlert className="size-3.5" aria-hidden />
            <span className="hidden sm:inline">Local development · no authentication</span>
          </div>
          <div className="ml-auto min-w-0">
            <IdentityBar />
          </div>
        </header>
        <main className="mx-auto w-full max-w-[1400px] flex-1 px-4 py-6 lg:px-8">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
