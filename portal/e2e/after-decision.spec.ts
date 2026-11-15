import { expect, test } from "@playwright/test";

// Run AFTER a human approved the Requirements gate in the portal:
//   EAEH_PORTAL_URL=… EAEH_E2E_AFTER_REQUIREMENTS_APPROVAL=1 npm --prefix portal run e2e -- after-decision
// Verifies that the decision went through the approval service and the existing workflow
// engine resumed (HLD generated, HLD gate pending). Read-only: it decides nothing.

const LAUNCH = process.env.EAEH_PORTAL_URL ?? "";
test.skip(!LAUNCH || process.env.EAEH_E2E_AFTER_REQUIREMENTS_APPROVAL !== "1", "run after a human approved the Requirements gate");

test("the engine resumed after the human decision and the HLD gate is pending", async ({ page }) => {
  const origin = new URL(LAUNCH).origin;
  await page.goto(LAUNCH);
  await expect(page.getByRole("table", { name: "Pending approvals" }).getByText("HLD")).toBeVisible({ timeout: 30_000 });
  await page.goto(origin + "/approvals?status=approved");
  const row = page.getByRole("table", { name: "Approval requests" }).getByRole("row", { name: /Requirements/ });
  await expect(row.first()).toBeVisible();
  await row.first().getByRole("link").last().click();
  await expect(page.getByText(/Approver:/)).toBeVisible();
  await expect(page.getByText(/Artifact hash:/)).toBeVisible();
  await page.goto(origin + "/activity");
  await expect(page.getByText("Approval granted").first()).toBeVisible();
});
