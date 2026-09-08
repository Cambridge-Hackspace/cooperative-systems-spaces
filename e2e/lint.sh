#!/usr/bin/env bash
#
# Everything about the test suite's own code that can be checked without a
# stack, a network or a container engine. This is the gate a commit touching
# e2e/ is expected to pass, and it runs on the FreeBSD workstation as happily
# as in CI.
#
# It deliberately runs every check rather than stopping at the first failure:
# knowing that three things broke is worth more than knowing that one did.
set -Eeuo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

failed=''
run() { # run <label> <command...>
  local label="$1"
  shift
  printf '\n=== %s ===\n' "${label}"
  if "$@"; then :; else failed="${failed} ${label}"; fi
}

mapfile -t scripts < <(git ls-files '*.sh')

# The versions, printed rather than assumed.
#
# Neither linter is pinned in either environment: a workstation has whatever it
# installed and CI has whatever its runner image ships, and the two disagree.
# (This paragraph does not begin with the linter's name because a comment that
# starts with it is parsed as a directive, not as prose -- SC1072.)
# That is not hypothetical -- `[ ! -z "$x" ]` in server/test_auth.sh passed this
# gate on shellcheck 0.11.0 at --severity=style and was rejected by the version
# on ubuntu-latest, so the first anybody heard of it was a red CI job on a tree
# that linted clean locally.
#
# Printing both versions in both logs makes that difference a line you can read
# instead of a contradiction you have to reproduce. It does not resolve it:
# pinning the linter is a real decision with a real cost -- a pin stops new
# checks arriving as well as stopping surprises -- and it is not one to take
# silently in the middle of something else.
printf '\n=== tool versions ===\n'
printf 'shellcheck: %s\n' "$(shellcheck --version 2>/dev/null | awk '/^version:/{print $2}' || echo 'not installed')"
printf 'shfmt:      %s\n' "$(shfmt --version 2>/dev/null || echo 'not installed')"

# -x so that `source e2e/lib.sh` and `source e2e/images.env` are actually
# followed. Without it shellcheck reports SC1091 and, more importantly, cannot
# see the variables those files define -- so it would miss a genuine typo.
run "shellcheck" shellcheck -x --severity=style "${scripts[@]}"

# shfmt is the shell half of the black-equivalent: total, non-negotiable, and
# not configurable per-file. -d fails on any difference rather than rewriting.
if command -v shfmt >/dev/null 2>&1; then
  run "shfmt" shfmt -d -i 2 -ci -bn -s "${scripts[@]}"
else
  printf '\n=== shfmt ===\nshfmt not installed; refusing to report a clean tree\n'
  failed="${failed} shfmt(missing)"
fi

# ---------------------------------------------------------------------------
# The no-backdoors rule, enforced rather than documented
# ---------------------------------------------------------------------------
# Every row the stack battery asserts on is created through the shipping HTTP
# API or the shipping CLI. The one permitted database access is `sql_ro` in
# e2e/stack.sh, which sets PGOPTIONS so the *server* refuses a write.
#
# A rule of this kind written only as a comment lasts exactly until the first
# stage that would be quicker to write with an INSERT. So it is a check: any
# other psql invocation under e2e/ fails the lint, and the fix is to create the
# row the way a user would.
#
# Comments are stripped before the search. The rule is about invocations, and
# the two files that explain the rule necessarily contain the word -- a check
# that fired on its own rationale would be switched off within the week.
check_no_backdoors() {
  local f line offenders=''
  while IFS= read -r f; do
    case "${f}" in
      e2e/stack.sh | e2e/lint.sh) continue ;;
    esac
    line="$(sed -e 's://.*$::' -e 's:#.*$::' "${f}" | grep -n 'psql' || true)"
    [[ -n ${line} ]] && offenders="${offenders}${f}:${line}"$'\n'
  done < <(git ls-files 'e2e/*.sh' 'e2e/*.mjs' 'e2e/*/*.sh' 'e2e/*/*.mjs')

  if [[ -z ${offenders} ]]; then
    echo "no psql invocation outside sql_ro"
    return 0
  fi
  echo "psql outside e2e/stack.sh's sql_ro:"
  printf '%s' "${offenders}"
  return 1
}
run "no-backdoors" check_no_backdoors

# ---------------------------------------------------------------------------
# The drivers parse
# ---------------------------------------------------------------------------
# `node --check` is not a linter, but it is the difference between a syntax
# error surfacing here and surfacing as a stage that "produced no cases" forty
# minutes into a session.
check_drivers() {
  local f rc=0
  for f in e2e/drivers/*.mjs; do
    [[ -e ${f} ]] || continue
    if node --check "${f}"; then
      echo "ok ${f}"
    else
      rc=1
    fi
  done
  return "${rc}"
}
if command -v node >/dev/null 2>&1; then
  run "drivers" check_drivers
else
  printf '\n=== drivers ===\nnode not installed; refusing to report a clean tree\n'
  failed="${failed} drivers-missing-node"
fi

# ---------------------------------------------------------------------------
# The fuzz tier's work list matches the route table
# ---------------------------------------------------------------------------
# e2e/corpus/endpoints.json is generated from server/tests/common/mod.rs, which
# is itself asserted equal to the router by checks/tests/route_table_matches.rs.
# Generating it at run time instead would mean a route deleted by accident
# vanished from the fuzz list too, silently -- so it is committed, and this
# regenerates and diffs.
#
# Adding a route therefore fails here until somebody runs
# `node e2e/gen-endpoints.mjs --write`, which is a deliberate act meaning "yes,
# this endpoint should be fuzzed", and the result is reviewable in the diff
# rather than materialising invisibly inside a test run.
check_endpoint_inventory() {
  local tmp
  tmp="$(mktemp)"
  node e2e/gen-endpoints.mjs >"${tmp}" || {
    rm -f "${tmp}"
    return 1
  }
  if diff -u e2e/corpus/endpoints.json "${tmp}"; then
    rm -f "${tmp}"
    echo "endpoints.json is in step with the route table"
    return 0
  fi
  rm -f "${tmp}"
  echo "endpoints.json has drifted; run: node e2e/gen-endpoints.mjs --write"
  return 1
}
if command -v node >/dev/null 2>&1; then
  run "endpoint-inventory" check_endpoint_inventory
fi

# ---------------------------------------------------------------------------
# The corpus parses
# ---------------------------------------------------------------------------
check_corpus() {
  node -e '
    const c = require("./e2e/corpus/hostile.json")
    const n = c.strings.length + c.scalars.length + c.timestamps.length
    if (n < 40) { console.error("corpus has only " + n + " entries"); process.exit(1) }
    console.log("corpus: " + c.strings.length + " strings, " + c.scalars.length + " scalars, " + c.timestamps.length + " timestamps")
  '
}
if command -v node >/dev/null 2>&1; then
  run "corpus" check_corpus
fi

# ---------------------------------------------------------------------------
# The Tier 9 oracle, before the tier
# ---------------------------------------------------------------------------
# Tier 9's whole value rests on its invariants being right: a journey that runs
# a thousand actions past a broken invariant reports a thousand successes, and
# nothing in the output distinguishes that from a healthy system.
#
# So the self-test feeds each invariant what a broken server would send and
# requires it to fire -- and requires it to stay quiet on a healthy world, which
# is the half that catches an invariant that fires on everything.
#
# It runs here rather than in the stack battery because it needs no stack, no
# database and no network. The cheapest thing to run is the thing that decides
# whether the most expensive thing means anything.
if command -v node >/dev/null 2>&1; then
  run "tier-9-oracle" node e2e/journeys/selftest.mjs
  run "groupsio-oracle" node e2e/journeys/groupsio-selftest.mjs
  run "stripe-oracle" node e2e/journeys/stripe-selftest.mjs
  run "toolbilling-oracle" node e2e/journeys/toolbilling-selftest.mjs
fi

printf '\n'
if [[ -n ${failed} ]]; then
  echo "FAILED:${failed}"
  exit 1
fi
echo "all e2e checks passed"
