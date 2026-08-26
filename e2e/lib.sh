# shellcheck shell=bash
#
# Shared helpers for the stack battery. Sourced, never executed.
#
# A note that belongs at the top of the suite rather than buried: pipes are safe
# *here*, because run.sh sets `set -Eeuo pipefail`. They are not safe in
# `.reaper.toml`'s `run.cmd`, which reaper hands to /bin/sh -- dash, which has no
# pipefail. `run.sh | tee log` there exits with tee's status, so a failing suite
# reports as a pass and @pristine is then taken on the strength of it. That is
# why e2e/reaper-run.sh exists and why run.cmd must never contain a pipe.

# ---------------------------------------------------------------------------
# Output
# ---------------------------------------------------------------------------
log()  { printf '[%s] %s\n' "$(date -u '+%H:%M:%S')" "$*"; }
warn() { printf '[%s] WARN  %s\n' "$(date -u '+%H:%M:%S')" "$*" >&2; }
die()  { printf '[%s] FATAL %s\n' "$(date -u '+%H:%M:%S')" "$*" >&2; exit 1; }

stage_banner() { printf '\n========== stage: %s ==========\n' "$1"; }

# ---------------------------------------------------------------------------
# Result recording
# ---------------------------------------------------------------------------
# Cases accumulate into a per-stage TSV and are converted to JUnit at stage end.
# Two rules, both learned the hard way elsewhere:
#
#   * the XML is written in a trap, so a stage that dies mid-way still leaves a
#     file describing the failures it had rather than no file at all;
#   * nothing writes a passing file first and amends it later, because a run
#     that dies between the two leaves artifacts that contradict the exit status,
#     and anything globbing out/*.xml believes the artifacts.

CASE_FILE=""

cases_begin() { # cases_begin <stage>
  CASE_FILE="${OUT}/.cases-${1}.tsv"
  : > "${CASE_FILE}"
}

# record_case <name> <ok|fail|skip> [message]
record_case() {
  local name="$1" status="$2" message="${3:-}"
  printf '%s\t%s\t%s\n' "${name}" "${status}" "${message}" >> "${CASE_FILE}"
  case "${status}" in
    ok)   log "  ok    ${name}" ;;
    fail) log "  FAIL  ${name}${message:+ -- ${message}}" ;;
    skip) log "  skip  ${name}${message:+ -- ${message}}" ;;
  esac
}

# assert_eq <name> <expected> <actual> -- the workhorse.
assert_eq() {
  local name="$1" expected="$2" actual="$3"
  if [[ "${expected}" == "${actual}" ]]; then
    record_case "${name}" ok
  else
    record_case "${name}" fail "expected [${expected}], got [${actual}]"
  fi
}

xml_escape() {
  printf '%s' "$1" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' \
                         -e 's/"/\&quot;/g' -e "s/'/\&apos;/g"
}

# emit_junit <stage> [key=value properties...]
emit_junit() {
  local stage="$1"; shift
  local file="${OUT}/junit/${stage}.xml"
  local total=0 failures=0 skipped=0

  [[ -f "${CASE_FILE}" ]] || { : > "${CASE_FILE}"; }
  while IFS=$'\t' read -r _ status _; do
    total=$((total + 1))
    [[ "${status}" == "fail" ]] && failures=$((failures + 1))
    [[ "${status}" == "skip" ]] && skipped=$((skipped + 1))
  done < "${CASE_FILE}"

  {
    printf '<?xml version="1.0" encoding="UTF-8"?>\n'
    printf '<testsuite name="%s" tests="%d" failures="%d" skipped="%d">\n' \
      "$(xml_escape "${stage}")" "${total}" "${failures}" "${skipped}"
    if [[ $# -gt 0 ]]; then
      printf '  <properties>\n'
      local kv
      for kv in "$@"; do
        printf '    <property name="%s" value="%s"/>\n' \
          "$(xml_escape "${kv%%=*}")" "$(xml_escape "${kv#*=}")"
      done
      printf '  </properties>\n'
    fi
    while IFS=$'\t' read -r name status message; do
      printf '  <testcase classname="%s" name="%s">' \
        "$(xml_escape "${stage}")" "$(xml_escape "${name}")"
      case "${status}" in
        fail) printf '<failure message="%s"/>' "$(xml_escape "${message}")" ;;
        skip) printf '<skipped message="%s"/>' "$(xml_escape "${message}")" ;;
      esac
      printf '</testcase>\n'
    done < "${CASE_FILE}"
    printf '</testsuite>\n'
  } > "${file}"

  rm -f "${CASE_FILE}"
  log "stage ${stage}: ${total} case(s), ${failures} failure(s), ${skipped} skipped"
  return "$(( failures > 0 ? 1 : 0 ))"
}

# ---------------------------------------------------------------------------
# Container engine
# ---------------------------------------------------------------------------
# `pm` is the engine, chosen once. Both podman and docker are supported because
# the reaper guest has podman and the CI runner has docker, and a suite that
# only ran under one of them would leave the other environment untested.
pm() { "${ENGINE}" "$@"; }
