import { Ban, Bot, CheckCircle2, Circle, CircleAlert, Hourglass, Loader2, UserCheck, XCircle } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import type { StatusInfo, Tone } from "@/lib/status";

const toneClass: Record<Tone, string> = {
  running: "border-sky-200 bg-sky-50 text-sky-800",
  ai: "border-violet-200 bg-violet-50 text-violet-800",
  approval: "border-amber-200 bg-amber-50 text-amber-900",
  success: "border-emerald-200 bg-emerald-50 text-emerald-800",
  failed: "border-red-200 bg-red-50 text-red-800",
  warning: "border-orange-200 bg-orange-50 text-orange-800",
  neutral: "border-slate-200 bg-slate-50 text-slate-700",
};

function ToneIcon({ info }: { info: StatusInfo }) {
  switch (info.tone) {
    case "running":
      return <Loader2 className="animate-spin" aria-hidden />;
    case "ai":
      return <Bot aria-hidden />;
    case "approval":
      return info.label === "Pending" || info.label.includes("approval") ? <UserCheck aria-hidden /> : <Hourglass aria-hidden />;
    case "success":
      return <CheckCircle2 aria-hidden />;
    case "failed":
      return <XCircle aria-hidden />;
    case "warning":
      return <CircleAlert aria-hidden />;
    default:
      return info.label === "Cancelled" ? <Ban aria-hidden /> : <Circle aria-hidden />;
  }
}

export function StatusBadge({ info, className }: { info: StatusInfo; className?: string }) {
  return (
    <Badge className={cn(toneClass[info.tone], className)} data-tone={info.tone}>
      <ToneIcon info={info} />
      {info.label}
    </Badge>
  );
}

export function toneDot(tone: Tone): string {
  return {
    running: "bg-sky-500",
    ai: "bg-violet-500",
    approval: "bg-amber-500",
    success: "bg-emerald-500",
    failed: "bg-red-500",
    warning: "bg-orange-500",
    neutral: "bg-slate-400",
  }[tone];
}
