# Testing

How this project is tested, what each layer can and cannot see, where each one
runs, and what is *not* covered yet.

The organising idea is from
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
| 1 Pure unit | Is this calculation right, at its boundaries? | **Substantial.** 151 Rust tests and 172 TypeScript, from 44 Rust and 0 TypeScript. About a third of the Rust tests run on the workstation; the rest need Linux. |
| 1b Cross-implementation vectors | Do two independent implementations agree? | **Started.** `contracts/door_rules.json` — 10 cases read by `server/tests/door_vectors.rs` and `edge/tests/door_vectors.rs`, with the edge half fed from the server's *declared* output. It found the inactive-member divergence. The five ToolGuard wire-type copies are not unified, but `checks/tests/toolguard_wire_types.rs` now records exactly how they disagree and fails on a sixth. `wire_kinds.json` is not written. |
| 2 Component conformance | Did the rendered output drift? | **Started.** Five suites, 129 cases, on the components carrying the four fixes the acceptance test reverts. Thirty-five components have none. Every suite here was mutation-checked against the defect it covers. |
| 3 Source-as-data | Does the code's structure still hold its claims? | **Substantial.** 55 cases in `checks/`, plus 11 in `frontend/tests/structure/`. This tier has found more real defects than any other, and the whole crate runs in under a second on any host — including the one where `css-server` cannot be built at all. |
| 4 Server contract | Do the authorization rules hold, in isolation? | **Complete for what it can reach.** 991 route × credential pairs asserted in-process against a deliberately dead pool, plus the 24 device pairs it explicitly defers, which the stack tier asserts. |
| 5 Browser vs fake API | What does the app do when a request *fails*? | **Running.** 32 tests across two viewports, green. A fake API as a Vite middleware — so it imports the real validator and shares one origin with the real bundle — with four injection shapes. It found the config-shape freeze that no other tier could see, and getting `abortNext` to actually abort took three attempts: Chromium retries an idempotent GET when a connection closes before any bytes, so only a *truncated* response is a real transport failure. |
| 6 Full stack | Does it work against a real database, broker, charset? | **Running, green.** Twelve stages: preflight, up, schema, restart, contract, fuzz, concurrency, health, devices, browser, logs, down. Postgres LATIN1 / lc_collate=C / lc_ctype=C, `TZ=America/Chicago`, mosquitto, and the real release binary. It found the migration this schema could not apply, the 401-for-a-role defect, and the 404 on every deep link. `devices` runs both edge binaries, which is the only way to exercise a `#[cfg]` branch; `logs` treats the server's own ERROR output as an oracle. |
| 7 Seeded fuzz | Does any ordinary-but-untried request crash it? | **Running.** Three oracles over all 164 endpoints, seeded and replayable. |
| 8 Concurrency | Does the invariant survive simultaneous writers? | **Running.** Both known races, each asserted on the resource and paired with a sequential sibling. |
| 9 Simulated users | What breaks only after history accumulates? | **Oracle only.** Six invariants over the accumulated world, and a 20-case self-test that feeds each of them what a broken server would send and requires it to fire — written first, deliberately, because an invariant that never fires is indistinguishable from a passing suite. It runs in `e2e/lint.sh` with no stack at all. The journey driver that uses them is not written. |
| 10 Live browser audit | Does the UI hold up over a world somebody else built? | **Not started.** |
| 11 Human evidence | Does this make sense to a newcomer? | **Half started.** The contrast audit exists: WCAG relative luminance over all fourteen themes, with OKLCH converted for daisyUI's built-ins and the reference implementation checked against three known answers. It found **36 semantic/base pairings below AA**, pinned as a ratchet. The prose-transcript half needs Tier 9's journey driver and does not exist. |

**Formatting and linting are complete and gating.** `rustfmt`, `prettier`,
`eslint` (type-aware, flat config), `shellcheck` and `shfmt` all pass, and CI
fails on any of them. A `[Vue warn]` during a component test is a test failure,
with no allowlist.

**Where the findings came from.** Worth recording, because it says where to
spend the next hour:

| Tier | Real defects it found |
|---|---|
| 3 Source-as-data | The unauthenticated ToolGuard endpoints; four broken CLI paths; the `UserRole` wire drift; the duplicate migrations root; the two divergent error conversions; five diverged copies of one wire format |
| 1 / 2 Unit and component | The unreachable training warning; the iCal `v-html`; the roster refresh that never refreshed; the roster error banner that destroyed the list |
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

**Which machine.** The Linux jobs run on `${{ vars.CI_RUNNER || 'ubuntu-latest' }}`.
Unset means GitHub-hosted, which is what a fork or a clone gets with no
configuration at all. Set the repository or organisation variable `CI_RUNNER` to
a self-hosted label to send them elsewhere; this project's upstream uses
`arc-runner-set`.

**What that variable is worth knowing about.** It was a hardcoded
`arc-runner-set` until this branch, and self-hosted runners do not cross a fork
boundary -- a fork cannot claim its parent's runners, because that would let
anyone who forked the repository run code on the parent's infrastructure. So on
this fork every job queued against a runner that could never take it and was
cancelled silently twenty-four hours later. The Actions tab stayed empty through
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
`cc` could not recognise. Three of those four were fixed by upgrading a
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

**Tier 10 has no live browser audit.** Tier 5 now runs — 32 tests, two
viewports, green on the workstation, in a reaper session and in GitHub Actions —
so the thing that blocked this is gone. What is missing is the tier that points
those specs at the *real* stack over a world somebody else built, with a
watchdog failing any test that observes a 5xx. That is written down in the
design and not written in code.

**Tier 9 has an oracle and no journeys.** The invariant self-test was written
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

**Clippy still does not run in CI.** The build is warning-free now — the last
four went with the `AuthError` response deletion, an unmutated lock guard and a
vestigial database handle — so `-D warnings` is finally possible. Turning it on
is its own unit of work, because `clippy::pedantic` on 19.6k never-linted lines
produces a commit carrying forty `#[allow]`s, which is the weakening this
methodology forbids wearing the costume of progress.

---

## 8. Known defects that tests record rather than fix

Every one of these is **pinned by an assertion on the current behaviour**, not
left as a failing test. That is a deliberate choice and it is worth stating why:
a suite that stays red teaches people to ignore red, and within a month a
genuine regression is indistinguishable from the wallpaper. An assertion that
pins a defect in place fails the day somebody fixes it — which is exactly when
somebody should read it, confirm the fix, and delete the assertion. Each one
says so in its own failure message.

None of these was fixed here, and each says why.

### Login is case-sensitive, on username and on email

`find_user_by_username` and `find_user_by_email` both filter with a plain `eq`
and no `lower()` on either side. This is not a collation artifact — it is the
behaviour on every cluster, including UTF-8 ones.

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

### Text a non-UTF-8 database cannot store answers 500

Postgres refuses it at the server with SQLSTATE 22P05, and the application turns
that into a 500 — telling the user the site is broken about an input only they
can change.

*Why not fixed:* diesel classifies error kinds structurally, and
`DatabaseErrorInformation` exposes message, details, hint, table, column,
constraint and statement position — and no SQLSTATE. Recognising 22P05 today
means matching English prose that changes with the server's `lc_messages`, which
is a worse failure than the one it fixes: it would work in testing and stop
working in a deployment whose locale differs, silently, in the direction of
calling a 4xx a 500. The real fix is encoding-aware validation at the input
boundary, using the encoding the server reports at boot.
*(`findings/astral-text-is-a-500-not-a-4xx`, contract stage)*

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
their card is a product decision. The vectors assert the current behaviour with
the reasoning written out, and it cannot change unnoticed.
*(`contracts/door_rules.json`, last case)*

### A "24 / 7" schedule is closed for sixty seconds every night

The server matches an interval as `start <= now < end`, and the template ends at
`23:59`. Not fixable in the template: the interval is `HH:MM` parsed to a
`NaiveTime`, so the end of a day cannot be written down — `24:00` does not parse
and `00:00` is rejected by `validate` as `end <= start`. The fix belongs in the
server's interval model.
*(`frontend/tests/unit/schedule_templates.spec.ts`)*

### `hasRole` is fail-open on an unrecognised *required* role

`roleHierarchy[required] || 0` maps an unknown role to level 0, so a guard
asking for a role that does not exist admits everyone, `Unknown` included.

*Why not fixed:* it is a behaviour change on the authorization path, and the
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

### Thirty-six colour pairings are below WCAG AA

`text-error`, `text-warning`, `text-success` and `text-info` set a foreground
and leave the background to whatever card the element sits in — `base-100`,
`base-200` or `base-300` in this application. Across the fourteen themes, 36 of
those pairings fall below 4.5:1. `lofi`'s `text-success` on `base-300` is
**1.01:1** — the same colour, effectively.

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
* **Neither field on the login form was labelled.** A `<label>` with no `for`
  whose input is a sibling rather than a child is associated with nothing: a
  screen reader announces an unlabelled text field, and clicking the label does
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
| eslint pinned to an unsupported major | both frontends | 9.39.5 is the last 9.x and is out of support; 10.x is current. `frontend_edge` was set up on 9 **to match `frontend`**, not because 9 is right: one repository with two flat-config dialects is worse than one a major behind in step. Moving both is its own unit of work. This narrowing covers the version only — every rule is on. |
| `frontend_edge` linted without a ratchet | not a narrowing, recorded for contrast | `frontend`'s `no-unsafe-*` family is off except on a growing include list, because 24k lines were written under `"strict": false`. `frontend_edge` is 565 lines whose base tsconfig is already `"strict": true`, so every rule is on everywhere and there is no list. A narrowing appearing here later is a regression to argue about. |
| clippy not yet in CI | the `rust` job | The build is warning-free now, so `-D warnings` is finally possible. Turning clippy on is its own unit of work: `clippy::pedantic` on 19.6k never-linted lines produces a commit carrying forty `#[allow]`s, which is the weakening this methodology forbids wearing the costume of progress. |
| A blanket-500 budget rather than a fix | 121 sites under `server/src/api` | Converting them all at once is a large diff touching every handler, reviewed by nobody, for status codes nothing yet asserts. `checks/tests/database_errors_keep_their_meaning.rs` pins the count **per file** so a fix in one and a regression in another cannot cancel out — and it fails when a file *improves* without the budget coming down, because a ratchet that does not tighten gives back the ground it won. |
| The invite-redemption race is not exercised on a non-UTF-8 cluster | that one scenario | A device invite code is eight emoji, so the row cannot be written at all. The finding is asserted instead, and the profile-config race runs either way. `CSS_E2E_DB_ENCODING=UTF8` exercises the race itself. |
| Two ERROR messages exempted in the `logs` stage | those two strings | Both are correct 404s logged at the wrong level. Each exemption is itself checked for staleness — as a *skip*, not a failure, because those lines come from the fuzz tier reaching for things that do not exist and a short run legitimately may not reach them. Making it a failure would couple the logs stage's result to the fuzz iteration count. |
| The astral-plane corpus entry, on a non-UTF-8 cluster | one corpus entry, one kind of cluster | A non-UTF-8 database cannot store it, so every route that writes text answers 500 — reproducing `findings/astral-text-is-a-500-not-a-4xx` on each of them and burying anything new. The alternative considered and rejected was exempting 500 for those routes, which switches the oracle off for them entirely. `CSS_E2E_DB_ENCODING=UTF8` runs the full corpus, and the fuzz stage reports the omission as a skip on every run that makes it. |
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

Each revert is **surgical** — the behavioural change only, not the whole
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
