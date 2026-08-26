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
# shellcheck source=e2e/stack.sh
source "${HERE}/stack.sh"

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
STAGES_ALL="preflight,up,schema,restart,contract,fuzz,concurrency,health,down"
STAGES_DEFAULT="preflight,up,schema,restart,contract,fuzz,concurrency,health,down"

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
    --only)
      STAGES="$2"
      shift 2
      ;;
    --only=*)
      STAGES="${1#*=}"
      shift
      ;;
    --skip)
      STAGES="$(printf '%s' "${STAGES_DEFAULT}" | tr ',' '\n' \
        | grep -vxF -e "${2//,/$'\n'}" | paste -sd, -)"
      shift 2
      ;;
    --provision)
      PROVISION="$2"
      shift 2
      ;;
    --provision=*)
      PROVISION="${1#*=}"
      shift
      ;;
    --engine)
      ENGINE="$2"
      shift 2
      ;;
    --engine=*)
      ENGINE="${1#*=}"
      shift
      ;;
    --list-stages)
      printf '%s\n' "${STAGES_ALL//,/$'\n'}"
      exit 0
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *) die "unknown argument: $1 (try --help)" ;;
  esac
done

[[ ${STAGES} == "all" ]] && STAGES="${STAGES_ALL}"
[[ ${STAGES} == "default" ]] && STAGES="${STAGES_DEFAULT}"

case "${PROVISION}" in
  podman) ENGINE="${ENGINE:-podman}" ;;
  docker) ENGINE="${ENGINE:-docker}" ;;
  external) ENGINE="${ENGINE:-}" ;;
  *) die "--provision must be podman, docker or external (got '${PROVISION}')" ;;
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
  # here: no jq, no python3, no unzip, and no git. Everything below is a shell
  # builtin, coreutils, or the engine.
  #
  # `git` is deliberately absent. The ubuntu-26.04 template carries
  # ZFS, podman, rsync and a guest agent -- no git -- and the run verb executes
  # on the host rather than in the toolchain image. preflight found that on the
  # first real run, which is what it is for.
  local t
  for t in sed grep awk tr sort comm install find; do
    if command -v "${t}" >/dev/null 2>&1; then
      record_case "tool/${t}" ok
    else
      record_case "tool/${t}" fail "not on PATH"
    fi
  done

  # --- the container engine ------------------------------------------------
  if [[ ${PROVISION} == "external" ]]; then
    record_case "engine" skip "--provision=external: the caller supplies the stack"
    if [[ -n ${CSS_TEST_DATABASE_URL:-} ]]; then
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

    # Staleness, checked without git.
    #
    # The risk is running the suite against binaries built from different code
    # -- a `reaper run` after an edit but without a `reaper build`. The obvious
    # check is `git rev-parse HEAD` against the commit build.sh recorded, and
    # that is what this used to do; it failed on the first real session run
    # because the guest has no git, and BUILD.txt's commit came from the
    # toolchain container, which does. Comparing against a value that is
    # always "unknown" on one side is a check that always fails, which is only
    # marginally better than one that always passes.
    #
    # Modification time answers the same question with tools that are actually
    # present: if any source file is newer than the artifacts, the artifacts
    # are stale, whatever commit they claim.
    local newer
    newer="$(find server/src cli/src edge/src css_lib/src checks/src \
      Cargo.toml Cargo.lock -newer e2e/artifacts/BUILD.txt 2>/dev/null | head -5)"
    if [[ -z ${newer} ]]; then
      record_case "artifacts/not-stale" ok
    else
      record_case "artifacts/not-stale" fail \
        "source is newer than BUILD.txt; run e2e/build.sh. First: ${newer//$'\n'/, }"
    fi

    # And when git *is* available -- the workstation, and CI -- take the
    # stronger reading too.
    if command -v git >/dev/null 2>&1 && git rev-parse HEAD >/dev/null 2>&1; then
      local built_commit head_commit
      built_commit="$(awk '/^commit:/ {print $2}' e2e/artifacts/BUILD.txt)"
      head_commit="$(git rev-parse HEAD)"
      assert_eq "artifacts/commit-matches-tree" "${head_commit}" "${built_commit}"
    else
      record_case "artifacts/commit-matches-tree" skip "no git here; the modification-time check above is what covers staleness"
    fi
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

  if [[ -z ${manifest_digests} ]]; then
    record_case "images/manifest-parsed" fail \
      "found no [run] images in .reaper.toml -- the comparison below would pass vacuously"
  else
    record_case "images/manifest-parsed" ok
  fi
  if [[ ${manifest_digests} == "${env_digests}" ]]; then
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
  if [[ ${avail_kb} -ge ${floor_kb} ]]; then
    record_case "disk/free-above-8GiB" ok
  else
    record_case "disk/free-above-8GiB" fail \
      "$((avail_kb / 1024)) MiB free at $(pwd); this suite needs 8 GiB"
  fi

  emit_junit preflight "provision=${PROVISION}" "engine=${ENGINE:-none}"
}

# ===========================================================================
# up -- the stack, brought up in the order the code requires
# ===========================================================================
# The order is not a preference. `MqttService::new` calls `connect().wait()?`
# and `main.rs` propagates the error, so with `edge_enabled` the server will not
# boot at all if the broker is down -- and the resulting message reads like a
# configuration fault rather than an ordering one. Postgres first because the
# server connects to it during the same boot, mosquitto second, server last.
stage_up() {
  cases_begin up
  stack_paths

  if [[ ${PROVISION} == "external" ]]; then
    # CI supplies the services. What it cannot supply is the assurance that
    # they are the ones this suite expects, so the checks below still run --
    # against whatever is there.
    record_case "up/provisioned-externally" skip "the caller supplied postgres and mqtt"
    [[ -n ${CSS_TEST_DATABASE_URL:-} ]] \
      || {
        record_case "up/database-url" fail "CSS_TEST_DATABASE_URL is unset"
        emit_junit up
        return 1
      }
    parse_external_database_url "${CSS_TEST_DATABASE_URL}"
    record_case "up/database-url" ok
  else
    stack_rm_quiet
    start_postgres
    if wait_for "postgres" 90 pg_ready; then
      record_case "up/postgres" ok
    else
      record_case "up/postgres" fail "never became ready; see logs/postgres.log"
      collect_stack_logs
      emit_junit up
      return 1
    fi

    start_mosquitto
    if wait_for "mosquitto" 30 tcp_open "${MQTT_PORT}"; then
      record_case "up/mosquitto" ok
    else
      record_case "up/mosquitto" fail "never accepted a connection on ${MQTT_PORT}"
      collect_stack_logs
      emit_junit up
      return 1
    fi
  fi

  # The snapshot is taken here -- after the cluster exists and *before*
  # css-server has connected once. Left to itself reaper snapshots after a
  # whole successful run, which would capture the migrated schema and every row
  # the suite created, and quietly retire the migration and bootstrap
  # assertions the schema and restart stages exist for.
  if [[ -n ${REAPER_CONTROL:-} && -x "${REAPER_CONTROL}/snapshot" ]]; then
    # REAPER_CONTROL is a directory of executables, not a socket. `snapshot`
    # keeps the first point it is given, so calling it on every run is the
    # intended use rather than something to guard against.
    if "${REAPER_CONTROL}/snapshot" >>"${OUT}/logs/reaper-control.log" 2>&1; then
      record_case "up/snapshot-before-migrations" ok
    else
      record_case "up/snapshot-before-migrations" fail \
        "the control channel refused the request; see logs/reaper-control.log"
    fi
  else
    record_case "up/snapshot-before-migrations" skip \
      "no REAPER_CONTROL; later runs replay against whatever state this one leaves"
  fi

  if [[ ${PROVISION} != "external" ]]; then
    if build_runtime_image; then
      record_case "up/runtime-image" ok
    else
      record_case "up/runtime-image" fail "see logs/runtime-image.log"
      emit_junit up
      return 1
    fi
  fi

  # The frontend bundle. The server's fallback is a ServeDir over it, and an
  # absent directory turns every non-API path into a 404 that looks like a
  # routing defect three stages later.
  if [[ -s "${ROOT}/frontend/dist/index.html" ]]; then
    record_case "up/frontend-bundle" ok
  else
    # A hard stop, not a recorded failure. Without the bundle the server's
    # bind mount does not resolve, the container never starts, and every stage
    # after this one reports a connection refused -- twenty failures all
    # describing one missing directory, with the actual cause four screens up.
    record_case "up/frontend-bundle" fail \
      "frontend/dist/index.html is missing or empty -- run e2e/build.sh"
    emit_junit up
    return 1
  fi

  write_stack_config
  record_case "up/config-written" ok

  start_server
  if wait_for "css-server" 120 server_ready; then
    record_case "up/css-server" ok
  else
    record_case "up/css-server" fail "never answered /status; see logs/css-server.log"
    collect_stack_logs
    emit_junit up
    return 1
  fi

  collect_stack_logs
  emit_junit up "encoding=${PG_ENCODING}" "tz=${STACK_TZ}" "provision=${PROVISION}"
}

pg_ready() {
  if [[ ${PROVISION} == "external" ]]; then
    tcp_open "${PG_PORT}"
  else
    pm exec "${C_PG}" pg_isready -h 127.0.0.1 -p "${PG_PORT}" -U "${PG_USER}"
  fi
}

server_ready() { [[ "$(http_status /status)" == "200" ]]; }

# postgresql://user:pass@host:port/db -- only the pieces sql_ro needs.
parse_external_database_url() {
  local url="${1#*://}"
  local creds="${url%%@*}" rest="${url#*@}"
  PG_USER="${creds%%:*}"
  PG_PASS="${creds#*:}"
  local hostport="${rest%%/*}"
  PG_PORT="${hostport##*:}"
  PG_DB="${rest#*/}"
  PG_DB="${PG_DB%%\?*}"
}

# ===========================================================================
# schema -- what the migrations actually built, on a hostile cluster
# ===========================================================================
stage_schema() {
  cases_begin schema
  stack_paths

  # --- the cluster is the hostile one we asked for -------------------------
  # Without this, every assertion below runs against whatever encoding the
  # image defaulted to, and the stage reports a pass for hostility it never
  # applied. Postgres has no migration path out of an encoding, so getting this
  # wrong is not recoverable within a run -- it has to fail here.
  assert_eq "schema/encoding" "${PG_ENCODING}" "$(sql_ro "SELECT pg_encoding_to_char(encoding) FROM pg_database WHERE datname = current_database()" | tr -d ' ')"
  assert_eq "schema/lc-collate" "C" "$(sql_ro "SELECT datcollate FROM pg_database WHERE datname = current_database()" | tr -d ' ')"
  assert_eq "schema/lc-ctype" "C" "$(sql_ro "SELECT datctype FROM pg_database WHERE datname = current_database()" | tr -d ' ')"

  # --- sql_ro is read-only, proven rather than asserted --------------------
  # The whole no-backdoors rule rests on this connection refusing writes. If
  # PGOPTIONS stopped being applied -- a psql version change, an engine that
  # drops -e, a typo -- every later stage could silently seed its own rows and
  # nothing would say so.
  if sql_ro "CREATE TABLE e2e_backdoor_probe (x int)" >/dev/null 2>&1; then
    record_case "schema/sql_ro-refuses-writes" fail \
      "the read-only connection accepted a CREATE TABLE; the no-backdoors rule is not in force"
  else
    record_case "schema/sql_ro-refuses-writes" ok
  fi

  # --- the migrations ran, all of them ------------------------------------
  local applied declared
  applied="$(sql_ro "SELECT count(*) FROM __diesel_schema_migrations" | tr -d ' ')"
  declared="$(find "${ROOT}/server/migrations" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')"
  assert_eq "schema/migrations-all-applied" "${declared}" "${applied}"
  if [[ ${declared} -lt 20 ]]; then
    record_case "schema/migrations-counted" fail \
      "only ${declared} migration directories found; the comparison above is vacuous"
  else
    record_case "schema/migrations-counted" ok
  fi

  # --- the tables the tiers below depend on exist -------------------------
  # Named individually rather than counted. A count tells you the number
  # changed; a name tells you which table went missing, and these are the ones
  # whose absence would make a later stage fail somewhere unrecognisable.
  local t
  for t in users doors door_access_rules door_access_events schedules tools \
    space_devices profile_config_versions webhooks toolguard_tools; do
    if [[ "$(sql_ro "SELECT to_regclass('public.${t}') IS NOT NULL" | tr -d ' ')" == "t" ]]; then
      record_case "schema/table/${t}" ok
    else
      record_case "schema/table/${t}" fail "not present after migration"
    fi
  done

  # --- the profile-config bootstrap ran exactly once ----------------------
  # main.rs:145 inserts a version when `get_latest_profile_config_version()`
  # returns None. This is the baseline the restart stage compares against.
  local versions
  versions="$(sql_ro "SELECT count(*) FROM profile_config_versions" | tr -d ' ')"
  assert_eq "schema/profile-config-bootstrapped-once" "1" "${versions}"

  # --- byte-order collation is actually in force --------------------------
  # lc_collate=C is what makes every "alphabetical" list in the frontend an
  # assertion rather than a hope. Under a UTF-8 collation 'B' sorts before 'a';
  # under C it does not, and that difference is the check.
  assert_eq "schema/collation-is-byte-order" "B|a" \
    "$(sql_ro "SELECT string_agg(v, '|' ORDER BY v) FROM (VALUES ('a'),('B')) AS t(v)" | tr -d ' ')"

  # --- the timezone the server sees ---------------------------------------
  # chrono::Local silently falls back to UTC when tzdata is absent, which would
  # make every timezone assertion in the suite a no-op that passes.
  assert_eq "schema/timezone-is-not-utc" "${STACK_TZ}" \
    "$(sql_ro "SHOW timezone" | tr -d ' ')"

  emit_junit schema "encoding=${PG_ENCODING}" "migrations=${declared}"
}

# ===========================================================================
# restart -- the idempotence nothing else can see
# ===========================================================================
# `main.rs`'s profile-config bootstrap is not tracked by diesel. It reads
# `get_latest_profile_config_version()` and inserts when the answer is None. A
# second boot must find Some and insert nothing. If that guard ever inverts,
# `profile_config_versions` grows a row per boot -- and no unit test, no
# contract test and no fuzz run would notice, because each of them only ever
# sees one boot.
stage_restart() {
  cases_begin restart
  stack_paths

  local before after mig_before mig_after
  before="$(sql_ro "SELECT count(*) FROM profile_config_versions" | tr -d ' ')"
  mig_before="$(sql_ro "SELECT count(*) FROM __diesel_schema_migrations" | tr -d ' ')"

  if [[ -z ${before} ]]; then
    record_case "restart/baseline-read" fail "could not read the baseline; the stack is not up"
    emit_junit restart
    return 1
  fi
  record_case "restart/baseline-read" ok

  stop_server
  # A stopped server that never actually stopped would make the restart a
  # no-op and every assertion below pass for the wrong reason.
  if wait_for "css-server to stop" 30 server_stopped; then
    record_case "restart/server-stopped" ok
  else
    record_case "restart/server-stopped" fail "still answering /status after 30s"
    emit_junit restart
    return 1
  fi

  start_server
  if wait_for "css-server" 120 server_ready; then
    record_case "restart/server-booted-again" ok
  else
    record_case "restart/server-booted-again" fail "second boot never answered /status"
    collect_server_log
    emit_junit restart
    return 1
  fi

  after="$(sql_ro "SELECT count(*) FROM profile_config_versions" | tr -d ' ')"
  mig_after="$(sql_ro "SELECT count(*) FROM __diesel_schema_migrations" | tr -d ' ')"

  assert_eq "restart/profile-config-not-duplicated" "${before}" "${after}"
  assert_eq "restart/migrations-not-reapplied" "${mig_before}" "${mig_after}"

  # And the versions that do exist are a contiguous run from 1. A bootstrap
  # that inserted and then rolled back would leave the count right and the
  # numbering wrong, which is exactly the shape the Tier 9 oracle models.
  assert_eq "restart/versions-are-contiguous-from-1" "1" \
    "$(sql_ro "SELECT CASE WHEN min(version) = 1 AND max(version) = count(*) THEN 1 ELSE 0 END FROM profile_config_versions" | tr -d ' ')"

  collect_server_log
  emit_junit restart
}

server_stopped() { ! server_ready; }

# ===========================================================================
# health -- the two endpoints main.rs composes outside /api

# ===========================================================================
# contract -- the authorization surface, against a real database
# ===========================================================================
# Everything the offline Tier 4 matrix structurally cannot answer: that a valid
# credential is *accepted*, that the 24 device pairs it deferred are refused
# with 401 rather than faulting, and what the application does with input a
# LATIN1 cluster refuses to store. The driver carries the reasoning.
stage_contract() {
  cases_begin contract
  stack_paths

  if ! server_ready; then
    record_case "contract/stack-is-up" fail "css-server is not answering; run the up stage first"
    emit_junit contract
    return 1
  fi
  record_case "contract/stack-is-up" ok

  # The driver's exit status is deliberately ignored here and its cases are
  # absorbed instead: "the driver exited 1" is not a test report, and a driver
  # that failed one case out of forty should show one failure.
  run_node contract.mjs >"${OUT}/logs/contract.log" 2>&1 || true
  absorb_driver_cases || true

  collect_server_log
  emit_junit contract "driver=contract.mjs"
}

# ===========================================================================
# fuzz -- Tier 7, seeded, against the live stack
# ===========================================================================
# The cheapest defect-per-line in this suite. Three oracles that need no model
# of any endpoint -- no 5xx, well-formed envelope, still alive -- applied to
# all 164 of them. The driver carries the reasoning and the replay caveat.
stage_fuzz() {
  cases_begin fuzz
  stack_paths

  if ! server_ready; then
    record_case "fuzz/stack-is-up" fail "css-server is not answering; run the up stage first"
    emit_junit fuzz
    return 1
  fi
  record_case "fuzz/stack-is-up" ok

  run_node fuzz.mjs >"${OUT}/logs/fuzz.log" 2>&1 || true
  absorb_driver_cases || true

  # The seed lands in the JUnit properties as well as in a case, because CI
  # renders properties beside the failures and a finding whose seed is three
  # screens away in a log is a finding nobody replays.
  local seed
  seed="$(awk '/^fuzz seed:/ {print $3; exit}' "${OUT}/logs/fuzz.log" 2>/dev/null || true)"
  echo "${seed}" >"${STACK_DIR}/fuzz-seed.txt"

  collect_server_log
  emit_junit fuzz "seed=${seed:-unknown}" \
    "iterations=${CSS_FUZZ_ITERATIONS:-400}"
}

# ===========================================================================
# concurrency -- Tier 8
# ===========================================================================
# Two races this codebase is known to have, each asserted on the resource
# rather than on the response tally, and each paired with a sequential sibling
# so that a failure to reproduce is distinguishable from a broken setup.
stage_concurrency() {
  cases_begin concurrency
  stack_paths

  if ! server_ready; then
    record_case "concurrency/stack-is-up" fail "css-server is not answering"
    emit_junit concurrency
    return 1
  fi
  record_case "concurrency/stack-is-up" ok

  run_node concurrency.mjs >"${OUT}/logs/concurrency.log" 2>&1 || true
  absorb_driver_cases || true

  collect_server_log
  emit_junit concurrency \
    "fanout=${CSS_RACE_FANOUT:-8}" "rounds=${CSS_RACE_ROUNDS:-3}"
}
# ===========================================================================
# They are outside `api_routes()`, so the Tier 4 matrix cannot see them at all:
# it builds its router from `api::api_routes()` and would report full coverage
# of a surface that is missing both. This is the only tier that reaches them.
stage_health() {
  cases_begin health
  stack_paths

  assert_eq "health/status-200" "200" "$(http_status /status)"

  # /metrics comes from dr-metrix, which lives entirely in the bin shim. Its
  # absence would mean the release binary was built without the metrics wiring
  # -- invisible to every other tier, since no library test can reach it.
  assert_eq "health/metrics-200" "200" "$(http_status /metrics)"
  if http_head /metrics | grep -q '^# HELP'; then
    record_case "health/metrics-is-prometheus-text" ok
  else
    record_case "health/metrics-is-prometheus-text" fail \
      "no '# HELP' line; the endpoint answered but not with an exposition format"
  fi

  # The SPA fallback. `not_found_service` serves index.html for any unmatched
  # path, which is what makes client-side routing work -- and a missing bundle
  # turns the whole UI into 404s while every API assertion still passes.
  assert_eq "health/spa-fallback" "200" "$(http_status /some/client/side/route)"

  # But not for /api. A fallback that swallowed unknown API paths would turn
  # every typo in the frontend into a silent 200 serving HTML, and the Tier 4
  # matrix's assert_ne 404 would be measuring the wrong thing.
  local api_unknown
  api_unknown="$(http_status /api/definitely-not-a-route)"
  if [[ ${api_unknown} == "404" || ${api_unknown} == "405" ]]; then
    record_case "health/unknown-api-path-is-not-the-spa" ok
  else
    record_case "health/unknown-api-path-is-not-the-spa" fail \
      "expected 404/405, got ${api_unknown}: the SPA fallback is swallowing API paths"
  fi

  emit_junit health
}

# ===========================================================================
# down -- tear the stack down
# ===========================================================================
stage_down() {
  cases_begin down
  stack_paths
  collect_stack_logs
  stop_server
  stop_mosquitto
  stack_rm_quiet
  record_case "down/torn-down" ok
  emit_junit down
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
  if [[ -n ${FAILED_STAGES} ]]; then
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
} >"${OUT}/SUMMARY.md"

if [[ -n ${FAILED_STAGES} ]]; then
  die "failed stages:${FAILED_STAGES}"
fi
log "all requested stages passed"
