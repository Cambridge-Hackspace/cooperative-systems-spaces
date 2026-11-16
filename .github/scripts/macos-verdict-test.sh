#!/usr/bin/env bash
#
# The test for macos-verdict.sh. CI runs it before trusting the verdict, and it
# runs anywhere with bash and jq.
#
# Two fixtures are real, trimmed from GitHub's API:
#   unacquired.json  run 43930212321 (PR #236): the macOS leg never got a runner
#   built.json       run 43917585450 (dev push): the macOS leg built
# Every other case is derived from those two with jq, so each one differs from
# a real record in exactly the field it is about.
set -Eeuo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
verdict="${here}/macos-verdict.sh"
fixtures="${here}/fixtures"
work=$(mktemp -d)
trap 'rm -rf "${work}"' EXIT

failed=0
cases=0
MAC='.name | startswith("release (macos-latest,")'

# expect <pass|fail> <label> <jobs.json> <event> [text the output must contain]
expect() {
  local want="$1" label="$2" file="$3" event="$4" needle="${5:-}" out rc=0 got
  cases=$((cases + 1))
  out=$("${verdict}" "${file}" "${event}" 2>&1) || rc=$?
  if [[ ${rc} -eq 0 ]]; then got=pass; elif [[ ${rc} -eq 1 ]]; then got=fail; else got="exit ${rc}"; fi
  if [[ ${got} != "${want}" ]]; then
    printf 'NOT OK %s: wanted %s, got %s\n%s\n' "${label}" "${want}" "${got}" "${out}"
    failed=$((failed + 1))
  elif [[ -n ${needle} && ${out} != *"${needle}"* ]]; then
    printf 'NOT OK %s: output lacks "%s"\n%s\n' "${label}" "${needle}" "${out}"
    failed=$((failed + 1))
  else
    printf 'ok %s\n' "${label}"
  fi
}

# derive <name> <source fixture> <jq filter applied to the macOS job>
derive() {
  jq "(.jobs[] | select(${MAC})) |= ($3)" "${fixtures}/$2" >"${work}/$1.json"
}

# The fixtures are what they claim to be; otherwise every case below is vacuous.
jq -e "[.jobs[] | select(${MAC})] | length == 1 and .[0].conclusion == \"cancelled\" and .[0].runner_name == \"\" and (.[0].steps | length) == 0" \
  "${fixtures}/unacquired.json" >/dev/null || {
  echo 'NOT OK unacquired.json no longer holds the no-runner record'
  exit 1
}
jq -e "[.jobs[] | select(${MAC})] | length == 1 and .[0].conclusion == \"success\" and (.[0].steps | length) > 0" \
  "${fixtures}/built.json" >/dev/null || {
  echo 'NOT OK built.json no longer holds a built macOS leg'
  exit 1
}

# The real records.
expect pass 'no runner, on a pull request' "${fixtures}/unacquired.json" pull_request '::warning::'
expect fail 'no runner, on a push' "${fixtures}/unacquired.json" push 'never given a runner'
expect fail 'no runner, on a manual run' "${fixtures}/unacquired.json" workflow_dispatch
expect pass 'built, on a pull request' "${fixtures}/built.json" pull_request 'built'
expect pass 'built, on a push' "${fixtures}/built.json" push 'built'

# A real build failure must fail on every event, including when continue-on-error
# has left the job's own conclusion as success.
derive broken built.json '.conclusion = "failure" | (.steps[] | select(.name | contains("--bin css-edge")) | .conclusion) = "failure"'
expect fail 'build broke, on a pull request' "${work}/broken.json" pull_request 'css-edge (failure)'
expect fail 'build broke, on a push' "${work}/broken.json" push
derive masked built.json '(.steps[] | select(.name | contains("--bin css-edge")) | .conclusion) = "failure"'
expect fail 'build broke but the job says success' "${work}/masked.json" pull_request 'css-edge (failure)'

# Started, then cancelled: it had a runner, so this is not the capacity case.
derive started built.json '.conclusion = "cancelled" | (.steps[] | select(.name | contains("--bin css-cli")) | .conclusion) = "cancelled"'
expect fail 'cancelled mid-build' "${work}/started.json" pull_request 'cancelled'

# Near misses on the no-runner signature, one field each.
derive hadrunner unacquired.json '.runner_name = "GitHub Actions 1"'
expect fail 'no steps but a runner' "${work}/hadrunner.json" pull_request 'not the no-runner signature'
derive skipped unacquired.json '.conclusion = "skipped" | .runner_name = null'
expect fail 'skipped' "${work}/skipped.json" pull_request 'not the no-runner signature'
derive nullrunner unacquired.json '.runner_name = null'
expect pass 'no runner given as null' "${work}/nullrunner.json" pull_request
derive failedq unacquired.json '.conclusion = "failure"'
expect pass 'no runner, recorded as failure' "${work}/failedq.json" pull_request
derive queued unacquired.json '.status = "queued" | .conclusion = null'
expect fail 'still queued' "${work}/queued.json" pull_request 'not completed'

# The job has to be found, exactly once.
jq "del(.jobs[] | select(${MAC}))" "${fixtures}/built.json" >"${work}/missing.json"
expect fail 'no macOS job' "${work}/missing.json" pull_request 'found 0'
jq "(.jobs += [.jobs[] | select(${MAC})])" "${fixtures}/built.json" >"${work}/twice.json"
expect fail 'two macOS jobs' "${work}/twice.json" pull_request 'found 2'

# Usage errors are not verdicts.
cases=$((cases + 1))
rc=0
"${verdict}" >/dev/null 2>&1 || rc=$?
if [[ ${rc} -eq 2 ]]; then
  echo 'ok no arguments is a usage error'
else
  echo "NOT OK no arguments exited ${rc}, not 2"
  failed=$((failed + 1))
fi

echo "${cases} cases, ${failed} failed"
[[ ${failed} -eq 0 ]]
