import { render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { SessionGate } from "@/components/SessionGate";
import { mockFetch, session } from "./helpers";

const wrap = (ui: React.ReactNode) =>
  render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>{ui}</QueryClientProvider>);

describe("SessionGate (no login)", () => {
  it("exchanges the launch secret once and removes it from the address bar", async () => {
    window.history.replaceState(null, "", "/?launch=s3cret");
    const m = mockFetch({ "/portal/session": session() });
    wrap(<SessionGate><p>dashboard</p></SessionGate>);
    expect(await screen.findByText("dashboard")).toBeInTheDocument();
    const post = m.calls.find((c) => c.init?.method === "POST");
    expect(JSON.parse(String(post?.init?.body))).toEqual({ launch: "s3cret" });
    expect(window.location.search).toBe("");
  });

  it("does not ask for the session before the cookie exists (real server ordering)", async () => {
    window.history.replaceState(null, "", "/?launch=s3cret");
    let cookie = false;
    const fetchMock = vi.fn(async (_url: RequestInfo | URL, init?: RequestInit) => {
      if (init?.method === "POST") {
        await new Promise((r) => setTimeout(r, 30));
        cookie = true;
        return new Response(JSON.stringify({ ok: true }), { status: 200 });
      }
      return cookie
        ? new Response(JSON.stringify(session()), { status: 200 })
        : new Response(JSON.stringify({ error: { code: "portal_session_required", message: "no session" } }), { status: 401 });
    });
    vi.stubGlobal("fetch", fetchMock);
    wrap(<SessionGate><p>dashboard</p></SessionGate>);
    expect(await screen.findByText("dashboard")).toBeInTheDocument();
  });

  it("explains how to open the portal when there is no session", async () => {
    window.history.replaceState(null, "", "/");
    mockFetch({ "/portal/session": [401, { error: { code: "portal_session_required", message: "open the portal with the link" } }] });
    wrap(<SessionGate><p>dashboard</p></SessionGate>);
    await waitFor(() => expect(screen.getByText(/Open the portal from your terminal/)).toBeInTheDocument());
    expect(screen.queryByText("dashboard")).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/password/i)).not.toBeInTheDocument();
  });
});
