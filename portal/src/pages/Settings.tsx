import { ShieldAlert } from "lucide-react";
import { useSession } from "@/hooks/queries";
import { PageHeader } from "@/components/PageHeader";
import { ErrorState, LoadingRows } from "@/components/States";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import type { Acting } from "@/lib/types";

function Identity({ title, a, empty }: { title: string; a: Acting | null; empty: string }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
      </CardHeader>
      <CardContent className="text-sm">
        {a ? (
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5">
            <dt className="text-muted-foreground">Profile</dt>
            <dd>{a.profile}</dd>
            <dt className="text-muted-foreground">Principal</dt>
            <dd>{a.principal}</dd>
            <dt className="text-muted-foreground">Kind</dt>
            <dd>{a.kind}</dd>
            <dt className="text-muted-foreground">Roles</dt>
            <dd>{a.roles.join(", ") || "—"}</dd>
            <dt className="text-muted-foreground">Projects</dt>
            <dd className="break-all">{typeof a.projects === "string" ? a.projects : JSON.stringify(a.projects)}</dd>
          </dl>
        ) : (
          <p className="text-muted-foreground">{empty}</p>
        )}
      </CardContent>
    </Card>
  );
}

/** Read-only: the portal has no settings of its own; these are the launch options in force. */
export function SettingsPage() {
  const s = useSession();
  return (
    <>
      <PageHeader title="Settings" description="Read-only. Change identities or mode by restarting `eaeh portal` with different options or configuration." />
      {s.isPending && <LoadingRows />}
      {s.isError && <ErrorState error={s.error} />}
      {s.data && (
        <div className="grid gap-6 lg:grid-cols-2">
          <Identity title="Viewing identity (--as)" a={s.data.viewer} empty="—" />
          <Identity title="Approver identity (--approver)" a={s.data.approver} empty="None: the portal is read-only. Restart with --approver <approver-profile> to record decisions." />
          <Card>
            <CardHeader>
              <CardTitle>Installation</CardTitle>
              <CardDescription>The portal shows only this installation. A workflow started with a different EAEH_CONFIG / data directory appears in a portal started with that configuration.</CardDescription>
            </CardHeader>
            <CardContent className="text-sm">
              <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5">
                <dt className="text-muted-foreground">Data directory</dt>
                <dd className="break-all font-mono text-xs">{s.data.installation.data_dir}</dd>
                <dt className="text-muted-foreground">Config directory</dt>
                <dd className="break-all font-mono text-xs">{s.data.installation.config_dir}</dd>
                <dt className="text-muted-foreground">Execution mode</dt>
                <dd>{s.data.execution_mode}</dd>
                <dt className="text-muted-foreground">API version</dt>
                <dd>{s.data.api_version}</dd>
                <dt className="text-muted-foreground">Authentication</dt>
                <dd>{s.data.authentication}</dd>
              </dl>
            </CardContent>
          </Card>
          <Card className="border-amber-200">
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <ShieldAlert className="size-4 text-amber-700" /> Security notice
              </CardTitle>
              <CardDescription>Local development only.</CardDescription>
            </CardHeader>
            <CardContent className="text-sm text-muted-foreground">
              The portal has no login. It acts as the identities above, listens on a loopback address only and requires the launch secret printed in your terminal. Do not expose it to other machines, users or tenants; authentication must be
              introduced before the portal is used beyond a trusted local development environment.
            </CardContent>
          </Card>
        </div>
      )}
    </>
  );
}
