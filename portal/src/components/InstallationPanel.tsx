import { Database, Info } from "lucide-react";
import { useSession } from "@/hooks/queries";

/** Which EAEH installation this portal reads, and why a CLI project might be missing.
 * The portal and the CLI show the same projects only on the same configuration/data dir. */
export function InstallationPanel({ projectCount, compact = false }: { projectCount?: number; compact?: boolean }) {
  const s = useSession();
  if (!s.data) return null;
  const i = s.data.installation;
  const demo = !!i.label;
  return (
    <div
      className={`rounded-md border px-4 py-3 text-xs ${demo ? "border-amber-400 bg-amber-50 text-amber-950" : "border-slate-200 bg-slate-50 text-slate-700"}`}
      data-testid="installation-panel"
    >
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <span className="inline-flex items-center gap-1.5 font-medium">
          <Database className="size-3.5" />
          {demo ? <span data-testid="installation-label">{i.label}</span> : "EAEH installation"}
        </span>
        <span>
          data <code className="break-all">{i.data_dir}</code>
        </span>
        <span>
          config <code className="break-all">{i.config_dir}</code>
        </span>
        <span>mode {s.data.execution_mode}</span>
        {projectCount !== undefined && <span>{projectCount} project(s) visible</span>}
      </div>
      {!compact && (
        <details className="mt-2">
          <summary className="inline-flex cursor-pointer items-center gap-1 font-medium">
            <Info className="size-3.5" /> Not seeing a project?
          </summary>
          <ul className="mt-1 list-disc pl-5 leading-relaxed">
            <li>
              The portal shows the projects of <b>this</b> installation only. In the project folder, run <code>eaeh project list</code> or <code>eaeh workflow status</code>. Each prints the data directory it uses, and it must match the one above. If it doesn't, stop this portal and start it with the same configuration as your CLI (<code>echo $EAEH_CONFIG</code>).
            </li>
            <li>
              <code>scripts/demo-portal.sh</code> runs a separate <b>DEMO</b> installation (port 8190). Your own projects never appear there.
            </li>
            <li>A project you can't access is listed as restricted. Access is granted per project (<code>eaeh principal grant</code>).</li>
            <li>New projects and workflows appear within about 10 seconds. No restart or reload is needed.</li>
          </ul>
        </details>
      )}
    </div>
  );
}
