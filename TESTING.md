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

# Rust — see §3. On FreeBSD it does not build; use a reaper session.
cargo fmt --all -- --check
cargo test --locked --all-targets

# The whole battery on a disposable Linux machine
reaper up && reaper test && reaper down
```

---

## 2. What is actually covered today

Honest status, tier by tier. "Written but unexercised" is called out where it
applies, because a suite nobody has watched pass is a suite of unknown value.

| Tier | Question only it answers | State |
|---|---|---|
| 1 Pure unit | Is this calculation right, at its boundaries? | **Partial.** 82 Rust tests + 36 TypeScript, from 44 Rust and 0 TypeScript. 51 of the Rust tests run on the workstation; the rest need Linux. |
| 1b Cross-implementation vectors | Do two independent implementations agree? | **Not started.** Six copies of the ToolGuard wire types still exist. |
| 2 Component conformance | Did the rendered output drift? | **Not started.** vitest and jsdom are installed and the harness works; no component is mounted yet. |
| 3 Source-as-data | Does the code's structure still hold its claims? | **Substantial.** 9 checks in `checks/`, plus 4 in `frontend/tests/structure/`. This tier found more real defects than any other. |
| 4 Server contract | Do the authorization and enum rules hold, in isolation? | **Started.** `server/src/lib.rs` exists, `DatabaseManager::disconnected()` is the seam, and `server/tests/contract_auth.rs` runs 7 cases in-process with no database. The full route × credential matrix is not written. |
| 5 Browser vs fake API | What does the app do when a request *fails*? | **Not started.** |
| 6 Full stack | Does it work against a real database, broker, charset? | **Skeleton only.** `e2e/run.sh` has its stage machinery, `preflight`, and provisioning; no stack is brought up yet. |
| 7 Seeded fuzz | Does any ordinary-but-untried request crash it? | **Not started.** |
| 8 Concurrency | Does the invariant survive simultaneous writers? | **Not started.** Two live races are identified in §8. |
| 9 Simulated users | What breaks only after history accumulates? | **Not started.** |
| 10 Live browser audit | Does the UI hold up over a world somebody else built? | **Not started.** |
| 11 Human evidence | Does this make sense to a newcomer? | **Not started.** |

**Formatting and linting are complete and gating.** `rustfmt`, `prettier`,
`eslint` (type-aware, flat config) and `shellcheck` all pass, and CI fails on
any of them.

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
| Stack stages | `./e2e/run.sh --only preflight` · `--list-stages` | session / CI |

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

**The Tier 4 matrix is one route wide.** `server/src/lib.rs` now exists, all
three crates have had the lib/bin split, and `server/tests/contract_auth.rs`
proves the seam works: seven cases over the real router with a dead pool,
including the liveness meta-test that makes the negative results mean
something. What is missing is the breadth — the hand-written table of all 134
routes × every credential state. The hard part is done; the table is not.

**Role gating cannot be asserted offline at all.** `AdminUser` and `StaffUser`
delegate to `AuthUser`, which loads the user from the database, so 403-for-
insufficient-role needs a real Postgres. Those rows belong to the container
tier and are deliberately not folded into the offline file.

**7 dead-code warnings remain in `css-server`**, down from 32 before the lib
split. They are unused variables and one never-read field — each needs an
individual judgement about whether the code is dead or the caller is missing,
which is exactly the kind of thing that should not be batch-resolved. Until
they are, `-D warnings` cannot go on and CI does not run clippy.

**Tiers 5 through 11 are not implemented.** `e2e/run.sh` has its stage
machinery, argument handling, result recording and a working `preflight`, and
it refuses an unknown `--only` stage rather than silently running nothing. What
it does not yet have is any stage that brings a stack up. `STAGES_ALL` lists
exactly one stage because exactly one is implemented — listing more would mean
either failing every run or passing without doing anything, and a suite that
reports green for work it did not do is the specific failure this whole
exercise exists to prevent.

**Tier 2 has a harness but no component tests.** vitest, `@vue/test-utils` and
jsdom are installed and 36 tests run against pure modules and stores. No
component is mounted yet.

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

---

## 8. Known defects that tests record rather than fix

Each of these is asserted as-is, so it cannot widen unnoticed, and each says in
its own comments why it was not fixed here.

**A "24 / 7" schedule is closed for sixty seconds every night.** The server
matches an interval as `start <= now < end`, and the template ends at `23:59`.
Not fixable in the template: the interval is `HH:MM` parsed to a `NaiveTime`, so
the end of a day cannot be written down — `24:00` does not parse and `00:00`
would be rejected by `validate` as `end <= start`. The fix belongs in the
server's interval model.
*(`frontend/tests/unit/schedule_templates.spec.ts`)*

**`hasRole` is fail-open on an unrecognised *required* role.**
`roleHierarchy[required] || 0` maps an unknown role to level 0, so a guard
asking for a role that does not exist admits everyone, `Unknown` included.
Changing it is a behaviour change to the authorization path and belongs with the
server-side matrix work.
*(`frontend/tests/unit/auth-roles.spec.ts`)*

**A lost-update race on `profile_config_versions`, which surfaces as a 500.**
`insert_profile_config_version` does `SELECT max(version)` then
`INSERT version = max+1` inside a `READ COMMITTED` transaction against
`UNIQUE (version)`. Two concurrent admin edits collide. It becomes a 500 rather
than a 409 because it returns `DatabaseError::Diesel`, whose conversion
special-cases only `NotFound` — while the *direct* `From<diesel::result::Error>`
path does map `UniqueViolation` to `Conflict`. Two conversion paths, two answers
for one failure. This is Tier 8's sharpest target and is not yet written.

**Device invite redemption has the same shape.** `register_device` reads the
invite, checks `used_at`, then marks it used in a separate statement with no
transaction and no `WHERE used_at IS NULL`.

**`api/toolguard.rs` hand-rolls device auth** with a bare `HeaderMap` beside a
`DeviceAuth` extractor that exists for the purpose. Two implementations of one
check.

**`RegisterView` renders `terms_of_service_md` with `v-html` without converting
it**, so markdown syntax appears literally.

---

## 9. Narrowings in force

Every one of these is scoped to exactly what it covers.

| Narrowing | Scope | Reason |
|---|---|---|
| `no-unsafe-*`, `no-explicit-any` off | everything except `tsconfig.strict.json`'s include list | The base tsconfig is `"strict": false` and 585 of 1034 initial lint problems were downstream of that. **Growing the strict include list is the unit of work**; `eslint.config.js` and `tsconfig.strict.json` name the same paths so the two ratchets move together. Every other rule, `no-floating-promises` included, stays on everywhere. |
| `vue/multi-word-component-names` off | `src/App.vue` only | The framework's own convention; the file cannot be renamed. |
| `no-require-imports` off | the four CommonJS config files at `frontend/` root | tailwind and postcss load them through their own resolvers; converting them to ESM is a build change, not a lint fix. |
| `vue/no-v-html` disabled per element | 3 elements | Two render markdown already escaped server-side by comrak with `Options::default()` (`render.unsafe_` is false); one renders config text only an administrator can set. The fourth — an external iCal feed — was **not** exempted; it is now interpolated. |
| clippy not yet in CI | the `rust` job | 30 dead-code warnings in `css-server` that the pending lib split resolves. The rule set is written; the fallout is not cleared. |
| `expect_used` allowed | workspace | An `expect` carries a message and documents an invariant; an `unwrap` documents nothing. |
| `print_stdout` allowed | `cli` only | Printing is that crate's entire job. |

---

## 10. The acceptance test

The gate on the whole exercise, from §15 of the methodology: **take defects you
have already found and fixed by hand, revert the fixes, and confirm the harness
rediscovers them.** A methodology that cannot rediscover your known bugs is not
yet measuring anything.

The four fixes at the head of `feature/tests` are the corpus:

| Reverted fix | Should be caught by | Status |
|---|---|---|
| `92afb4c` door check-in / rule management silent failures | Tier 5 transport-error injection | **not yet** — Tier 5 unwritten |
| `5c2fa3c` profile page for non-admins | Tier 3 route parity; Tier 6 `contract` | route parity exists; contract stage unwritten |
| `fdc887c` `--frontend-path` wiring | Tier 6 `devices`, running the **debug** binary — the flag only exists under `#[cfg(debug_assertions)]`, so a release binary ignores it and building only one profile leaves a `cfg` branch nothing executes | **not yet** |
| `11c4f42` profile config admin-only | Tier 6 `contract`; Tier 8 version race | **not yet** |

This has **not** been run. It cannot be until the tiers above exist. It is
recorded here as the gate, not as an achievement.

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
