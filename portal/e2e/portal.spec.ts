import { expect, test, type Page } from "@playwright/test";

// Browser E2E against a running `eaeh portal` with real EAEH data:
//   scripts/demo-portal.sh prepare && scripts/demo-portal.sh approve-skills (human)
//   EAEH_PORTAL_PORT=18190 scripts/demo-portal.sh start > /tmp/url  (keeps running)
//   EAEH_PORTAL_URL="$(head -1 /tmp/url)" npm --prefix portal run e2e
// These tests never confirm an approval decision: gate decisions are made by a human.

const LAUNCH = process.env.EAEH_PORTAL_URL ?? "";
const ORIGIN = LAUNCH ? new URL(LAUNCH).origin : "";

test.skip(!LAUNCH, "set EAEH_PORTAL_URL to the launch link printed by `eaeh portal`");

async function open(page: Page, path = "/") {
  await page.goto(LAUNCH);
  await expect(page.getByRole("heading", { name: "Dashboard" })).toBeVisible();
  if (path !== "/") await page.goto(ORIGIN + path);
}

test("opens directly on the dashboard without a login screen and drops the launch secret", async ({ page }) => {
  await open(page);
  expect(new URL(page.url()).searchParams.has("launch")).toBe(false);
  await expect(page.locator("input[type=password]")).toHaveCount(0);
  await expect(page.getByText(/login|sign in/i)).toHaveCount(0);
  await expect(page.getByText("Local development · no authentication")).toBeVisible();
});

test("dashboard shows real in-progress work and the pending Requirements approval", async ({ page }) => {
  await open(page);
  await expect(page.getByTestId("stat-Workflows in progress")).not.toHaveText("0");
  await expect(page.getByTestId("stat-Pending approvals")).not.toHaveText("0");
  const work = page.getByRole("table", { name: "In-progress engineering work" });
  await expect(work.getByText("engineering.design")).toBeVisible();
  await expect(work.getByText("Waiting for approval")).toBeVisible();
  await expect(work.getByText("Human approval")).toBeVisible();
  const pending = page.getByRole("table", { name: "Pending approvals" });
  await expect(pending.getByText("Requirements")).toBeVisible();
  await expect(page.getByRole("list", { name: "Recent activity" }).getByText("Approval requested").first()).toBeVisible();
});

test("a reviewer can open and read the requirements document with its metadata", async ({ page }) => {
  await open(page, "/approvals");
  await page.getByRole("table", { name: "Approval requests" }).getByRole("link", { name: /Review/ }).first().click();
  await expect(page.getByRole("heading", { name: /Requirements review/ })).toBeVisible();
  await expect(page.getByText("Artifact hash", { exact: true }).first()).toBeVisible();
  await expect(page.getByRole("button", { name: /^Copy [0-9a-f]{64}$/ }).first()).toBeVisible();
  const doc = page.getByTestId("document-content");
  await expect(doc).toBeVisible();
  await expect(doc.locator("h1, h2").first()).toBeVisible();
  await page.getByRole("tab", { name: "Validation & policy" }).click();
  await expect(page.getByText(/requirements:passed/).first()).toBeVisible();
  await expect(page.getByRole("form", { name: "Approval decision" }).getByText(/Deciding as approver/)).toBeVisible();
});

test("decision form requires comments for rejection and asks for confirmation (cancelled here)", async ({ page }) => {
  await open(page, "/approvals");
  await page.getByRole("table", { name: "Approval requests" }).getByRole("link", { name: /Review/ }).first().click();
  await page.getByRole("button", { name: /Reject/ }).click();
  await expect(page.getByText("Explain why the document is rejected.")).toBeVisible();
  await page.getByRole("button", { name: /^Approve$/ }).click();
  const dialog = page.getByRole("alertdialog");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText(/Approve Requirements v\d+\?/);
  await expect(dialog).toContainText(/[0-9a-f]{64}/);
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toBeHidden();
  await page.goto(ORIGIN + "/approvals");
  await expect(page.getByRole("table", { name: "Approval requests" }).getByText("Pending").first()).toBeVisible();
});

test("workflow detail shows the lifecycle from the definition with tasks and agents", async ({ page }) => {
  await open(page);
  await page.getByRole("table", { name: "In-progress engineering work" }).getByRole("row").nth(1).click();
  const life = page.getByRole("list", { name: "Workflow lifecycle" });
  await expect(life.getByRole("button", { name: /Approve requirements: Waiting for approval/ })).toBeVisible();
  await expect(life.getByRole("button", { name: /Brd: Completed/ })).toBeVisible();
  await expect(life.getByRole("button", { name: /Hld: Not started/ })).toBeVisible();
  await life.getByRole("button", { name: /Analyse/ }).click();
  const tasks = page.getByRole("table", { name: "Task executions" });
  await expect(tasks.getByText("Completed").first()).toBeVisible();
  await expect(tasks.getByText(/@\d+\.\d+\.\d+/).first()).toBeVisible();
  await life.getByRole("button", { name: /Approve requirements/ }).click();
  await expect(page.getByRole("link", { name: "Review document" })).toBeVisible();
});

test("projects, activity and settings show real data; audit respects audit.read", async ({ page }) => {
  await open(page, "/projects");
  await page.getByRole("table", { name: "Projects" }).getByText("simple-springboot-api").click();
  await page.getByRole("tab", { name: "Artifacts" }).click();
  await expect(page.getByRole("table", { name: "Engineering artifacts" }).getByText("design/requirements", { exact: true })).toBeVisible();
  await page.goto(ORIGIN + "/activity");
  await expect(page.getByText("Workflow started").first()).toBeVisible();
  await page.getByRole("tab", { name: "Audit log" }).click();
  await expect(page.getByText("The viewing identity cannot read the audit log")).toBeVisible();
  await page.goto(ORIGIN + "/settings");
  await expect(page.getByText("Viewing identity (--as)")).toBeVisible();
  await expect(page.locator("dd", { hasText: /^deterministic$/ })).toBeVisible();
});

test("useful errors and empty states", async ({ page }) => {
  await open(page, "/workflows/00000000-0000-7000-8000-000000000000");
  await expect(page.getByRole("alert")).toContainText(/not found/i);
  await page.goto(ORIGIN + "/approvals?status=rejected");
  await expect(page.getByText("No approvals match these filters")).toBeVisible();
});

test("without the launch secret the portal explains how to open it and exposes no data", async ({ browser }) => {
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  await page.goto(ORIGIN + "/");
  await expect(page.getByText("Open the portal from your terminal")).toBeVisible();
  const res = await page.request.get(ORIGIN + "/api/v1/dashboard");
  expect(res.status()).toBe(401);
  await ctx.close();
});

test("a completed design run shows gate-by-gate approval evidence and that it was not AI-generated", async ({ page }) => {
  await open(page, "/workflows?status=all");
  await expect(page.getByRole("table", { name: "Workflows" }).getByRole("row").nth(1)).toBeVisible();
  const rows = page.getByRole("table", { name: "Workflows" }).getByRole("row").filter({ hasText: "Completed" });
  test.skip((await rows.count()) === 0, "no completed run in this installation yet");
  await rows.first().click();
  const gates = page.getByRole("table", { name: "Gate verification" });
  for (const g of ["Approve requirements", "Approve hld", "Approve lld"]) {
    await expect(gates.getByRole("row", { name: new RegExp(g) }).getByText(/Verified approval|Approval invalid/)).toBeVisible();
  }
  await expect(gates.getByText("Approval invalid")).toHaveCount(0);
  await expect(page.getByTestId("completion-seal")).toBeVisible();
  await expect(page.getByRole("note").filter({ hasText: "not AI-generated" })).toBeVisible();
  await page.getByRole("list", { name: "Workflow lifecycle" }).getByRole("button", { name: /^Hld:/ }).click();
  await expect(page.getByText("Recorded fixture – not AI")).toBeVisible();
});
