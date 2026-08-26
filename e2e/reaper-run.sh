#!/usr/bin/env bash
#
# Entry point for an ephemeral-session harness (reaper). Two jobs, neither of
# which belongs inside run.sh.
#
# 1. Exit status. reaper hands `run.cmd` to /bin/sh, which is dash, and dash has
#    no `pipefail` -- so `run.sh | tee log` exits with *tee's* status. A failing
#    suite would be reported as a pass, and because @pristine is taken after the
#    first successful run, every later `reaper reset` would then return to a
#    state that was never good. This wrapper owns the pipeline under a shell it
#    chose. (Not hypothetical: the same trap produced a false "the server
#    compiles" reading while this tenant was being planned, from `cargo check
#    ... | tail`.)
#
# 2. Artifacts. Traces, JUnit files and evidence live on a machine scheduled for
#    destruction. Anything under $REAPER_OUT is collected back continuously --
#    but only if it is put there, and it must be put there whether the suite
#    passed or failed, because failure is the case that needs it.
#
# Outside a session $REAPER_OUT is unset and everything lands in e2e/out/, so
# this is runnable by hand to reproduce what a session did.
set -Eeuo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${HERE}/.." && pwd)"
OUT="${REAPER_OUT:-${HERE}/out}"

# `reaper test` empties the workstation's out/ before it starts, but the session
# keeps whatever it had and the backward sync never deletes. Clearing guest-side
# too is what makes out/ mean *this* run at both ends -- otherwise a trace from
# three cycles ago sits beside a fresh one looking identical.
rm -rf "${OUT:?}/junit" "${OUT:?}/logs" "${OUT:?}/evidence" \
       "${OUT:?}/playwright-report" "${OUT:?}/test-results"
mkdir -p "${OUT}"/{junit,logs}

status=0
# pipefail is set, so this is the suite's status and not tee's.
"${HERE}/run.sh" "$@" 2>&1 | tee "${OUT}/e2e.log" || status=$?

collect() { # collect <src> <name-under-out>
  local src="$1" name="$2"
  [[ -e "${src}" ]] || return 0
  rm -rf "${OUT:?}/${name}"
  cp -R "${src}" "${OUT}/${name}"
  echo "collected ${name}"
}
collect "${ROOT}/frontend/playwright-report" playwright-report
collect "${ROOT}/frontend/test-results"      test-results

# Which of these belong to *this* run. The backward sync never deletes -- it is
# not authoritative for what was in out/ beforehand -- so without this a stale
# artifact is indistinguishable from a fresh one.
{
  echo "run finished: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  echo "suite exit:   ${status}"
  echo "arguments:    $*"
  echo "build:"
  sed 's/^/  /' "${ROOT}/e2e/artifacts/BUILD.txt" 2>/dev/null || echo "  (no BUILD.txt)"
} > "${OUT}/RUN.txt"

exit "${status}"
