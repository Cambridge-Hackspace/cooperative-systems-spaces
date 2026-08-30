// Tier 10: the browser against the real stack.
//
// Distinct from playwright.config.ts, which is Tier 5 and drives a fake API
// through a Vite dev server. The difference is not the browser -- it is what is
// behind it, and that changes what the tier can honestly assert.
//
// TIER 5 injects faults and asserts the application copes. It can do that
// because it owns the world: `arm()` decides what the next response is.
//
// TIER 10 injects nothing, deliberately. It walks a world the earlier stages
// built -- users, roles, doors, rules, profile-config versions, the accumulated
// history of a full suite run -- and asserts the application holds up over it.
// A fault injected here would be a fault injected into a real server that other
// stages are also asserting against, and the finding would belong to whichever
// stage noticed first.
//
// So the oracle is inverted. Tier 5 asks "what does it do when the server
// misbehaves"; Tier 10 asks "does the server misbehave at all, and does the UI
// survive what it actually returns". The watchdog in tests/live/audit.spec.ts
// is what makes the second question mean something: any 5xx observed by the
// browser fails the test that saw it.
//
// No `webServer`. The stack is already up -- e2e/run.sh brought it up several
// stages ago -- and starting one here would point the audit at an empty
// database, which is the one thing this tier must not do.

import { defineConfig, devices } from '@playwright/test'

const BASE = process.env.CSS_BASE_URL ?? 'http://127.0.0.1:4399'

export default defineConfig({
  testDir: './tests/live',
  timeout: 45_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  // One worker, against a shared real server that other stages have also
  // written to. Parallel workers would interleave their own writes into the
  // world they are auditing and each would see the others' rows appear
  // mid-test.
  workers: 1,
  // `retries: 0`, for a different reason than Tier 5's.
  //
  // Tier 5 has no network and no shared database, so a flake there is a defect.
  // Here there genuinely is a network and a shared database, so the honest
  // argument for a retry is stronger -- and it is still refused, because a
  // retry would hide exactly the class of defect this tier is looking for. An
  // audit that passes on the second attempt has found something.
  retries: 0,
  forbidOnly: !!process.env.CI,
  reporter: [['list'], ['junit', { outputFile: 'test-results/playwright-live-junit.xml' }]],
  use: {
    baseURL: BASE,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'off',
    // Longer than the fake tier's default: this is a real server doing real
    // queries against a deliberately hostile cluster, and a timeout here should
    // mean "did not answer", not "answered slower than a fake".
    actionTimeout: 15_000,
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'mobile', use: { ...devices['Pixel 7'] } },
  ],
})
