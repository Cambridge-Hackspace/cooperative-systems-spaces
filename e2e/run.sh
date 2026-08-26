#!/usr/bin/env bash
#
# The stack battery: tiers 6-11 of docs/testing-methodology.md.
#
# Provisioning-agnostic on purpose. reaper is the pre-push loop, but GitHub CI
# is the gate and must be able to run this without reaper existing, so the
# Postgres and MQTT endpoints come from the environment and are only started
# here when --provision says to:
#
#   --provision=podman     start the stack with podman   (the reaper path)
#   --provision=docker     start the stack with docker   (the CI-with-DinD path)
#   --provision=external   use CSS_TEST_DATABASE_URL / CSS_TEST_MQTT_URL as given
#
# Pipes are safe in this file because of `set -Eeuo pipefail` below. They are
# NOT safe in .reaper.toml's run.cmd, which is handed to dash -- see e2e/lib.sh.
set -Eeuo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${HERE}/.." && pwd)"
cd "${ROOT}"

# shellcheck source=e2e/lib.sh
source "${HERE}/lib.sh"
# shellcheck source=e2e/images.env
source "${HERE}/images.env"

OUT="${REAPER_OUT:-${HERE}/out}"
mkdir -p "${OUT}/junit" "${OUT}/logs"

# ---------------------------------------------------------------------------
# Stages
# ---------------------------------------------------------------------------
# Only stages that are actually implemented appear here. A stage listed but
# unimplemented would either fail every run or, worse, pass without doing
# anything -- and a suite that reports green for work it did not do is the
# specific failure this whole exercise exists to prevent.
#
# STAGES_ALL grows as tiers land. TESTING.md tracks what each one covers.
STAGES_ALL="preflight"
STAGES_DEFAULT="preflight"

PROVISION="podman"
ENGINE=""
STAGES="${STAGES_DEFAULT}"

usage() {
  cat <<'EOF'
usage: e2e/run.sh [options]

  --only <a,b,c>     run exactly these stages (or "all", or "default")
  --skip <a,b>       run the default set minus these
  --provision <how>  podman | docker | external   (default: podman)
  --engine <name>    override the container engine binary
  --list-stages      print every implemented stage and exit
  -h, --help         this
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --only)          STAGES="$2"; shift 2 ;;
    --only=*)        STAGES="${1#*=}"; shift ;;
    --skip)          STAGES="$(printf '%s' "${STAGES_DEFAULT}" | tr ',' '\n' \
                        | grep -vxF -e "${2//,/$'\n'}" | paste -sd, -)"; shift 2 ;;
    --provision)     PROVISION="$2"; shift 2 ;;
    --provision=*)   PROVISION="${1#*=}"; shift ;;
    --engine)        ENGINE="$2"; shift 2 ;;
    --engine=*)      ENGINE="${1#*=}"; shift ;;
    --list-stages)   printf '%s\n' "${STAGES_ALL//,/$'\n'}"; exit 0 ;;
    -h|--help)       usage; exit 0 ;;
    *)               die "unknown argument: $1 (try --help)" ;;
  esac
done

[[ "${STAGES}" == "all"     ]] && STAGES="${STAGES_ALL}"
[[ "${STAGES}" == "default" ]] && STAGES="${STAGES_DEFAULT}"

case "${PROVISION}" in
  podman)   ENGINE="${ENGINE:-podman}" ;;
  docker)   ENGINE="${ENGINE:-docker}" ;;
  external) ENGINE="${ENGINE:-}" ;;
  *)        die "--provision must be podman, docker or external (got '${PROVISION}')" ;;
esac

has_stage() { [[ ",${STAGES}," == *",$1,"* ]]; }

# Every named stage must exist. A typo that silently ran nothing would be
# indistinguishable from a pass.
for want in ${STAGES//,/ }; do
  [[ ",${STAGES_ALL}," == *",${want},"* ]] \
    || die "no such stage: '${want}'. Implemented: ${STAGES_ALL}"
done

FAILED_STAGES=""

# ===========================================================================
# preflight -- assert this suite's own preconditions rather than discovering
# them three stages later as something that reads like a product bug.
# ===========================================================================
stage_preflight() {
  cases_begin preflight

  # --- the tools this suite assumes ---------------------------------------
  # Named explicitly so that the next template gap fails in one line with the
  # missing tool's name, rather than deep inside a stage. Note what is NOT
  # here: no jq, no python3, no unzip. Everything below is a shell builtin,
  # coreutils, git, or the engine.
  local t
  for t in git sed grep awk tr sort comm install; do
    if command -v "${t}" >/dev/null 2>&1; then
      record_case "tool/${t}" ok
    else
      record_case "tool/${t}" fail "not on PATH"
    fi
  done

  # --- the container engine ------------------------------------------------
  if [[ "${PROVISION}" == "external" ]]; then
    record_case "engine" skip "--provision=external: the caller supplies the stack"
    if [[ -n "${CSS_TEST_DATABASE_URL:-}" ]]; then
      record_case "env/CSS_TEST_DATABASE_URL" ok
    else
      record_case "env/CSS_TEST_DATABASE_URL" fail \
        "--provision=external requires it; there is nothing to connect to"
    fi
  else
    if command -v "${ENGINE}" >/dev/null 2>&1; then
      record_case "engine/${ENGINE}" ok
      # "Installed" and "able to start a container" are different claims. An
      # engine missing the packet-filter tooling its network backend drives
      # installs cleanly, reports healthy, pulls images, and fails only when
      # something tries to run.
      if pm run --rm "${IMG_RUNTIME}" true >/dev/null 2>&1; then
        record_case "engine/can-run-a-container" ok
      else
        record_case "engine/can-run-a-container" fail \
          "${ENGINE} is installed but could not start a container"
      fi
    else
      record_case "engine/${ENGINE}" fail "not on PATH"
    fi
  fi

  # --- build artifacts match this working tree -----------------------------
  # Without this, a run that skipped the build silently tests yesterday's
  # binaries and reports on code that is not in the tree.
  if [[ -f e2e/artifacts/BUILD.txt ]]; then
    record_case "artifacts/present" ok
    local built_commit head_commit
    built_commit="$(awk '/^commit:/ {print $2}' e2e/artifacts/BUILD.txt)"
    head_commit="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
    assert_eq "artifacts/commit-matches-tree" "${head_commit}" "${built_commit}"
  else
    record_case "artifacts/present" fail \
      "e2e/artifacts/BUILD.txt missing -- run e2e/build.sh first"
  fi

  # --- image digests agree with the manifest -------------------------------
  # reaper pre-pulls what .reaper.toml names; run.sh pulls what images.env
  # names. If they drift, the pre-pull silently stops helping -- or the run
  # uses bytes the manifest does not claim. The duplication is the check.
  # Only the `[run] images` block, not every digest in the file. `build.image`
  # is reaper's toolchain for the build verb; run.sh never pulls it and it has
  # no business in images.env. This narrowing covers exactly that one key --
  # every digest that run.sh could actually use is still compared.
  local manifest_digests env_digests
  manifest_digests="$(sed -n '/^images = \[/,/^]/p' .reaper.toml \
    | grep -oE '@sha256:[0-9a-f]{64}' | sort -u)"
  env_digests="$(grep -oE '@sha256:[0-9a-f]{64}' e2e/images.env | sort -u)"

  if [[ -z "${manifest_digests}" ]]; then
    record_case "images/manifest-parsed" fail \
      "found no [run] images in .reaper.toml -- the comparison below would pass vacuously"
  else
    record_case "images/manifest-parsed" ok
  fi
  if [[ "${manifest_digests}" == "${env_digests}" ]]; then
    record_case "images/digests-agree" ok
  else
    record_case "images/digests-agree" fail \
      "$(printf '%s' "$(comm -3 <(printf '%s\n' "${manifest_digests}") \
                                <(printf '%s\n' "${env_digests}") | tr '\n' ' ')")"
  fi

  # --- disk ---------------------------------------------------------------
  # The Ubuntu guest's boot disk has under 4 GiB free and this suite pulls
  # ~3.5 GiB of images. Naming the number means a future failure says "disk"
  # rather than something that reads like a registry fault.
  local avail_kb floor_kb
  avail_kb="$(df -Pk . | awk 'NR==2 {print $4}')"
  floor_kb=$((8 * 1024 * 1024))
  if [[ "${avail_kb}" -ge "${floor_kb}" ]]; then
    record_case "disk/free-above-8GiB" ok
  else
    record_case "disk/free-above-8GiB" fail \
      "$((avail_kb / 1024)) MiB free at $(pwd); this suite needs 8 GiB"
  fi

  emit_junit preflight "provision=${PROVISION}" "engine=${ENGINE:-none}"
}

# ===========================================================================
# Driver
# ===========================================================================
log "stages: ${STAGES}"
log "provision: ${PROVISION}${ENGINE:+ (engine: ${ENGINE})}"
log "results: ${OUT}"

for stage in ${STAGES//,/ }; do
  stage_banner "${stage}"
  if "stage_${stage}"; then
    :
  else
    FAILED_STAGES="${FAILED_STAGES} ${stage}"
  fi
done

{
  echo "# e2e run summary"
  echo
  echo "- finished: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  echo "- stages requested: ${STAGES}"
  echo "- stages implemented: ${STAGES_ALL}"
  echo "- provision: ${PROVISION}"
  echo
  if [[ -n "${FAILED_STAGES}" ]]; then
    echo "## FAILED:${FAILED_STAGES}"
  else
    echo "## All requested stages passed."
  fi
  echo
  echo "## Narrowings in force"
  echo
  echo "- Stages beyond those listed under 'implemented' do not exist yet."
  echo "  They are absent from STAGES_ALL rather than present-and-skipped, so"
  echo "  this run makes no claim about the tiers they will cover."
} > "${OUT}/SUMMARY.md"

if [[ -n "${FAILED_STAGES}" ]]; then
  die "failed stages:${FAILED_STAGES}"
fi
log "all requested stages passed"
