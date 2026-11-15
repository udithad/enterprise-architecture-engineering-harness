import { useEffect, useState, type ReactNode } from "react";
import { KeyRound } from "lucide-react";
import { api, ApiError } from "@/lib/api";
import { useSession } from "@/hooks/queries";
import { ErrorState, LoadingRows } from "./States";

/**
 * No login: the launch secret printed by `eaeh portal` arrives as `?launch=…`, is exchanged
 * once for an HttpOnly session cookie and removed from the address bar.
 */
export function SessionGate({ children }: { children: ReactNode }) {
  const [exchanging, setExchanging] = useState(() => new URLSearchParams(window.location.search).has("launch"));
  const [exchangeError, setExchangeError] = useState<unknown>(null);
  useEffect(() => {
    const url = new URL(window.location.href);
    const launch = url.searchParams.get("launch");
    if (!launch) return;
    url.searchParams.delete("launch");
    window.history.replaceState(null, "", url.pathname + url.search + url.hash);
    api
      .openSession(launch)
      .catch((e) => setExchangeError(e))
      .finally(() => setExchanging(false));
  }, []);
  // Ask who we are only after the launch secret has been exchanged for the cookie;
  // otherwise the first request races the exchange and caches a 401.
  const session = useSession(!exchanging);

  if (exchanging) return <div className="p-8"><LoadingRows rows={3} label="Opening portal" /></div>;
  if (!session.data && session.isPending) return <div className="p-8"><LoadingRows rows={3} label="Opening portal" /></div>;
  if (session.isError) {
    const needsLaunch = session.error instanceof ApiError && session.error.code === "portal_session_required";
    return (
      <div className="mx-auto mt-24 max-w-lg px-4">
        {needsLaunch ? (
          <div className="rounded-lg border bg-card p-6 shadow-xs">
            <div className="mb-2 flex items-center gap-2 font-semibold">
              <KeyRound className="size-4" /> Open the portal from your terminal
            </div>
            <p className="text-sm text-muted-foreground">
              The EAEH Governance Portal has no login. Open the link printed by <code className="font-mono">eaeh portal</code>; it carries a one-time launch secret for this process.
            </p>
            {exchangeError != null && <p className="mt-3 text-xs text-red-700">The launch link was not accepted: {exchangeError instanceof Error ? exchangeError.message : String(exchangeError)}</p>}
          </div>
        ) : (
          <ErrorState error={session.error} onRetry={() => void session.refetch()} />
        )}
      </div>
    );
  }
  return <>{children}</>;
}
