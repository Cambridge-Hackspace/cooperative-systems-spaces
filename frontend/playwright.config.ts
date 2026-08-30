// Tier 5: the browser against a fake API.
//
// WHERE THIS RUNS. Never on the FreeBSD workstation — no Playwright browser
// build exists for it. In a reaper session and in CI it runs out of the pinned
// `mcr.microsoft.com/playwright` image, which carries its own browsers, which is
// why `PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1` is set everywhere else.
// TESTING.md §3 is the table of what runs where.
//
// `retries: 0`, deliberately. The fake has no clock skew, no network and no
// shared database: every input is under the test's control. A flake here is a
// defect — most likely a race between the app's own promises — and a retry
// would turn it into noise nobody investigates. If something is genuinely
// timing-dependent, the fix is `hangNext` and an explicit wait, not a retry.

import { defineConfig, devices } from '@playwright/test'

const PORT = 5199
const BASE = `http://127.0.0.1:${PORT}`

export default defineConfig({
  testDir: './tests/e2e',
  // Long enough for a cold Vite transform on first navigation, short enough
  // that `hangNext` fails a test rather than the whole run.
  timeout: 30_000,
  expect: { timeout: 5_000 },
  fullyParallel: false,
  // The fake holds one world in one process, so parallel workers would share
  // it. One worker is the honest arrangement; the suite is small enough that
  // the wall-clock cost is not worth a per-worker fake.
  workers: 1,
  retries: 0,
  forbidOnly: !!process.env.CI,
  reporter: process.env.CI
    ? [['list'], ['junit', { outputFile: 'test-results/playwright-junit.xml' }]]
    : [['list']],
  use: {
    baseURL: BASE,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'off',
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    {
      // A phone viewport, because DoorCheckinView is a QR flow: it is opened by
      // a camera app on a handset and by nothing else. Testing it only at
      // 1280x720 tests the one size it is never used at.
      name: 'mobile',
      use: { ...devices['Pixel 7'] },
    },
  ],
  webServer: {
    command: 'npx vite --config vite.fake.config.ts',
    url: BASE,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
    stdout: 'pipe',
    stderr: 'pipe',
  },
})
