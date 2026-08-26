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
printf '\n'
if [[ -n ${failed} ]]; then
  echo "FAILED:${failed}"
  exit 1
fi
echo "all e2e checks passed"
