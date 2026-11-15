import { defineConfig, devices } from "@playwright/test";

// Browser E2E against a real `eaeh portal` with real EAEH data. The fixture script
// (e2e/start-portal.sh) prepares a deterministic-mode installation and prints the launch
// URL; see docs/implementation/EAEH-GOVERNANCE-PORTAL.md.
export default defineConfig({
  testDir: "./e2e",
  timeout: 60_000,
  fullyParallel: false,
  workers: 1,
  reporter: [["list"]],
  use: {
    ...devices["Desktop Chrome"],
    // `npx playwright install chromium` downloads the bundled browser; where that is not
    // possible, EAEH_E2E_CHANNEL=chrome uses the locally installed Google Chrome.
    ...(process.env.EAEH_E2E_CHANNEL ? { channel: process.env.EAEH_E2E_CHANNEL } : {}),
    trace: "retain-on-failure",
  },
});
