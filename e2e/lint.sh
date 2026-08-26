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
  local label="$1"; shift
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

printf '\n'
if [[ -n "${failed}" ]]; then
  echo "FAILED:${failed}"
  exit 1
fi
echo "all e2e checks passed"
