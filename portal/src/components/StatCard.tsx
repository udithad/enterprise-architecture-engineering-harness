import type { ReactNode } from "react";
import { Link } from "react-router";
import { Card } from "@/components/ui/card";
import { cn } from "@/lib/utils";

export function StatCard({ label, value, icon, to, hint, accent }: { label: string; value: number | string; icon: ReactNode; to?: string; hint?: string; accent?: string }) {
  const body = (
    <Card className={cn("flex h-full items-start justify-between gap-3 p-4 transition-colors", to && "hover:border-slate-300 hover:bg-muted/30")}>
      <div className="min-w-0">
        <p className="text-xs font-medium text-muted-foreground">{label}</p>
        <p className="mt-1.5 text-2xl font-semibold tabular-nums" data-testid={`stat-${label}`}>
          {value}
        </p>
        {hint && <p className="mt-0.5 truncate text-xs text-muted-foreground">{hint}</p>}
      </div>
      <div className={cn("rounded-md p-2 [&_svg]:size-4", accent ?? "bg-muted text-muted-foreground")}>{icon}</div>
    </Card>
  );
  return to ? (
    <Link to={to} className="block rounded-lg focus-visible:outline-2" aria-label={`${label}: ${value}`}>
      {body}
    </Link>
  ) : (
    body
  );
}
