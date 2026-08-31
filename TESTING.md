# Testing

How this project is tested, what each layer can and cannot see, where each one
runs, and what is *not* covered yet.

The organizing idea is from
[`calebpower/reaper`'s testing methodology](https://github.com/calebpower/reaper/blob/main/docs/testing-methodology.md):
not a pyramid but a **portfolio of oracles**. Every tier earns its place by
answering a question no cheaper tier can answer. A tier that cannot do that is a
tier to delete.

---

## 1. The short version

```sh
# Frontend — runs on any machine with Node, including the FreeBSD workstation
cd frontend
npm ci
npm run format:check && npm run lint && npm run type-check && \
  npm run type-check:strict && npm test

# The cheapest tier, and the one that has found the most. No server crate, no
# database, no toolchain beyond cargo: it reads source as data.
cargo test -p css-checks

# The suite's own code, plus tier 9's oracle. Needs shellcheck, shfmt and node.
./e2e/lint.sh

# Rust — see §3. On FreeBSD css-server does not build; use a reaper session.
cargo fmt --all -- --check
cargo test --locked --all-targets

# The whole battery on a disposable Linux machine
reaper test          # sync, build, reset, run
reaper down          # when you are finished with the session
```

**If you read one thing:** §2 is the tier-by-tier status, and it says which
tiers are running, which are written but unexercised, and which do not exist.
§8 is the list of defects the suite records rather than fixes — each pinned by
an assertion, so it fails the day somebody fixes it.

---

## 2. What is actually covered today

Honest status, tier by tier. "Written but unexercised" is called out where it
applies, because a suite nobody has watched pass is a suite of unknown value.

| Tier | Question only it answers | State |
|---|---|---|
| 1 Pure unit | Is this calculation right, at its boundaries? | **Substantial.** From 44 Rust tests and 0 TypeScript to 255 `#[test]`s across the workspace and 96 in `frontend/tests/unit`. About a third of the Rust tests run on the workstation; the rest need Linux. The most recent addition is the MFA primitives (+39 Rust, +15 TypeScript): the challenge store's single-use guarantee, the TOTP skew window pinned at a fixed instant so it asserts the tolerance rather than the clock, recovery-code hashing, the fifteen-cell enrollment-enforcement table, and the auth store's challenge branch — none of which had a single test before. |
| 1b Cross-implementation vectors | Do two independent implementations agree? | **Started.** `contracts/door_rules.json` — 10 cases read by `server/tests/door_vectors.rs` and `edge/tests/door_vectors.rs`, with the edge half fed from the server's *declared* output. It found the inactive-member divergence. The five ToolGuard wire-type copies are not unified, but `checks/tests/toolguard_wire_types.rs` now records exactly how they disagree and fails on a sixth. `wire_kinds.json` is not written. |
| 2 Component conformance | Did the rendered output drift? | **Complete.** All 40 components have a spec, plus three views: 44 files, 865 cases (`LoginView` is the newest, and the first coverage the login form has ever had). Every assertion mutation-checked, each mutation guarded so one that fails to apply errors rather than passes. It produced roughly forty findings, listed in §8 grouped by cause; the largest is that ToolTrainingModal's three action buttons cannot render at all. |
| 3 Source-as-data | Does the code's structure still hold its claims? | **Substantial.** 63 cases in `checks/`, plus 26 in `frontend/tests/structure/` — including four ratchets the tier-2 sweep motivated: the audit-log filter against the server enum, the components nothing imports, the frontend stubs, and the database writers that discard their row count. This tier has found more real defects than any other, and the whole crate runs in under a second on any host — including the one where `css-server` cannot be built at all. |
| 4 Server contract | Do the authorization rules hold, in isolation? | **Complete for what it can reach.** 991 route × credential pairs asserted in-process against a deliberately dead pool, plus the 24 device pairs it explicitly defers, which the stack tier asserts. |
| 5 Browser vs fake API | What does the app do when a request *fails*? | **Running.** 24 specs across two viewports, green. A fake API as a Vite middleware — so it imports the real validator and shares one origin with the real bundle — with four injection shapes. It found the config-shape freeze that no other tier could see, and getting `abortNext` to actually abort took three attempts: Chromium retries an idempotent GET when a connection closes before any bytes, so only a *truncated* response is a real transport failure. The fake now models an MFA-enrolled user, which is what lets the browser tier assert that a reload mid-challenge lands back on the login form rather than inside the application. |
| 6 Full stack | Does it work against a real database, broker, charset? | **Running, green.** Thirteen stages: preflight, up, schema, restart, contract, mfa, fuzz, concurrency, health, devices, browser, logs, down. Postgres LATIN1 / lc_collate=C / lc_ctype=C, `TZ=America/Chicago`, mosquitto, and the real release binary. It found the migration this schema could not apply, the 401-for-a-role defect, and the 404 on every deep link. `devices` runs both edge binaries, which is the only way to exercise a `#[cfg]` branch; `logs` treats the server's own ERROR output as an oracle. `mfa` is the only stage that can answer whether a second factor actually gates the JWT, and it generates its TOTP codes from a second, independent implementation of RFC 6238 checked against the specification's own vectors — so an accepted code is two implementations agreeing, not the server agreeing with itself. |
| 7 Seeded fuzz | Does any ordinary-but-untried request crash it? | **Running.** Three oracles over all 164 endpoints, seeded and replayable. |
| 8 Concurrency | Does the invariant survive simultaneous writers? | **Running.** Both known races, each asserted on the resource and paired with a sequential sibling. |
| 9 Simulated users | What breaks only after history accumulates? | **Running.** A seeded driver takes 200 weighted actions through the shipping API — registrations, role changes, deactivations, deletions, door rules, profile-config writes, and three nemesis classes in the same pool — maintaining a shadow model and checking all six invariants every 20 actions. A recent run: 29 users, 24 door rules, 10 checks, no violation. Two of the six invariants cannot currently mean what they were written to mean, and say so rather than passing quietly: `deactivations-held` is vacuous because deactivated users are not listed at all, and `invites-are-single-use` can only check its count half because nothing links a device to its invite. Both are §8 findings, not test debt. |
| 10 Live browser audit | Does the UI hold up over a world somebody else built? | **Running.** 22 tests across desktop and phone viewports, against the real server over everything the earlier stages created. Injects nothing — that is Tier 5's job and doing it here would produce findings belonging to whichever stage noticed first. The oracle is a watchdog: every test records what the browser actually received and fails on any 5xx or uncaught page error, so a server error on a page that still looks fine is caught. The watchdog self-tests, because every other assertion in the file passes by it staying silent. This tier also owns the only completed WebAuthn ceremonies in the repository: `passkey.spec.ts` attaches Chromium's virtual authenticator over CDP and enrolls a passkey, then signs in with it, so `finish_passkey_registration` and `finish_passkey_authentication` are verified against real signatures. |
| 11 Human evidence | Does this make sense to a newcomer? | **Running.** Two halves. The contrast audit: WCAG relative luminance over all fourteen themes, OKLCH converted for daisyUI's built-ins, the reference implementation checked against three known answers — it found **36 semantic/base pairings below AA**, pinned as a ratchet. And the transcript: the journey driver records what a person would have been shown at each step, and a zero-dependency reader renders it as prose plus every distinct message with how often and to whom. It asserts almost nothing on purpose — the question has no oracle — but it made a real finding on its first run (§8, the generic conflict message). Runs on the workstation, where `css-server` cannot be built. |

**Formatting and linting are complete and gating.** `rustfmt`, `prettier`,
`eslint` (type-aware, flat config), `shellcheck` and `shfmt` all pass, and CI
fails on any of them. A `[Vue warn]` during a component test is a test failure,
with no allowlist.

**Where the findings came from.** Worth recording, because it says where to
spend the next hour:

| Tier | Real defects it found |
|---|---|
| 3 Source-as-data | The unauthenticated ToolGuard endpoints; four broken CLI paths; the `UserRole` wire drift; the duplicate migrations root; the two divergent error conversions; five diverged copies of one wire format |
| 1 / 2 Unit and component | The unreachable training warning; the iCal `v-html`; the roster refresh that never refreshed; the roster error banner that destroyed the list; and, from the full sweep, the four causes in §8 — three UI fields the server has no column for, five components that report success or "no data" on a refusal, three date pickers floored in UTC, and the training flow whose buttons key off aliases nothing populates |
| 1b Vectors | Both door fail-open sites; the inactive-member divergence between the RFID and QR paths |
| 6 Full stack | The migration no non-UTF-8 database can apply; the 401-for-an-insufficient-role, which logs the user out; the config loader exiting 0 after refusing to start; **a 404 on every deep link, including the QR door URL** |
| 7 Seeded fuzz | Two different error envelopes across every guarded route; six handlers answering 500 for a row that does not exist; a 500 for a repository nobody configured; four routes that can never succeed |
| Build warnings, read rather than silenced | A `dsl::*` glob shadowing a function's parameters, so an UPDATE deactivated **every** trainer assignment in the table; the training-history filters passed and never read |

The last row is worth its own note. Every one of those was a plain rustc
warning — "unused variable", "field is never read" — sitting in the build
output. Fixing them properly rather than prefixing with an underscore is what
turned three warnings into two real defects and a vestigial dependency. A
warning is a question the compiler is asking; `_` is a way of not answering it.

---

## 3. Where things run, and why the split exists

Three environments, and the split is forced by facts rather than preference.

### The workstation (FreeBSD)

Runs: the whole frontend suite, `checks/`, and the `css_lib`, `css-cli` and
`css-edge` crates.

Does **not** run `css-server`, and cannot. `dr-metrix-axum 0.1.0` calls
`prometheus::process_collector::ProcessCollector::for_self()` unguarded, and
`prometheus` gates that module behind
`all(feature = "process", target_os = "linux")`. This is not something to work
around locally; the server builds in a session or in CI.

Does not run any browser tier either: Playwright publishes no FreeBSD browser
build.

> **npm on this host is broken** and it is not this project's fault: the
> packaged npm's own vendored `@sigstore/sign` tree is missing `imurmurhash`,
> so `npm ci` dies with `MODULE_NOT_FOUND`. Work around it with corepack, which
> fetches a package manager without using the broken one:
>
> ```sh
> corepack npm@11.6.2 ci --no-audit --no-fund
> ```
>
> Nothing in the repository depends on this; CI and the reaper session have a
> working npm.

### A reaper session (`ubuntu-26.04`)

The pre-push loop, and the only place the full battery runs on hardware you can
throw away.

```sh
reaper up          # ~4 min cold: clones a machine, builds its pool, pulls images
reaper test        # sync -> build -> reset -> run
reaper down        # destroy it; results come back on the way out
```

`build` runs the entire non-containerized Rust battery — fmt, tests, doc tests,
a full-workspace check including the bevy and egui crates — and produces the
binaries `run` needs. A failing unit test therefore fails the *build* verb,
before a session spends any time bringing a stack up, and `@pristine` is never
taken on the strength of a suite that did not pass.

Two operational notes learned the hard way:

- **Do not wrap `reaper up` in `timeout`, and do not background it through a
  process-group-managing harness.** `up` leaves a heartbeat process behind that
  renews the session's expiry; kill it and the sweeper collects the machine on
  schedule. One session was lost this way during development.
- **`run.cmd` must never contain a pipe.** reaper hands it to `/bin/sh`, which
  is dash, and dash has no `pipefail`: `run.sh | tee log` exits with *tee's*
  status, so a failing suite reports as a pass and `@pristine` is then taken on
  the strength of that false pass. `e2e/reaper-run.sh` exists to own the
  pipeline under bash. This is not hypothetical — the same trap produced a false
  "the server compiles" reading while this work was being planned, from
  `cargo check … | tail`.

### GitHub Actions

**The gate, and it stands alone.** Nothing in `.github/workflows/css-ci.yml`
depends on reaper existing. The `stack` job gets its Postgres from a
`services:` container and runs `e2e/run.sh --provision=external`, which takes
its endpoints from the environment rather than starting anything itself. That
`--provision` flag exists precisely so that reaper is an accelerator and never a
dependency.

**Which tiers actually run here.** Not all of them, and the differences are
deliberate rather than incidental:

- **Tier 9 (`journeys`) runs.** It needs no browser, so `--provision=external`
  exercises it in full — 15 cases on the last run.
- **Tier 11 (`evidence`) runs.** The reader is zero-dependency and takes the
  journey transcript as a file, so it works wherever node does. CI reaches it
  through `actions/setup-node` rather than the checksum-pinned bootstrap, which
  is a different path through `ensure_node` and worth knowing is exercised.
- **Tier 10 (`audit`) does NOT run here**, and records a skip saying so. It
  needs a browser *and* the full stack. The `browser-fake` job has a browser and
  no stack; the `stack` job has a stack and no browser. Tier 10 is therefore
  reaper-and-workstation only, and a green CI is not evidence about it.

That last one is the asymmetry to remember when reading a green pipeline: CI
covers eleven of the twelve tiers, and the twelfth says so out loud rather than
passing quietly.

**Which machine.** The Linux jobs run on `${{ vars.CI_RUNNER || 'ubuntu-latest' }}`.
Unset means GitHub-hosted, which is what a fork or a clone gets with no
configuration at all. Set the repository or organization variable `CI_RUNNER` to
a self-hosted label to send them elsewhere; this project's upstream uses
`arc-runner-set`.

**What that variable is worth knowing about.** It was a hardcoded
`arc-runner-set` until this branch, and self-hosted runners do not cross a fork
boundary -- a fork cannot claim its parent's runners, because that would let
anyone who forked the repository run code on the parent's infrastructure. So on
this fork every job queued against a runner that could never take it and was
canceled silently twenty-four hours later. The Actions tab stayed empty through
a merge to `master`. A workflow that cannot be claimed does not fail; it simply
never reports, which is the failure mode hardest to notice and the reason this
is a variable now rather than a label.

**Status.** It has now run. The first execution in this repository's history
was a `workflow_dispatch` on 2026-08-27, and it failed in three places, all of
them real:

- `frontend-edge` called `npm run type-check` in a directory that had no such
  script. The second frontend had none of the tooling the first one got, and the
  job was written as though it did. It now has all of it: prettier, eslint with
  type-aware rules, `type-check` and `type-check:strict`, every one of them run
  by CI. Adding the linter immediately found three floating promises in
  `StatusView.vue` -- `loadStatus()` unawaited in `onMounted`, in a
  `setInterval` and in a `setTimeout`. None is a live bug, because `loadStatus`
  handles its own rejections, and all three are now explicit `void`.

  A note on the workstation, because it shaped this: npm here is broken --
  `Cannot find module 'imurmurhash'`, absent from npm's own bundled
  `node_modules` under `/usr/local/lib/node_modules/npm` -- so no dependency can
  be installed with it. `corepack npm@11.6.2` fetches a working npm into a user
  cache and was used instead. The system npm is still broken and worth repairing.
- `shell` failed `shellcheck` on `server/test_auth.sh` (SC2236, `! -z` for
  `-n`). The same script passes on the workstation, because neither linter is
  pinned and 0.11.0 does not raise it at `--severity=style` while the runner
  image's version does. `e2e/lint.sh` now prints both tool versions so the
  difference is legible in both logs rather than a contradiction to reproduce.
- `stack` never got a broker. `--provision=external` started no mosquitto at
  all, while `up` recorded "the caller supplied postgres and mqtt" -- a claim
  nothing verified. `MqttService::new` connects during boot and `main.rs`
  propagates, so css-server exited before binding and six stages produced
  fifteen connection-refused cases against a port nothing would ever listen on.
  `start_mosquitto` had the host-process path for this all along; only the call
  site was inside the wrong branch.

`assets`, `frontend`, `rust` and `browser-fake` passed, which is the first
independent confirmation that the Rust tiers and the Playwright tier hold up on
a machine nobody here configured.

**The second run** fixed the first two and got the stack much further:
`preflight`, `up`, `schema`, `restart`, `contract`, `health`, `devices`, `logs`
and `down` all passed under `--provision=external` for the first time. `fuzz`
and `concurrency` failed, both on the same cause, and it was a defect in this
harness rather than in the application.

`run_node` builds the driver's environment separately in each branch -- a
command prefix on the host, `-e` flags into the container -- and the container
branch passed `CSS_DB_ENCODING` while the host branch did not. So in CI the
drivers read their own `UTF8` default and believed a LATIN1 cluster could store
anything. Both of them handle a non-UTF-8 database correctly: `fuzz.mjs` drops
the astral-plane corpus entries because they reproduce a known 500 on every
route that writes text, and `concurrency.mjs` records a pinned finding and skips
the invite race for the same reason. Neither ran. CI fired U+1F434 and U+1F967
at a LATIN1 cluster, got SQLSTATE 22P05 both times, and reported a defect
already documented in section 8 as a fresh fuzz finding.

The cost of that is not the red build. It is that every genuinely new finding
would have sat behind noise reproducing on every route that writes text.

Fixed by passing the variable on both paths, and by
`checks/tests/both_driver_paths_pass_the_same_env.rs`, which derives every
function in `e2e/stack.sh` that branches on the provisioning mode and asserts
the two sides hand the process the same environment. Deriving the list rather
than writing it down is the point: a fourth such function cannot be added
without the check seeing it, and a function that becomes unreadable to the check
fails rather than being skipped.

Two things that check found on the way, both recorded because neither was the
bug being chased. `start_edge` set `CONFIG_PATH` on the host path only --
harmless, because `edge/src/main.rs:98` sets that variable itself from
`--config`, which both paths pass; removed as dead rather than exempted. And
`sql_ro` was *reported* as diverging on `PGOPTIONS` by the first version of the
check, which was wrong: it writes two assignments on one line and the parser
took only the first. The suite's read-only database guarantee was never absent.
A check that had shipped in that state would have made a false claim about a
safety property, which is worse than the bug it was written for.

**The pipeline is green.** Every job, on run 33042472647: assets, frontend,
frontend-edge, shell, rust, stack, browser-fake, and all three release legs
including Windows. `docker-server` and `docker-edge` correctly report as skipped
rather than running to authenticate and do nothing. This is the first fully
green run in the repository's history, and it took seven attempts to get there.

Four of those seven found something real:

1. A hardcoded self-hosted runner label that no fork could claim, which is why
   the workflow had never executed at all.
2. `frontend_edge` calling an npm script it did not have, because that directory
   had never been given the tooling `frontend` got.
3. `run_node` passing `CSS_DB_ENCODING` on one provisioning path and not the
   other, so the drivers believed a LATIN1 cluster could store anything.
4. A trainer removal that removed nothing, answered 200, and then wrote an audit
   record for a user that does not exist -- section 8.

The Windows release leg took four of the seven on its own, each failing one
layer deeper than the last: no OpenSSL, then the wrong OpenSSL version, then the
right version in an unexpected directory layout, then a Visual Studio the pinned
`cc` could not recognize. Three of those four were fixed by upgrading a
dependency the lockfile had pinned for reproducibility rather than for any
reason anybody chose -- `openssl-sys` 0.9.109 to 0.9.117, `cc` 1.2.40 to 1.4.4,
`cmake` 0.1.54 to 0.1.58. The fourth was fixed by searching for the import
libraries instead of assuming their layout.

None of that was findable from here. The workstation is FreeBSD, there is no
Windows machine, and every hypothesis cost a full CI run to test -- which is why
the OpenSSL step prints what it found and fails with a directory listing when it
cannot, and why a `vswhere` step reports the toolchain on every Windows run.

---

## 4. The commands, by layer

| What | Command | Where |
|---|---|---|
| Rust format | `cargo fmt --all -- --check` | anywhere |
| Rust tests | `cargo test --locked --all-targets` | session / CI (server needs Linux) |
| Structural checks only | `cargo test -p css-checks` | anywhere — seconds, no server dependency |
| Frontend format | `npm run format:check` | anywhere |
| Frontend lint | `npm run lint` | anywhere |
| Frontend types | `npm run type-check` | anywhere |
| Frontend strict ratchet | `npm run type-check:strict` | anywhere |
| Frontend tests | `npm test` / `npm run test:coverage` | anywhere |
| e2e suite's own code | `./e2e/lint.sh` | anywhere with shellcheck + shfmt |
| Stack battery | `./e2e/run.sh --provision=podman\|docker\|external` | session / CI |
| One stage | `./e2e/run.sh --only schema` · `--list-stages` | session / CI |
| Replay a fuzz seed | `CSS_FUZZ_SEED=1234 ./e2e/run.sh --only up,fuzz` | session / CI |
| Deeper fuzz | `CSS_FUZZ_ITERATIONS=5000 ./e2e/run.sh --only up,fuzz` | session / CI |
| Harder race | `CSS_RACE_FANOUT=32 CSS_RACE_ROUNDS=10 ./e2e/run.sh --only up,concurrency` | session / CI |
| A harsher cluster | `CSS_E2E_DB_ENCODING=SQL_ASCII ./e2e/run.sh` | session / CI |
| The whole loop | `reaper test` | workstation |

### Replaying a fuzz finding

The seed is printed as the first line of `logs/fuzz.log`, recorded in the
stage's JUnit `<properties>`, and restated as its own case so it is visible in
a summary that shows nothing else.

**It reproduces the sequence of decisions, not the run.** Entity ids differ
between runs, so a replayed seed follows a *similar* path rather than an
identical one — which is stated here rather than implied, because a seed
advertised as a reproduction and delivering a near-miss wastes more time than no
seed at all. What it does reproduce reliably is which endpoint, which corpus
entry and which credential each iteration chose.

What a seed cannot give you, every finding carries anyway: the method, the full
path, the credential and the body, verbatim, in `stack/fuzz-findings.json` and
in the failure message. Reproducing by hand needs no seed and no replay.

### A note on the workstation

`npx` is broken on this FreeBSD host — the system npm's vendored
`@sigstore/sign` is missing `imurmurhash`. Run the binaries directly
(`node node_modules/vitest/vitest.mjs run`) or use
`corepack npm@11.6.2`. Nothing in the suite depends on `npx`, deliberately.

`shfmt` has no FreeBSD release binary; build one with
`GOBIN=~/.local/bin go install mvdan.cc/sh/v3/cmd/shfmt@v3.13.1`.
`e2e/lint.sh` fails rather than skipping when it is absent, because a lint that
reports a clean tree it did not check is worse than one that does not run.

`checks/` deliberately depends on **none** of the other crates. It reads text,
so it compiles and runs in seconds on any host — including the one where
`css-server` cannot be built at all, which is exactly where you want the
cheapest tier to work. A tier people skip is a tier that does not exist.

---

## 5. Reading the results

`e2e/run.sh` writes to `$REAPER_OUT` when it has one and `e2e/out/` otherwise:

- `junit/<stage>.xml` — one file per stage, written in a `finally` so a stage
  that dies mid-way still leaves a file describing the failures it had.
- `logs/` — container logs, streamed rather than dumped at the end.
- `e2e.log` — the whole run.
- `SUMMARY.md` — written last: stages run, stages skipped **and why**, seeds,
  and every narrowing in force.
- `RUN.txt` — which artifacts belong to *this* run. The backward sync never
  deletes, so without this a trace from three cycles ago sits beside a fresh one
  looking identical.

---

## 6. Rules this suite holds itself to

Straight from §2 of the methodology, and they are the reason several things
below look more laborious than they need to.

**Never weaken a test, check, assertion or lint to route around a defect.** No
disabled phase, no exclusion, no lowered threshold, no `#[ignore]`, no `skip`,
no `|| true`, no catch-and-swallow.

**Every narrowing carries a stated reason covering exactly what it narrows.**
The ones currently in force are listed in §9. They are in the summary, not
buried in a code comment.

**Every fix ships with a test that would have caught it**, or an explicit
statement of why it is untestable in isolation.

**A pre-existing failure is proven pre-existing.** Stash, re-run, name it. One
was: `edge/src/system_info.rs`'s platform test failed on FreeBSD before any of
this work, verified by stashing and re-running at `HEAD`.

**New assertions are mutation-checked.** Break the thing, watch the test fail,
restore. Every check in `checks/` was; so was the platform test. Where a
mutation check found the *test* wrong rather than the code, that is recorded in
the commit — `toolguard_auth.rs` caught two of three defects on its first
version and silently exonerated the third.

**Name what a test does not prove.** Several tests here exist to record a
limit rather than assert correctness; they say so in their own comments.

---

## 7. What is not covered, and what stands in the way

Being specific, because "not covered" without a reason is just a gap.

** Tier 5 now runs — 32 tests, two
viewports, green on the workstation, in a reaper session and in GitHub Actions —
so the thing that blocked this is gone. What is missing is the tier that points
those specs at the *real* stack over a world somebody else built, with a
watchdog failing any test that observes a 5xx. That is written down in the
design and not written in code.

**Tier 9's driver is written; two of its six invariants are unreachable.** The
driver runs and finds things — it produced the deactivated-roster finding below.
But `deactivations-held` judges an empty set, and `invites-are-single-use` can
only count. Both need a product change to become meaningful, so they are listed
in §8 rather than here.

** The invariant self-test was written
first, as the design demands: six invariants, 20 cases, each fed what a broken
server would send and required to fire. It runs with no stack at all, on every
push, and it passes. What does not exist is the driver that accumulates a real
world for those invariants to judge — so the oracle is currently a very
well-tested judge of nothing.

**Thirty-five of forty components have no Tier 2 spec.** Five do: the ones
carrying the fixes the acceptance test reverts, plus the roster. The rest are
covered only insofar as Tier 3 reads them as text.

**Tier 1b covers doors and nothing else.** `contracts/door_rules.json` is read
by two crates and found a real divergence. `wire_kinds.json` is not written, and
**six copies of the ToolGuard `SyncPayload` and `ToolStatus` types still
exist** — in `edge`, `kiosk`, both toolguard UIs and the server. They have
already diverged: `toolguard-test-ui` carries an extra `Unknown` variant and
`kiosk` types its tool id as `String` where everything else uses `Uuid`. Moving
them into `css_lib::toolguard` is the fix; a source-level check that
`assert_eq!(definitions_of("SyncPayload").len(), 1)` is the fallback if the GUI
crates cannot be built.

**The stack battery does not exercise several product areas.** There is no
`accounts`, `doors`, `webhooks`, `text` or `training` stage. The contract stage
covers authorization across the whole surface and the fuzz stage reaches every
endpoint with an oracle that knows nothing about any of them — so the *shapes*
are covered. What is not covered is whether a door unlock actually reaches the
edge, whether a webhook delivery arrives, or whether a training record survives
a round trip. Those need per-feature stages and each one is a day's work.

**121 handlers map a database error to a bare 500.** Ratcheted rather than
fixed, per §8. `checks/tests/database_errors_keep_their_meaning.rs` pins the
count per file so it can only go down.

**Four frontend calls hit routes that do not exist.** Found by
`checks/tests/route_parity.rs` and recorded there rather than repointed,
because in each case the server has a route that is *plausibly* the intended
target but takes a different shape, and matching a payload by guesswork would
replace a visible failure with a silent one:

- `trainingApi.addTrainingPrerequisite` POSTs `/api/training/prerequisites`;
  the server has `POST /api/training/steps/{step_id}/prerequisites`.
- `startTrainingSession` / `completeTrainingSession` POST
  `/api/training/progress/{userId}/{start,complete}`; the server has
  `/api/training/sessions/{start,complete}`.
- `getUsersForTraining` GETs `/api/trainers/users`, which does not exist — and
  already carries a 404 fallback, so somebody hit this, worked around it, and
  left the call in.

Because `api.ts` wraps every call in `.catch`, all four present to the user as a
generic "Failed to …" rather than anything naming a missing route.

**`toolpass-client` has two subcommands with no server counterpart.**
`add-user` and `remove-user` point at `/api/toolpass/v1/…`, and no
`/api/toolpass` router exists anywhere. They are left pointing at the path that
does not exist, deliberately: inventing a target would hide that the feature was
never built server-side.

**`POST /api/tools/user-training/{id}` returns 501.** The route is registered,
the frontend can call it, and it can never succeed. Recorded by the fuzz tier's
known-findings list rather than ignored.

**The WebAuthn ceremonies run against a virtual authenticator, not hardware.**
This was the one uncovered branch, and it is now covered:
`frontend/tests/live/passkey.spec.ts` attaches Chromium's virtual authenticator
over the DevTools Protocol and completes both ceremonies against the real stack,
so `finish_passkey_registration` and `finish_passkey_authentication` are
exercised for real. The keys are real P-256 keys, the signatures are real, and
`webauthn-rs` verifies them exactly as it does in production.

What remains narrowed is the device. A virtual authenticator is conformant by
construction, and real ones are not: the interesting failures with hardware are
vendor quirks — a key that refuses `credProtect`, one that reports UV it did not
perform, one whose attestation format the server does not know. None of that is
reachable without the hardware in the room, and no test suite substitutes for
trying a real key once.

Two smaller consequences worth writing down. The spec is Chromium-only, because
the virtual authenticator is a Chromium DevTools domain; both Playwright
projects in this repository are Chromium, so nothing is lost today, but adding a
WebKit or Firefox project would silently reduce what runs. And the passkey spec
loads the page from `localhost` while every other live spec uses `127.0.0.1` —
not a style difference: a browser refuses an rp_id that is not a suffix of the
page's domain, `WebauthnBuilder::new` refuses an origin that is not a domain at
all, and those two constraints meet at exactly one value.

**Clippy still does not run in CI.** The build is warning-free now — the last
four went with the `AuthError` response deletion, an unmutated lock guard and a
vestigial database handle — so `-D warnings` is finally possible. Turning it on
is its own unit of work, because `clippy::pedantic` on 19.6k never-linted lines
produces a commit carrying forty `#[allow]`s, which is the weakening this
methodology forbids wearing the costume of progress.

---

## 8. Known defects that tests record rather than fix

Every one of these is **pinned by an assertion on the current behavior**, not
left as a failing test. That is a deliberate choice and it is worth stating why:
a suite that stays red teaches people to ignore red, and within a month a
genuine regression is indistinguishable from the wallpaper. An assertion that
pins a defect in place fails the day somebody fixes it — which is exactly when
somebody should read it, confirm the fix, and delete the assertion. Each one
says so in its own failure message.

None of these was fixed here, and each says why.

### The frontend sweep: four causes, about forty faces

Writing a tier-2 spec for all forty components produced far more findings than
the tier usually does, and listing them one by one would suggest forty unrelated
problems. They are not. Four causes account for most of them, and each is fixed
in one place.

Every finding below is pinned in the named spec, so each fails the day it is
fixed. None was fixed here.

**Cause 1 — `utils/api.ts` swallows rejections, and callers do not check.**
Most of the typed client's methods end in `.catch(...)` returning
`{ success: false, error }`, so the promise resolves on failure. A caller that
does not read `.success` cannot tell a refusal from a success.

- `ToolCreateModal` and `ToolEditModal` `await` the write and emit `created` /
  `updated` regardless. A refused create or edit is announced as a success, the
  parent refreshes a list that has not changed, and nothing is shown.
- `ToolEventHistory` sets `events = response.data || []` without reading the
  flag, so a 403 renders as "No events recorded for this tool." — a confident
  wrong answer rather than a blank.
- `ToolTrainingCard` sets `error` and renders it nowhere, so a refused overview
  says "No training steps configured for this tool." while the header reads
  "Loading..." permanently.
- `ToolTrainingSetupModal` ignores the tool update, then creates steps anyway.
- Several components' `catch` blocks are unreachable for the same reason, and
  three of them read `err.response.data.message` where the envelope fills
  `error`, so even reached they would discard the server's words.

*Why not fixed:* the honest fix is at the client — stop swallowing, or return a
discriminated result callers cannot ignore — and it touches every call site.
That is one change to design, not fourteen patches.

**Cause 2 — `types/training.ts` describes a server that does not exist.**
`TrainingStep` invents `passing_score` and `is_active`, neither of which is a
column in `training_steps`, and renames `expires_after_days` to `expiry_days`.
`TrainingStepWithProgress` adds `progress` and `can_start` as aliases for
`user_progress` and `is_available`, and nothing populates an alias.

- `EditTrainingStepModal` sends four fields the server's update request does not
  declare; serde drops them and answers 200. The "Active (visible to users)"
  checkbox controls nothing that exists.
- `CompleteTrainingModal`'s entire validation keys off `passing_score`, so the
  score input never renders and the minimum-score guard never fires. An
  instructor cannot record a score on a modal built around recording one.
- `ToolTrainingSetupModal` collects a passing score and shows it back on its
  review page; it goes nowhere.
- **`ToolTrainingModal`'s Start, Mark Complete and Retry buttons key off
  `can_start` and `progress`, so none of them renders for anyone, on any step.**
  Fed the alias names, all three appear and work. This is the largest single
  finding of the sweep: the training flow exists on the server and the UI cannot
  reach it.
- `getStepStatusClass` reads `progress` while `getStepNumberClass` reads
  `user_progress`, so the two disagree about the same step — which is the
  evidence that one is reading the wrong field rather than both being wrong.

*Why not fixed:* aligning the types is mechanical, but deciding what
`passing_score` and `is_active` should *mean* — add the columns, or delete the
UI — is a product question.

**Cause 3 — user-facing dates computed in UTC.**
Five components build a date with `new Date().toISOString().split('T')[0]`,
which is the date in UTC rather than the user's date. West of UTC they disagree
for the last hours of every day.

- `EditTrainerModal`, `AssignTrainerModal` and `TrainerManagement` floor a date
  picker at the UTC date, so a trainer cannot be given an expiry of today.
- `RecordTrainingModal` defaults the *training date* to it, and floors the
  picker with the same value — so an instructor recording an evening session is
  handed tomorrow's date and nothing objects.
- `ToolTrainingModal`'s inline record form does the same.

Three of those also send a bare calendar date where `api/trainers.rs` declares
`Option<DateTime<Utc>>`, which serde cannot parse.

*Why not fixed:* one helper, five call sites, and a decision about whether the
site timezone or the browser's should win. The suite now pins
`TZ=America/Chicago` so the tests can tell the two apart at all —
`tests/unit/suite-environment.spec.ts`.

**Cause 4 — no `try/finally` around a busy flag — fixed in `ca54bea`.**
Fifteen sites across seven components set `saving`/`busy`/`loading` true,
awaited, and cleared it on the next line. A rejection stranded the flag and
disabled the control for the life of the page. `MfaSettings` had it in three
handlers at once, so a network error on Set up authenticator, Confirm or
Regenerate recovery codes locked the entire page with nothing on screen to say
why — while `addWebauthn`, in the same file, did the same work inside
`try/finally` and recovered correctly. Ten components had the sibling shape in
their loaders: no `try/catch`, so a rejected load spun forever and the rejection
escaped to an `app.config.errorHandler` that `src/main.ts` never sets.

*Fixed in one pass, as this entry said it should be:* every loader now catches,
every catch says something, and the sixteen pinned tests were rewritten to
assert the fix. Five silent refusals went with them — a refused places config
that looked like the module being switched off, a refused door-event history
that looked like a door nobody had ever opened, and three more.

*This entry is kept rather than deleted* because the shape recurs, and because
the ten loaders it names are why `src/main.ts` still has no
`app.config.errorHandler` — which is a gap in its own right.

### Frontend findings that stand on their own

Not attributable to the four causes above.

- **A `javascript:` URL can be saved as a homepage link.** Neither
  `HomeLinkManagement` nor `api/home_links.rs:239` checks the scheme, and
  `HomeView.vue:85` renders it as `:href`. Vue does not sanitise an href
  binding, so it becomes a live script handler on the *public* home page. Admin
  to set, but it turns "an admin may curate links" into persistent script
  execution against every visitor.
- **A webhook URL is an SSRF read primitive.** `api/webhooks.rs:166` validates
  only the scheme. The server fetches the URL and stores the response:
  `WebhookDelivery.response_body` is returned to the admin UI. Pointing a
  webhook at a link-local or loopback address and firing a test delivery reads
  it back out of the Deliveries tab. Admin-gated, and webhooks legitimately
  point anywhere — so this is a design to decide on rather than an outright
  bug, but it should be decided.
- **`ScheduleManagement` edits and deletes the wrong window.** The editor sorts
  each day's windows by start time; the handlers map the clicked row back to the
  array by counting in array order. When the two disagree, the × removes a
  different window than the one it sits beside, and an edit lands on a window
  the user never touched — producing an invalid interval the validator then
  reports by name.
- **`DoorManagement` posts a rule that can never match.** The rule kind and
  value are separate refs and nothing resets the value when the kind changes, so
  switching from `role` to `user` posts a user rule whose value is the string
  "Member".
- **`ProfileConfigAdmin` does not show the configuration.** Every display reads
  `editConfig`, which is empty until Edit is pressed, so the page claims
  profiles are disabled and no fields exist whatever the server returned.
- **`TrainerManagement`'s Activate button is unreachable.** `includeInactive` is
  set nowhere, so the list never contains an inactive trainer, and Activate
  renders only for one.
- **`AuditLogTable` cannot page or filter.** The endpoint returns no total, so
  `totalPages` is always 1 and the pagination block never renders — an admin
  sees the fifty most recent events and has no control that reaches page two.
  The filter offers 9 of the server's 66 event types, missing every door,
  device and MFA event.
- **`PrerequisitesModal`'s two write paths both address the wrong thing.** Add
  posts to a route that does not exist; remove sends a `training_steps` id where
  the server deletes from `training_prerequisites`, matching nothing and
  answering 200.
- **`AssignTrainerModal` never announces success**, and `ToolTrainingCard`
  carries a red debug banner — both in components nothing imports, recorded in
  `tests/structure/components-are-reachable.spec.ts` so the specs cannot claim
  users are hitting them.
- **Two components render a `console.log` per step per render**, and
  `DeviceManagement` reports five of six write failures through `alert()` with
  the axios message rather than the server's.

### A mistyped MFA code costs the whole login, and nothing says so

`verify_login` calls `take_login` at the *top* of the handler, before it looks
at the code — so the challenge is consumed whether the code was right or wrong.

**Burning it is the correct security choice.** It is the only thing between a
captured `challenge_token` and an unlimited grind through a six-digit space:
`/api/auth/mfa/verify` has no rate limit of its own, and adding one is strictly
more machinery than destroying the challenge.

The defect is that the frontend does not reflect it. `LoginView` shows the
server's "Invalid TOTP code" and leaves `pendingMfa` set, so the user is looking
at a form with a cursor in it and a token that has already been destroyed. The
retry any human would make then fails with **"Unknown or expired
challenge_token"** — a different message, and a misleading one: nothing expired,
and there is no way for the user to learn that one wrong digit sent them back to
the password prompt.

*Why not fixed:* the repair is a product decision rather than a typo. Either the
view clears `pendingMfa` on a rejected code and says "that code was not
accepted, please sign in again" — honest, and the smaller change — or `/verify`
allows a bounded number of attempts against one challenge, which is kinder and
costs a rate limiter. Pinned from both sides: `e2e/drivers/mfa.mjs`
(`mfa/a-wrong-code-destroys-the-challenge`) at the server, and
`tests/e2e/mfa-login.spec.ts` in the browser, where the pin walks through the
exact two-click sequence a user performs.

### Recovery codes outlive the factor they belonged to

`totp_disable` deletes the TOTP row and recomputes the enrollment flag, but
never touches `user_recovery_codes`. A member who deliberately turns their
second factor off leaves nine unspent Argon2-hashed codes behind in the
database.

Not currently exploitable, and the reason is worth writing down because it is
not a property of `totp_disable` at all: with no factor enrolled,
`recompute_user_mfa_enrolled` clears `mfa_enrolled_at`, so login never issues a
challenge and no endpoint will accept them; and re-enrolling calls
`replace_user_recovery_codes`, which overwrites the set. Both of those could
change without anybody thinking about this function.

*Why not fixed:* credential material that survives the credential it belonged to
should be someone's deliberate decision, and the decision has a real question in
it — whether disabling one factor should invalidate the recovery set shared by
all of them. Pinned by
`mfa/disabling-totp-leaves-the-recovery-codes-in-the-database`.

### Offering the wrong kind of challenge token destroys it

`MfaService::take` removes the entry from the map *before* it discovers the
variant is not the one that was asked for, and then returns `None`. So
presenting a WebAuthn-registration token to `/verify` — or a login token to
`register/finish` — does not merely fail: it consumes the ceremony, and the
legitimate call that follows fails too, with "Unknown or expired
challenge_token".

Not a privilege escalation: the tokens are 48 random alphanumerics, so nobody is
guessing one to grief a stranger. It is a client-bug amplifier, and it is
invisible in the logs, because the second failure looks like an expiry — the one
explanation that is certainly wrong.

*Why not fixed:* the repair is a choice between `take` peeking before removing
and separating the two token namespaces, and both are defensible. Pinned in both
directions by `asking_for_the_wrong_kind_of_challenge_destroys_it` in
`server/src/mfa.rs`.

### Recovery codes are case- and dash-sensitive

The codes are displayed in uppercase with dashes and hashed exactly as
generated. `verify_recovery_path` trims the input and does nothing else, so a
member typing their printed code in lowercase — which is what a phone keyboard
offers by default — is refused, and the refusal is indistinguishable from a
wrong code. The attempt is not free, either: per the finding above, it costs
them the whole login.

*Why not fixed:* one line, but which line is a real question — normalize in the
API layer, or widen `verify_recovery_code`? — and the same question applies to
the dashes. Pinned by `recovery_codes_are_case_sensitive`.

### Login is case-sensitive, on username and on email

`find_user_by_username` and `find_user_by_email` both filter with a plain `eq`
and no `lower()` on either side. This is not a collation artifact — it is the
behavior on every cluster, including UTF-8 ones.

It matters most for email, which is the field people retype. Somebody who
registered as `Alice@example.com` and types `alice@example.com` is told "Wrong
credentials", which is indistinguishable from a wrong password and sends them to
reset one that was right.

The worse half is not asserted, because it needs two accounts: the unique index
is on the raw column, so `Alice@example.com` and `alice@example.com` are two
separate accounts with two separate profiles.

*Why not fixed:* a functional index, a migration, and a decision about rows that
already collide. That is a product change, not a status-code correction.
*(`findings/login-is-case-sensitive`, contract stage)*

### Text the database cannot store answered 500 — fixed

Postgres refuses text it cannot represent, and the application turned that into
a 500: the user was told the site was broken about an input only they could
change.

This was recorded here as unfixable. Diesel's `DatabaseErrorInformation` exposes
no SQLSTATE, so 22P05 and 22021 are not reachable as codes and the only
available signal is Postgres's English prose, which moves with `lc_messages`.
That was the right call for what it looked like: a hostile-cluster curiosity.

It was not that. The same arm catches `invalid byte sequence for encoding
"UTF8": 0x00` — a NUL byte, which **no Postgres text column accepts in any
encoding** — so a request carrying `%00` answered 500 on an ordinary UTF-8
production database, and had done since the routes existed. A fragile match is a
poor trade for a LATIN1-only defect and a good one for a live bug on every
deployment.

`is_unrepresentable_text` now classifies both as 400 — with one deliberate
exception, which is the more interesting half.

Classification happens by error *type*, and the database's complaint is
identical whether the unstorable text came from the caller or from the server:
it just says it cannot store it. Only the handler that made the request knows
which. `create_device_invite` generates its own value — eight emoji — so a 400
there would tell an administrator their input was bad when they supplied no
input at all, and point them at a fix that does not exist. That route overrides
the default and answers 500 naming the real cause: the deployment's database
cannot store device codes and a UTF-8 database is required. The blanket-500
budget for `devices.rs` goes from 0 to 1 to record it, which is exactly the
escape hatch that check's failure message offers.

So the rule is: the default lives in `errors.rs`, and a route that supplies its
own text overrides it. Pushing the special case down into `errors.rs` would mean
asking it to decide something it cannot see. Two things make the
fragility survivable, and both are enforced rather than asserted: a message that
does not match falls through to the same 500 as before, so a localised server is
no worse off; and the two phrases are pinned by tests taken from real captured
server output, alongside a counter-test that `deadlock detected`, `out of shared
memory` and `could not connect to server` are **not** swallowed — a substring
match on prose is exactly the shape that grows to catch what it should not.

### The profile-config write is not atomic

`update_profile_config` commits the new version row and *then* writes
`profiles_enabled` back to the configuration file. The two are not in a
transaction and there is no compensation. A read-only ConfigMap, a full disk or
a permissions change leaves a committed version row and returns 500: the admin
sees a failure and the version history shows their change.

Found because the stack originally mounted its config read-only.

*Why not fixed:* the two stores are a database and a file, so there is no
transaction to put them in. The fix is either to stop storing
`profiles_enabled` in two places or to write the file first and roll the row
back — both product decisions.
*(`findings/profile-config-write-is-not-atomic`, contract stage)*

### A deactivated member's card still opens the door

`compile_state_for` filters *users* through `list_active_users()`, but an
explicit `kind=card` rule is not user-scoped — so the card is compiled into
`allow_cards` and the edge opens the door, while `DoorService::evaluate`
short-circuits on `!user.is_active` and the QR path refuses the same person. Two
doors, two answers, one deactivation.

*Why not fixed:* whether deactivating a member should revoke card rules naming
their card is a product decision. The vectors assert the current behavior with
the reasoning written out, and it cannot change unnoticed.
*(`contracts/door_rules.json`, last case)*

### A "24 / 7" schedule is closed for sixty seconds every night

The server matches an interval as `start <= now < end`, and the template ends at
`23:59`. Not fixable in the template: the interval is `HH:MM` parsed to a
`NaiveTime`, so the end of a day cannot be written down — `24:00` does not parse
and `00:00` is rejected by `validate` as `end <= start`. The fix belongs in the
server's interval model.
*(`frontend/tests/unit/schedule_templates.spec.ts`)*

### `hasRole` is fail-open on an unrecognized *required* role

`roleHierarchy[required] || 0` maps an unknown role to level 0, so a guard
asking for a role that does not exist admits everyone, `Unknown` included.

*Why not fixed:* it is a behavior change on the authorization path, and the
right shape is a total mapping that fails to compile when a role is added.
*(`frontend/tests/unit/auth-roles.spec.ts`)*

### Two live races

**Device-invite redemption.** `register_device` reads the invite, checks
`used_at`, inserts a device, inserts its auth token, and only then marks the
invite used — four statements, no transaction, no `WHERE used_at IS NULL`. The
extra row is the smaller half: a device row carries a standing auth token on the
toolguard and door surface, so a single-use invite that mints two hands one to
somebody who was never meant to have it, and the audit trail shows one
legitimate registration.

**The profile-config version race.** `insert_profile_config_version` does
`SELECT max(version)` then `INSERT version = max + 1` under `READ COMMITTED`
against `UNIQUE (version)`.

Both are exercised by the concurrency stage, asserted on the resource rather
than the response tally, and each paired with a sequential sibling so that a
failure to reproduce is distinguishable from a broken setup. A round that finds
nothing means this scheduling did not lose — not that the window is closed.
*(`e2e/drivers/concurrency.mjs`)*

### The suite's own configuration did not reach its drivers — fixed

Recorded because it invalidated results rather than produced wrong ones, which
is the harder kind to notice.

`run_node` builds the driver environment separately per provisioning path. The
host path runs node in the same shell and inherits whatever the run exported;
the container path inherits nothing and forwarded four wiring variables. Six
settings the drivers read were never conveyed under `--provision=podman`:
`CSS_FUZZ_ITERATIONS`, `CSS_FUZZ_SEED`, `CSS_FUZZ_BATCH`, `CSS_RACE_ROUNDS`,
`CSS_RACE_FANOUT`, `CSS_RUN_TAG`.

So on the workstation and in every reaper session the fuzzer ran 400 iterations
whatever any profile said, the concurrency tier used default rounds and fanout,
and **`CSS_FUZZ_SEED` did nothing at all** — while `SUMMARY.md` printed a replay
command built around that seed on every run. A documented reproduction procedure
that silently does not work is worse than none: anyone replaying a finding would
have taken a different path and concluded it was flaky.

CI was unaffected, because `--provision=external` runs the driver on the host.
That is the asymmetry that hid it, and it means reaper's fuzz tier was *weaker*
than CI's — 400 against 600 — for the whole of this work.

`checks/tests/both_driver_paths_pass_the_same_env.rs` asserted the two paths
agreed with each other, and they did: both were equally incomplete. That
limitation was written into its own doc comment and went on to cost exactly what
it warned about. It now also asserts that every `process.env.*` a driver reads
is something `run_node` can convey.

### Deactivating a member removes them from the admin roster entirely

`DatabaseManager::list_users` filters `is_active.eq(true)`, and
`PaginationParams` carries only `page` and `per_page`. So a deactivated member
is not shown as inactive — they are absent, and **there is no way to list them
through the API at all**.

An administrator who deactivates somebody by mistake cannot find them again to
undo it. In a space where membership lapses and resumes, that is the difference
between "set them inactive until they renew" and "they are gone, create them
again".

Found by Tier 9, which is exactly the shape that tier exists for: no single
response is wrong, and the world after a deactivation is.

Pinned by `findings/deactivated-users-vanish-from-the-admin-roster`, which
asserts they are absent, so the assertion fails the day the roster includes
them. The journey driver withholds deactivated users from the `roster-matches`
comparison for the same reason and points at that pin; when it goes, so does the
adjustment.

It also makes `deactivations-held` vacuous. That invariant asks whether anything
the driver deactivated is reported active, and a deactivated user is not
reported at all — so one of Tier 9's six invariants judges an empty set until
this is fixed.

### "Resource already exists" does not say what already exists

Found by Tier 11 on its first run, and it is the shape that tier exists for:
every assertion in the suite is satisfied by this response, and it is still the
wrong thing to show somebody.

    23. an administrator tried to add a role rule to a door.
        The system refused. It said: "Resource already exists"

A correct 409. But the administrator is not told *which* resource, nor that the
rule they wanted is already on the door and nothing needs doing. A reasonable
reader concludes something must be deleted first, or that the door is in a bad
state. Two lines earlier in the same transcript, `"User not found"` does the job
properly — specific, and it tells you what to change.

The message comes from `From<diesel::result::Error>`'s `UniqueViolation` arm,
which by construction cannot know what the caller was trying to create. That is
the same division of knowledge as the device-invite case above: the generic
classifier is right as a default, and only the handler knows enough to say
something useful. The fix is per-handler overrides at the routes where a
conflict is likely and the object is known — door rules first, since that is
where it was observed.

Not fixed here. Tier 11's output is evidence for a person to act on, and turning
every conflict into bespoke prose across ~40 handlers is a unit of work with a
design decision in it, not a correction.

### Nothing links a device back to the invite that created it

`space_device_auth_requests` carries the `device_code`; `space_devices` has no
invite or auth-request column. So "which invite produced this device" is a
question the audit trail cannot answer, for a system that decides who opens a
door.

The consequence for the suite is that `invites-are-single-use` can only check
its weaker half — that the server lists at least as many devices as the driver
registered. Its stronger half, "invite X produced two devices", is not
observable through the API, and the journey driver records that as a skip rather
than mapping a field that does not exist and counting nothing forever.

### A failed audit write is logged and discarded

`AuditLogger::log_event` ends:

```rust
if let Err(e) = self.db.create_audit_log(&audit_log) {
    tracing::error!("Failed to save audit log to database: {}", e);
}
Ok(())
```

The write fails, the caller is told the event was logged, and the only
surviving trace is one ERROR line in a log nobody reads. For a system that
decides who can open a door and who can operate a machine, an audit trail that
silently drops entries is worth more attention than the operations it records.

Found because it fired: a fuzz run reached the trainer-removal route with a
synthetic user id, the audit insert violated `audit_logs_user_id_fkey`, and
`logs/no-audit-write-was-swallowed` was the only oracle that noticed. That
particular trigger is fixed at its cause -- the handler now answers 404 for a
removal that removes nothing, so it never reaches the logger -- but **the
swallowing is untouched**, and any other route whose audit write fails will
fail the same silent way.

Not fixed here because the alternatives are a design decision rather than a
correction. Propagating the error turns a transient database problem into a
failed request for an operation that already succeeded; queueing the write
needs somewhere durable to queue it. Both are the owner's call.

`logs/no-audit-write-was-swallowed` is the standing detector: it fails the
stack battery on any run where an audit write is discarded, which is how this
would be found again rather than accumulating quietly.

### The audit log records the wrong role for the first administrator

`auth::register` writes `"role": "Newbie"` into the audit event unconditionally,
even when `should_grant_admin_role` has just made the account an admin. The
account is correct; the record of how it came to exist is not — which is the
opposite of what an audit trail is for.

### A repository nobody configured answered 500

`POST /api/admin/pages/{wiki,site}/refresh` returned 500 with "repo not
configured" — a state an administrator put the instance in, not a fault, so the
server told them it had broken about a setting they can change in the next
screen. **Fixed**: it is a 409 now, and the check is against the configuration
rather than against the error's text, because a status code that depends on a
string is a status code that changes when a message is reworded.

*(Found by the fuzz tier. Listed here because the reasoning generalises to the
121 sites in the ratchet.)*

### Four routes are registered and can never succeed

`create_training_type`, `authorize_trainer`, `complete_training` and
`revoke_training` return 501. That is an honest answer — better than a 500, and
far better than a route that silently does nothing — but from outside the
codebase a registered route is a promise, and the training UI calls two of them
with no way for anybody reading the frontend to tell a stub from a working
endpoint: `api.ts` wraps every call in `.catch` and produces the same generic
"Failed to …" for both.

*Why not fixed:* they are unbuilt features, not defects.
*(`checks/tests/unimplemented_endpoints.rs` pins the list and asserts each one
says which feature is missing.)*

### Thirty-six color pairings are below WCAG AA

`text-error`, `text-warning`, `text-success` and `text-info` set a foreground
and leave the background to whatever card the element sits in — `base-100`,
`base-200` or `base-300` in this application. Across the fourteen themes, 36 of
those pairings fall below 4.5:1. `lofi`'s `text-success` on `base-300` is
**1.01:1** — the same color, effectively.

Not hypothetical: `ProfileField.vue` renders both the required marker and the
validation message in `text-error`, so a theme in that list is a theme where the
reason a form will not submit cannot be read.

*Why not fixed:* changing a designer's palette from a test is not a test's
business. The number is what makes the conversation possible, and it is
ratcheted so it can only go down.
*(`frontend/tests/structure/contrast.spec.ts`)*

### A config without a `pages` block froze the entire application

**Fixed.** `shouldShowWikiInNav` and its three siblings read
`config.value?.pages.wiki_enabled` — the optional chain covered `config` and
stopped there. A config object whose `pages` block was absent threw
`Cannot read properties of undefined`.

That would be a minor bug almost anywhere else. Here it is called from a
computed during `App.vue`'s render, and **Vue stops patching a component whose
render function throws** — so App.vue froze on whatever it had last drawn, which
during boot is the full-screen `fixed inset-0 … z-50` loading overlay. The
application rendered correctly and then accepted no input at all, with nothing
visible anywhere except a console warning.

`PublicConfig`'s own comment says feature blocks "default to false on older
servers that don't yet emit these blocks", so a server that omits one is an
expected case; the guard was one level too shallow to survive it.

Found by the browser tier, whose fake happened to serve a config without
`pages`. **The fake was wrong, and that is what made it a good test** — it sent
a shape a server could plausibly send, and the client could not survive it.
*(`frontend/tests/unit/config-store.spec.ts` — eleven cases, mutation-checked)*

### A failed configuration load says nothing at all

`configStore.fetchConfig` catches its own errors and does not rethrow, so
`App.vue`'s `Promise.all([authStore.initialize(), configStore.fetchConfig()])`
resolves, its `catch` never runs, and the "Initialization Error" notification
that exists for exactly this case is **unreachable** for the failure most likely
to happen at boot.

The application then runs on `PublicConfig`'s defaults — no site name, page
links off, features gated — and nothing anywhere says the configuration failed
to load. It looks like an administrator has not set anything up.

*Why not fixed:* rethrowing changes what every other caller of `fetchConfig`
sees, and the store's `error` ref is arguably the right channel — what is
missing is anything rendering it. That is a product decision about where a
degraded boot should be surfaced.
*(`frontend/tests/components/AppBoot.spec.ts`, and the browser tier)*

### Smaller things, recorded where they live

* The configured `site_name` is not in the page header. `/config/public` serves
  it and `App.vue` renders a hardcoded `CSS`; only `HomeView` and `AdminView`
  read the configured value. A space that sets its own name sees it in two
  places out of three.
* **Neither field on the login form was labeled.** A `<label>` with no `for`
  whose input is a sibling rather than a child is associated with nothing: a
  screen reader announces an unlabeled text field, and clicking the label does
  not focus it. Both fields on the one form every user must complete were like
  that. **Fixed** — found because the browser tier addresses fields by their
  label, which is what assistive technology does, so the spec's selector *is*
  the accessibility assertion rather than a separate one somebody has to
  remember to write. The rest of the application's forms have not been audited.

* `api/toolguard.rs` hand-rolls device auth from a bare `HeaderMap` beside the
  `DeviceAuth` extractor that exists for the purpose. Two implementations of one
  check. *(`checks/tests/toolguard_auth.rs`)*
* ToolGuard parses its query parameters before it authenticates, because `Query`
  is a `FromRequestParts` extractor and runs before the handler body. A request
  missing `card` gets 400, not 401.
  *(`server/tests/contract_matrix.rs::toolguard_parses_parameters_before_it_authenticates`)*
* `RegisterView` renders `terms_of_service_md` with `v-html` without converting
  it, so markdown syntax appears literally.
* `PagesConfig::default()` names two live GitHub repositories, and
  `PagesService::new` clones whatever is there into a hardcoded `/tmp` path at
  boot — so a deployment starting with no config file at all clones two
  repositories belonging to somebody else on its first boot.
  *(`server/src/config.rs::the_default_config_still_names_two_live_repositories`)*
* A zero `purchase_price` is indistinguishable from an unknown one, because the
  row is rendered behind a truthiness check.
  *(`frontend/tests/components/ToolCard.spec.ts`)*
* Four frontend calls target training routes the server does not have.
  *(`checks/tests/route_parity.rs`, `UNRESOLVED`)*

---

## 9. Narrowings in force

Every one of these is scoped to exactly what it covers.

| Narrowing | Scope | Reason |
|---|---|---|
| `tests/components` and `tests/e2e` outside the strict project | those two directories | Mounting a component pulls its whole `.vue` file into the strict project, so adding tier-2 specs made the ratchet report ~30 `strictNullChecks` errors across nine components that no test touches directly. Those errors are real and are the ratchet's future work; fixing them as a side effect of adding a test is the giant unreviewable diff the ratchet exists to avoid. Both directories are still fully type-checked by `npm run type-check` — they lose `strictNullChecks`, not checking. **The next step is one component at a time**, and when nine are done `tests/components/**` goes back. |
| `no-unsafe-*`, `no-explicit-any` off | everything except `tsconfig.strict.json`'s include list | The base tsconfig is `"strict": false` and 585 of 1034 initial lint problems were downstream of that. **Growing the strict include list is the unit of work**; `eslint.config.js` and `tsconfig.strict.json` name the same paths so the two ratchets move together. Every other rule, `no-floating-promises` included, stays on everywhere. |
| `vue/multi-word-component-names` off | `src/App.vue` only | The framework's own convention; the file cannot be renamed. |
| `no-require-imports` off | the four CommonJS config files at `frontend/` root | tailwind and postcss load them through their own resolvers; converting them to ESM is a build change, not a lint fix. |
| `vue/no-v-html` disabled per element | 3 elements | Two render markdown already escaped server-side by comrak with `Options::default()` (`render.unsafe_` is false); one renders config text only an administrator can set. The fourth — an external iCal feed — was **not** exempted; it is now interpolated. |
| `JOURNEY_ITERATIONS` / `JOURNEY_SEEDS` set and read by nothing | those two names in `.reaper.toml` | The Tier 9 journey driver is not written, so `[profiles.hunt]` configures a tier that has no consumer. Kept rather than deleted, because the configuration records the intent; exempted by name in `checks/tests/configured_env_is_read.rs`, which fails if either ever *does* become read, so the exemption cannot outlive its reason. |
| eslint pinned to an unsupported major | both frontends | 9.39.5 is the last 9.x and is out of support; 10.x is current. `frontend_edge` was set up on 9 **to match `frontend`**, not because 9 is right: one repository with two flat-config dialects is worse than one a major behind in step. Moving both is its own unit of work. This narrowing covers the version only — every rule is on. |
| `frontend_edge` linted without a ratchet | not a narrowing, recorded for contrast | `frontend`'s `no-unsafe-*` family is off except on a growing include list, because 24k lines were written under `"strict": false`. `frontend_edge` is 565 lines whose base tsconfig is already `"strict": true`, so every rule is on everywhere and there is no list. A narrowing appearing here later is a regression to argue about. |
| clippy not yet in CI | the `rust` job | The build is warning-free now, so `-D warnings` is finally possible. Turning clippy on is its own unit of work: `clippy::pedantic` on 19.6k never-linted lines produces a commit carrying forty `#[allow]`s, which is the weakening this methodology forbids wearing the costume of progress. |
| A blanket-500 budget rather than a fix | 52 sites, of which 10 are `errors.rs` itself | **The stated reason below no longer holds, and this needs a decision.** It was written when nothing asserted these statuses. The Tier 7 no-5xx oracle now does, across all 164 routes, and it is finding them one seed at a time: two runs in a row went red on two different routes (`trainers.rs`, `training.rs`), both the same shape -- a handler naming an entity that does not exist, Postgres rejecting it on a foreign key, and a blanket `InternalServerError` reporting the caller's mistake as the server breaking. Each fix is three lines and mechanical, because `From<diesel::result::Error>` already classifies these correctly and the handler is discarding that. Left alone this makes CI intermittently red forever, and the intermittency is the fuzz seed, not flakiness. |
| *(the original reason, kept)* | |  Converting them all at once is a large diff touching every handler, reviewed by nobody, for status codes nothing yet asserts. `checks/tests/database_errors_keep_their_meaning.rs` pins the count **per file** so a fix in one and a regression in another cannot cancel out — and it fails when a file *improves* without the budget coming down, because a ratchet that does not tighten gives back the ground it won. |
| The invite-redemption race is not exercised on a non-UTF-8 cluster | that one scenario | A device invite code is eight emoji, so the row cannot be written at all. The finding is asserted instead, and the profile-config race runs either way. `CSS_E2E_DB_ENCODING=UTF8` exercises the race itself. |
| Two ERROR messages exempted in the `logs` stage | those two strings | Both are correct 404s logged at the wrong level. Each exemption is itself checked for staleness — as a *skip*, not a failure, because those lines come from the fuzz tier reaching for things that do not exist and a short run legitimately may not reach them. Making it a failure would couple the logs stage's result to the fuzz iteration count. |
| *(removed)* The astral-plane corpus entry | — | **This narrowing no longer exists.** It withheld corpus entries a non-UTF-8 cluster could not store, because every route that wrote them answered 500 and buried anything new. Classifying unrepresentable text as 400 retired the reason, so the full corpus now runs on every cluster and `fuzz/whole-corpus-on-every-cluster` asserts that nothing is withheld — a future narrowing has to delete a passing assertion rather than quietly add a filter. Recorded here rather than deleted, because a narrowing that disappears without explanation is indistinguishable from one nobody noticed. |
| Two `(method, template, status)` triples exempted in the fuzz tier | those two triples | Narrower than a route exemption, which would cover the next real 500 on that route, and much narrower than a status exemption, which would switch the oracle off. A stale entry here **is** a failure: the fuzzer's coverage narrowing is itself the news. |
| `findings/...` assertions pin defects rather than failing | the eight listed in §8 | A suite that stays red teaches people to ignore red. Each of these fails the day the defect is fixed, with a message saying that failing is the good outcome and the assertion should be deleted. |
| `expect_used` allowed | workspace | An `expect` carries a message and documents an invariant; an `unwrap` documents nothing. |
| `print_stdout` allowed | `cli` only | Printing is that crate's entire job. |

---

## 10. The acceptance test

The gate on the whole exercise, from §15 of the methodology: **take defects you
have already found and fixed by hand, revert the fixes, and confirm the harness
rediscovers them.** A methodology that cannot rediscover your known bugs is not
yet measuring anything.

It is a script, `e2e/acceptance.sh`, rather than something somebody did once:

    e2e/acceptance.sh break     # revert the four fixes
    reaper test                 # the suite is now EXPECTED to fail
    e2e/acceptance.sh restore

Each revert is **surgical** — the behavioral change only, not the whole
commit. `11c4f42` in particular added a migration, a database module and an
entire version history alongside the guard change; reverting that wholesale
produces a tree that fails for reasons which have nothing to do with the
defect, which looks like success and proves nothing.

The script asserts its own reverts landed. A substitution that matches nothing
leaves the tree correct and the run green, and the acceptance test then reports
a pass for work it did not do — the one failure this document exists to
prevent. Four files must be modified or it restores itself and exits non-zero.

### It has been run, and it passes

Run on 2026-08-26 against the suite as it stands. All four reverted fixes were
rediscovered, each by the tier the table below predicts — and by **no other**,
which is the part that says the tiers are not redundant:

| Reverted fix | Caught by | What it said |
|---|---|---|
| `5c2fa3c` | Tier 6 `contract` | `newbie-reads-profile-config -- expected [200], got [403]` |
| `11c4f42` | Tier 6 `contract` | `newbie-cannot-write-profile-config -- expected [403], got [200]` |
| `fdc887c` | Tier 6 `devices` | `debug-serves-the-path-it-was-given -- --frontend-path pointed at a fixture and the marker is not in the response` |
| `92afb4c` | Tier 5 `browser` | 4 failed: the two `abortNext` specs, on both viewports |

Nine stages stayed green throughout, which matters as much as the four
failures: a suite that goes red everywhere when anything breaks cannot tell you
*what* broke.

Two details worth keeping. `92afb4c` was caught **only** by the transport-abort
specs — the `failNext` cases passed, because axios attaches a `response` to
every HTTP error and the fallback branch is unreachable through them. And an
automated security scanner, running against the working tree while it was
reverted, independently flagged `11c4f42` as a privilege escalation. That is the
acceptance test's premise confirmed from an angle nobody arranged.

### The corpus, and what should catch each one

| Reverted fix | Caught by |
|---|---|
| `92afb4c` door check-in silent failures | Tier 2, `DoorCheckinView.spec.ts` — the transport-failure case. **Only** a rejection with no `response` reaches that branch: axios attaches one to every HTTP error, so a suite injecting 500s never executes it. |
| `5c2fa3c` profile page for non-admins | Tier 6 `contract` — `5c2fa3c/newbie-reads-profile-config` must be 200. Tier 3 route parity catches it at build time too. |
| `fdc887c` `--frontend-path` | Tier 6 `devices`, running the **debug** binary against a fixture directory and asserting the *bytes served*. No unit test can see this: `cargo test` compiles one profile, and the release build has a different `create_router` that ignores the flag by design. |
| `11c4f42` profile config admin-only | Tier 6 `contract` — `11c4f42/newbie-cannot-write-profile-config` must be 403. |

### The nuance on the two guard reverts

`5c2fa3c` and `11c4f42` both change a guard on `/api/profiles/config`, and the
contract tier's route table states what each guard should be. So reverting the
guard **alone** is caught by `checks/tests/route_table_matches.rs` during the
build verb — correctly, and before a stack is ever brought up.

That is a real answer and it is not the one being tested. So those two reverts
also update the route table to match, which is what a regression looks like when
somebody changes a guard deliberately and keeps the table in step. Only a tier
that talks to a running server can catch that, which is exactly the claim the
contract stage exists to support.

---

## 11. Adding to this

In the methodology's order of return on effort, adjusted for what already
exists here:

1. `server/src/lib.rs`, unblocking Tier 4 and clearing the 30 warnings.
2. The Tier 4 route × credential matrix — the largest single reduction in
   unmeasured risk available.
3. Tier 7 seeded fuzz. Cheapest defect-per-line of anything here.
4. Tier 5 browser-vs-fake with transport error injection.
5. Tier 1b golden vectors, before the ToolGuard wire types gain a seventh copy.
6. Tier 6 full stack, started hostile.
7. Tier 8 concurrency, one harness per capacity rule.
8. Tier 2 component conformance.
9. Tier 9 simulated users — **write the oracle self-test first**, or you will
   not know whether it works.

When you add a check, add the thing that guards it too. Every scraper in
`checks/` asserts that it found something, because a scraper that quietly finds
less than it used to is indistinguishable from a codebase that got smaller —
and four separate scraper bugs were caught that way while writing them.
