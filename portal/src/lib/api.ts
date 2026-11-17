// Thin client for the portal server. The browser never holds an EAEH token: the local
// `eaeh portal` process injects the configured identity (ADR-0026). All requests are
// same-origin and carry the HttpOnly session cookie.

export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
    public requestId?: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

async function parse(res: Response): Promise<unknown> {
  const text = await res.text();
  if (!text) return null;
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

export async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let res: Response;
  try {
    res = await fetch(path, {
      credentials: "same-origin",
      ...init,
      headers: { Accept: "application/json", ...(init?.body ? { "Content-Type": "application/json" } : {}), ...init?.headers },
    });
  } catch {
    throw new ApiError(0, "network_error", "The portal server is not reachable. Is `eaeh portal` still running?");
  }
  const body = await parse(res);
  if (!res.ok) {
    const e = (body as { error?: { code?: string; message?: string; request_id?: string } } | null)?.error;
    throw new ApiError(
      res.status,
      e?.code ?? `http_${res.status}`,
      e?.message ?? (typeof body === "string" && body ? body : `Request failed with status ${res.status}`),
      e?.request_id ?? res.headers.get("x-request-id") ?? undefined,
    );
  }
  return body as T;
}

export const api = {
  get: <T>(path: string) => request<T>(`/api${path}`),
  post: <T>(path: string, body: unknown) => request<T>(`/api${path}`, { method: "POST", body: JSON.stringify(body) }),
  session: () => request<import("./types").PortalSession>("/portal/session"),
  openSession: (launch: string) =>
    request<{ ok: boolean }>("/portal/session", { method: "POST", body: JSON.stringify({ launch }) }),
};

/** Human-readable guidance for an API failure. */
export function describeError(e: unknown): { title: string; detail: string; hint?: string } {
  if (e instanceof ApiError) {
    const hints: Record<string, string> = {
      portal_session_required: "Open the portal with the link printed by `eaeh portal` in your terminal.",
      unauthenticated: "The stored identity is expired or revoked. Renew it with `eaeh auth renew --profile <name>` and restart the portal.",
      forbidden: "The identity the portal runs as does not hold the permission for this view. Check `eaeh auth status`.",
      approver_not_configured: "Restart the portal with `--approver <approver-profile>` to record decisions.",
      operator_not_configured: "Restart the portal with `--operator <developer-profile>` to start, cancel, resume, restart or retry workflows.",
      admin_not_configured: "Restart the portal with `--admin <admin-profile>` to register projects.",
      not_available_in_portal: "This operation stays with the eaeh CLI; see the CLI-to-Portal parity inventory.",
      conflict: "The state changed or the request conflicts with existing records; reload and review.",
      rate_limited: "Too many requests in the last minute; the view refreshes automatically.",
      network_error: "Start it again with `eaeh portal` and reopen the printed link.",
    };
    return {
      title: e.status === 0 ? "Portal server unreachable" : `${e.status ? `${e.status} · ` : ""}${e.code.replaceAll("_", " ")}`,
      detail: e.message + (e.requestId ? ` (request ${e.requestId})` : ""),
      hint: hints[e.code],
    };
  }
  return { title: "Unexpected error", detail: e instanceof Error ? e.message : String(e) };
}
