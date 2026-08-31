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
#   --provision=external   no container engine. Postgres is the caller's, named by
#                          CSS_TEST_DATABASE_URL; the MQTT broker is started here as
#                          a host process, because a service container cannot be
#                          pointed at a mosquitto config. CSS_TEST_MQTT_URL is NOT
#                          read by anything -- it was named here and never
#                          implemented, which is how CI ran with no broker at all.
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
STAGES_ALL="preflight,up,schema,restart,contract,mfa,fuzz,concurrency,journeys,health,devices,browser,audit,evidence,logs,down"
STAGES_DEFAULT="preflight,up,schema,restart,contract,mfa,fuzz,concurrency,journeys,health,devices,browser,audit,evidence,logs,down"

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

# Every stage named has to exist, and the check belongs here rather than at
# dispatch. Stages are dispatched as `"stage_${stage}"`, so a name with no
# function behind it becomes a command-not-found that is recorded as though the
# stage ran and failed -- which reads as a broken tier rather than as a typo.
#
# `.reaper.toml`'s [profiles.hunt] named `journeys`, a stage that does not
# exist, and would have reported exactly that.
for _stage in ${STAGES//,/ }; do
  case ",${STAGES_ALL}," in
    *",${_stage},"*) ;;
    *) die "no such stage: ${_stage}. Valid: ${STAGES_ALL}" ;;
  esac
done
unset _stage

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

    # The broker is this suite's to start even here, so the binary has to be
    # present. Asserted at preflight rather than left to start_mosquitto,
    # because a missing broker does not degrade the run: css-server refuses to
    # boot without one, so the symptom is a 120-second timeout in `up` and
    # fifteen connection-refused cases across six later stages, none of which
    # says the word "mosquitto" anywhere.
    if command -v mosquitto >/dev/null 2>&1; then
      record_case "tool/mosquitto" ok
    else
      record_case "tool/mosquitto" fail \
        "--provision=external starts the broker itself: apt-get install -y mosquitto"
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
    record_case "up/provisioned-externally" skip \
      "the caller supplied postgres; the broker is started below, here"
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

  fi

  # The broker, and it is started in BOTH provisioning modes.
  #
  # This block lived inside the `else` above until CI ran for the first time,
  # so --provision=external started no broker at all -- while `up` recorded
  # "the caller supplied postgres and mqtt", a claim nothing checked. A missing
  # broker is not a degraded stack, it is no stack: MqttService::new connects
  # during boot and main.rs propagates the failure, so css-server exits before
  # it binds. The suite then waited 120 seconds for a process that was already
  # gone, and six stages produced fifteen "connection refused" cases against a
  # port nothing was ever going to listen on.
  #
  # `start_mosquitto` always had the host-process path this needs; only the call
  # site was missing. "External" means no container engine, not no services:
  # postgres arrives as a service container and the broker cannot, because
  # mosquitto 2 binds to loopback inside its own namespace and a GitHub service
  # container takes no command with which to point one at a config file.
  start_mosquitto
  if wait_for "mosquitto" 30 tcp_open "${MQTT_PORT}"; then
    record_case "up/mosquitto" ok
  else
    record_case "up/mosquitto" fail "never accepted a connection on ${MQTT_PORT}"
    collect_stack_logs
    emit_junit up
    return 1
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
  # whose absence would make a later stage fail somewhere unrecognizable.
  local t
  # Names taken from server/src/schema.rs, not guessed. The first version of
  # this list asked for `toolguard_tools`, which has never existed -- ToolGuard
  # is a feature over `tools.external_id`, not a table -- so the stage reported
  # a missing table on every run and the report was the check's, not the
  # schema's.
  for t in users doors door_access_rules door_access_events door_checkins \
    schedules tools space_devices space_device_auth space_device_auth_requests \
    profile_config_versions webhooks audit_logs places home_links; do
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

  local before after mig_before mig_after config_before config_after
  before="$(sql_ro "SELECT count(*) FROM profile_config_versions" | tr -d ' ')"
  mig_before="$(sql_ro "SELECT count(*) FROM __diesel_schema_migrations" | tr -d ' ')"
  config_before="$(cksum <"${STACK_DIR}/config.toml")"

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

  # The configuration file is the operator's. A boot must not edit it.
  #
  # `AppConfig::from_file` rewrites it in place when a field is missing --
  # backing the original up first, but still replacing what somebody wrote with
  # a merge of their values and a set of defaults, and then refusing to start.
  # For a while this suite was protected from that by mounting the file
  # read-only, which is protection by accident: it went away the moment the
  # mount had to become writable for update_profile_config to work.
  #
  # A checksum is the whole check. It notices the rewrite, and it notices any
  # other write nobody expected.
  config_after="$(cksum <"${STACK_DIR}/config.toml")"
  assert_eq "restart/config-file-untouched-by-boot" "${config_before}" "${config_after}"
  if [[ -n "$(find "${STACK_DIR}" -maxdepth 1 -name 'config.toml.*.backup' 2>/dev/null)" ]]; then
    record_case "restart/no-config-backup-was-written" fail \
      "the loader took the rewrite path; it backed the config up and refused to start"
  else
    record_case "restart/no-config-backup-was-written" ok
  fi

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
# mfa -- Tier 6, the second factor against a real HMAC and a real database
# ===========================================================================
# The only stage that can answer whether a second factor actually gates the
# JWT. Everything cheaper stops one step short of it: the unit tests verify a
# code against a secret with no user, the contract matrix proves the eleven MFA
# routes refuse an anonymous caller but never that one accepts a legitimate
# credential, and the browser tier runs against a fake that accepts a constant.
#
# Placed after `contract` because it registers its own accounts and needs
# nothing the earlier stages leave behind, and before `fuzz` because fuzz
# hammers the same endpoints with hostile input -- a real defect found here
# reads far better than the same defect found as a 500 in a fuzz log.
stage_mfa() {
  cases_begin mfa
  stack_paths

  if ! server_ready; then
    record_case "mfa/stack-is-up" fail "css-server is not answering; run the up stage first"
    emit_junit mfa
    return 1
  fi
  record_case "mfa/stack-is-up" ok

  run_node mfa.mjs >"${OUT}/logs/mfa.log" 2>&1 || true
  absorb_driver_cases || true

  collect_server_log
  emit_junit mfa "driver=mfa.mjs"
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
# Tier 9: simulated users.
#
# The oracle for this tier -- six invariants over the accumulated world, with a
# 20-case self-test that feeds each of them what a broken server would send --
# was written first and had nothing to judge until now. This stage is what gives
# it a world.
#
# It runs after `concurrency` and before `health` on purpose: it leaves a large
# accumulated history behind, and `health` asserting the server still serves
# every basic route afterwards is worth more than it asserting so against a
# nearly-empty database.
stage_journeys() {
  cases_begin journeys
  stack_paths

  if ! tcp_open "${SERVER_PORT}"; then
    record_case "journeys/stack-is-up" fail "css-server is not answering"
    emit_junit journeys
    return 1
  fi
  record_case "journeys/stack-is-up" ok

  if run_node journeys.mjs; then :; else
    log "journeys driver exited non-zero; its cases are recorded below"
  fi
  absorb_driver_cases

  emit_junit journeys "seed=${CSS_JOURNEY_SEED:-random}" \
    "iterations=${CSS_JOURNEY_ITERATIONS:-200}"
}

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
  # Three probes at different depths, because "the SPA fallback is broken" is
  # three different faults and one number cannot tell them apart:
  #
  #   /                    the bundle is where the server was told it is
  #   /tools               a real client-side route, one segment deep
  #   /door/{id}/checkin   the QR flow -- three segments, and the one that
  #                        arrives as a cold deep link from a phone camera,
  #                        which is the case nobody tests by hand because
  #                        clicking through the app never produces it
  #
  # A run where the first two pass and the third does not is a real defect and
  # it is the highest-consequence route in the product.
  assert_eq "health/root-serves-the-app" "200" "$(http_status /)"
  assert_eq "health/spa-route-one-level" "200" "$(http_status /tools)"
  assert_eq "health/spa-route-deep-link" "200" \
    "$(http_status /door/00000000-0000-4000-8000-000000000001/checkin)"

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
# devices -- the one tier that can see a `cfg` branch
# ===========================================================================
# `edge/src/web_server.rs` has two `create_router` functions. The
# `#[cfg(debug_assertions)]` one serves `state.frontend_path` through a
# `ServeDir`; the `#[cfg(not(debug_assertions))]` one serves the `include_dir!`
# embedding and ignores the flag entirely.
#
# So `--frontend-path` -- the subject of fdc887c -- exists only in a debug
# build, and a release binary silently disregards it. No unit test can see that:
# `cargo test` compiles one profile, and whichever one it compiles is the only
# arm that exists as far as the test is concerned. Only running both binaries
# distinguishes them, which is why e2e/build.sh produces both.
#
# The assertion is on the *bytes served*, not on the flag being accepted. A
# binary that takes the flag and ignores it accepts it just as cheerfully.
stage_devices() {
  cases_begin devices
  stack_paths

  local fixture="${STACK_DIR}/edge-frontend"
  local marker="EDGE-FIXTURE-${RANDOM}${RANDOM}"
  mkdir -p "${fixture}"
  cat >"${fixture}/index.html" <<EOF
<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>fixture</title></head>
<body><h1>${marker}</h1></body></html>
EOF

  # An Unauthenticated edge starts the web server and nothing else -- no broker,
  # no server, no registration. That is exactly the surface being asserted here,
  # and it keeps this stage independent of the emoji device codes a non-UTF-8
  # cluster cannot store.
  cat >"${STACK_DIR}/edge.config.toml" <<'EOF'
name = "e2e-edge"
auth_status = "Unauthenticated"
remote_transport = "websocket"
toolguard_sync_interval_secs = 300
calendar_mqtt_topic = "cs/spaces/calendar/events"
calendar_sync_interval_secs = 300
EOF

  local edge_port=8080
  if tcp_open "${edge_port}"; then
    record_case "devices/port-is-free" fail \
      "something is already listening on ${edge_port}; css-edge hardcodes it"
    emit_junit devices
    return 1
  fi
  record_case "devices/port-is-free" ok

  # --- the debug build honours --frontend-path -----------------------------
  if start_edge css-edge-dbg "${fixture}"; then
    if wait_for "css-edge (debug)" 30 edge_ready "${edge_port}"; then
      record_case "devices/debug-binary-starts" ok
      local served
      served="$(http_head / "${edge_port}")"
      if [[ ${served} == *"${marker}"* ]]; then
        record_case "devices/debug-serves-the-path-it-was-given" ok
      else
        record_case "devices/debug-serves-the-path-it-was-given" fail \
          "--frontend-path pointed at a fixture and the marker is not in the response"
      fi
      assert_eq "devices/debug-api-status" "200" "$(http_status /api/status "${edge_port}")"
    else
      record_case "devices/debug-binary-starts" fail "never answered on ${edge_port}"
    fi
  else
    record_case "devices/debug-binary-starts" fail "could not start css-edge-dbg"
  fi
  stop_edge

  # --- the release build ignores it, and says so ---------------------------
  # Not a defect: it is what the cfg split means. Asserted so that the split
  # itself cannot be removed by accident -- if a release build ever started
  # honouring the flag, the embedded bundle would stop being what ships.
  if start_edge css-edge "${fixture}"; then
    if wait_for "css-edge (release)" 30 edge_ready "${edge_port}"; then
      record_case "devices/release-binary-starts" ok
      local served
      served="$(http_head / "${edge_port}")"
      if [[ ${served} == *"${marker}"* ]]; then
        record_case "devices/release-ignores-frontend-path" fail \
          "the release binary served the fixture; the include_dir! embedding is no longer what ships"
      else
        record_case "devices/release-ignores-frontend-path" ok
      fi

      # And what it does serve is a real bundle rather than build.rs's
      # placeholder. An empty embedding compiles cleanly and serves a page
      # saying the UI was not built -- which is a working binary with no
      # interface, and nothing else in this suite would notice.
      if [[ ${served} == *"UI not built"* ]]; then
        record_case "devices/release-embeds-a-real-bundle" fail \
          "css-edge is serving edge/build.rs's placeholder: frontend_edge was not built before cargo ran"
      elif [[ ${served} == *"<div id=\"app\">"* || ${served} == *"<script"* ]]; then
        record_case "devices/release-embeds-a-real-bundle" ok
      else
        record_case "devices/release-embeds-a-real-bundle" fail \
          "the embedded index.html is neither the placeholder nor a built bundle: ${served:0:200}"
      fi
      assert_eq "devices/release-api-status" "200" "$(http_status /api/status "${edge_port}")"
    else
      record_case "devices/release-binary-starts" fail "never answered on ${edge_port}"
    fi
  else
    record_case "devices/release-binary-starts" fail "could not start css-edge"
  fi
  stop_edge

  emit_junit devices "edge_port=${edge_port}"
}

edge_ready() { [[ "$(http_status /api/status "$1")" == "200" ]]; }

# ===========================================================================
# browser -- Tier 5, the real app against the fake API
# ===========================================================================
# Runs Playwright out of the pinned image, which carries its own browsers --
# which is why PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 is set everywhere else and why
# the package.json pin and the image tag have to agree. They do: 1.62.1.
#
# The fake is a Vite middleware, so this stage needs no database, no broker and
# no css-server. It is listed after the stack stages only because that is the
# order somebody reads a report in; it would run just as well first.
#
# Under --provision=external it is skipped with a reason rather than attempted:
# a CI runner has no browsers, and the GitHub workflow runs this in its own
# `browser-fake` job inside the same image. Naming that here means the skip is a
# statement about where the tier runs rather than a gap.
stage_browser() {
  cases_begin browser
  stack_paths

  if [[ ${PROVISION} == "external" ]]; then
    record_case "browser/runs-elsewhere" skip \
      "--provision=external has no browsers; CI runs this in its own browser-fake job inside the pinned Playwright image"
    emit_junit browser
    return 0
  fi

  # node_modules has to be the one e2e/build.sh installed, with the Playwright
  # package in it. Checked rather than assumed: without it `npx playwright`
  # downloads a *different* version into a container that already has browsers
  # for the pinned one, and the failure names a browser executable rather than a
  # missing dependency.
  if [[ ! -d "${ROOT}/frontend/node_modules/@playwright/test" ]]; then
    record_case "browser/playwright-installed" fail \
      "frontend/node_modules/@playwright/test is missing -- run e2e/build.sh"
    emit_junit browser
    return 1
  fi
  record_case "browser/playwright-installed" ok

  log "running the browser tier"
  local rc=0
  pm run --rm --network host \
    -v "${ROOT}/frontend:/app" \
    -w /app \
    -e CI=1 \
    -e PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 \
    "${IMG_PLAYWRIGHT}" \
    npx playwright test --config playwright.config.ts \
    >"${OUT}/logs/browser.log" 2>&1 || rc=$?

  if [[ ${rc} -eq 0 ]]; then
    record_case "browser/playwright" ok
  else
    # The count, from Playwright's own summary line, so the report says how bad
    # rather than only that it was bad.
    local summary
    summary="$(grep -oE '[0-9]+ (failed|passed)' "${OUT}/logs/browser.log" | tr '\n' ' ' || true)"
    record_case "browser/playwright" fail \
      "exit ${rc}: ${summary:-see logs/browser.log}"
  fi

  # Playwright writes its own JUnit; copied in so the run's junit/ is complete
  # rather than having one stage's results somewhere else.
  if [[ -f "${ROOT}/frontend/test-results/playwright-junit.xml" ]]; then
    cp "${ROOT}/frontend/test-results/playwright-junit.xml" "${OUT}/junit/browser-playwright.xml"
    record_case "browser/results-collected" ok
  else
    record_case "browser/results-collected" fail \
      "Playwright wrote no JUnit; the run did not reach the reporter"
  fi

  emit_junit browser
}
# ===========================================================================
# logs -- what the server said that nobody was listening to
# ===========================================================================
# The cheapest oracle in the suite, and it exists because of a specific habit
# this codebase has: `let _ = state.db.create_audit_log(..)`. Discarding that
# result is the right call -- failing a user's request because an audit row
# would not insert is worse than losing the row -- but it means an audit write
# that violates a constraint produces no error anybody sees. The request
# succeeds, the caller is told so, and the event is simply never recorded.
#
# The same shape appears wherever a background task logs and carries on: the
# MQTT reconnect loop, the webhook dispatcher, the pages poller.
#
# So the run's own log is an oracle. Every ERROR line the server emitted is a
# failure of this stage unless it is on the list below, and every entry on that
# list carries the reason it is expected. That turns "the server logged
# something alarming and the suite went green" into a case with a name.
#
# What this does NOT do: judge WARN. The toolguard rejections are warnings and
# they are the suite deliberately sending bad credentials.
# Tier 10: the live browser audit.
#
# Runs the real application against the real server, over the world every stage
# before it has built. Separate from `browser` (Tier 5) because the tiers ask
# opposite questions: Tier 5 injects faults into a fake it controls and asserts
# the application copes; this injects nothing and asserts the server does not
# misbehave on its own data while the UI survives what it really returns.
#
# Placed after `browser` and before `logs` deliberately. It is the last thing to
# touch the server, so the ERROR lines `logs` reads include anything this
# provoked -- a 5xx the browser saw and the server logged is one finding
# reported twice rather than two half-findings.
stage_audit() {
  cases_begin audit
  stack_paths

  if [[ ${PROVISION} == "external" ]]; then
    record_case "audit/runs-elsewhere" skip \
      "--provision=external has no browsers; CI runs Tier 5 in its own Playwright job and this tier needs the full stack, which that job does not have"
    emit_junit audit
    return 0
  fi

  if ! tcp_open "${SERVER_PORT}"; then
    record_case "audit/stack-is-up" fail "css-server is not answering"
    emit_junit audit
    return 1
  fi
  record_case "audit/stack-is-up" ok

  if [[ ! -d "${ROOT}/frontend/node_modules/@playwright/test" ]]; then
    record_case "audit/playwright-installed" fail \
      "frontend/node_modules/@playwright/test is missing -- run e2e/build.sh"
    emit_junit audit
    return 1
  fi
  record_case "audit/playwright-installed" ok

  log "running the live browser audit"
  local rc=0
  pm run --rm --network host \
    -v "${ROOT}/frontend:/app" \
    -w /app \
    -e CI=1 \
    -e PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 \
    -e CSS_BASE_URL="http://127.0.0.1:${SERVER_PORT}" \
    "${IMG_PLAYWRIGHT}" \
    npx playwright test --config playwright.live.config.ts \
    >"${OUT}/logs/audit.log" 2>&1 || rc=$?

  if [[ ${rc} -eq 0 ]]; then
    record_case "audit/playwright" ok
  else
    local failed
    failed="$(grep -oE '[0-9]+ failed' "${OUT}/logs/audit.log" | head -1)"
    record_case "audit/playwright" fail \
      "${failed:-the audit run exited ${rc}}; see logs/audit.log"
  fi

  if [[ -f "${ROOT}/frontend/test-results/playwright-live-junit.xml" ]]; then
    cp "${ROOT}/frontend/test-results/playwright-live-junit.xml" "${OUT}/junit/audit-playwright.xml"
    record_case "audit/results-collected" ok
  else
    record_case "audit/results-collected" fail "playwright wrote no junit output"
  fi

  emit_junit audit
}

# Tier 11: human evidence.
#
# The one tier with no oracle, because the question -- would any of this make
# sense to a newcomer -- is about whether English written for a person does its
# job, and no assertion settles that.
#
# So it produces evidence and makes looking cheap. The journey driver records
# what a human would have been shown at each step; this renders it as prose and,
# more usefully, collects every distinct message the system produced with how
# often and to whom. A suite can prove a route answers 404. Only a reader can
# notice the 404 said "Requested resource not found" to somebody who mistyped a
# tool name, or that a message about a database encoding was shown to a member
# who cannot change one.
#
# It asserts almost nothing on purpose. `fuzz`, `logs` and `audit` already own
# the no-5xx assertion; a fourth would be a fourth place to exempt the same
# known finding.
stage_evidence() {
  cases_begin evidence
  stack_paths

  local src="${STACK_DIR}/journey-transcript.jsonl"
  if [[ ! -s ${src} ]]; then
    record_case "evidence/transcript-present" fail \
      "no journey transcript at ${src}; the journeys stage did not run or wrote nothing"
    emit_junit evidence
    return 1
  fi
  record_case "evidence/transcript-present" ok

  # Rendered on the host with the bootstrapped node -- no container and no
  # stack. The point of a zero-dependency reader is that the evidence is
  # readable on the machine somebody is sitting at, including the FreeBSD
  # workstation where css-server cannot even be built.
  if run_node_host "${ROOT}/e2e/evidence/transcript.mjs" "${src}" \
    >"${OUT}/EVIDENCE.md" 2>"${OUT}/logs/evidence.log"; then
    record_case "evidence/rendered" ok "out/EVIDENCE.md"
  else
    record_case "evidence/rendered" fail "see logs/evidence.log"
    emit_junit evidence
    return 1
  fi

  # The count is the check. A transcript that renders but describes nothing is
  # the failure mode here -- the tier would produce a beautifully formatted
  # account of an empty run and report success.
  local steps
  steps="$(grep -cE '^\{' "${src}" || true)"
  if [[ ${steps:-0} -ge 5 ]]; then
    record_case "evidence/transcript-has-content" ok "${steps} step(s)"
  else
    record_case "evidence/transcript-has-content" fail \
      "only ${steps:-0} step(s) recorded; there is nothing for a person to read"
  fi

  # Surfaced as a case so the distinct-message count appears in the report
  # without anybody opening the file. Not asserted on: a run with more messages
  # is not worse than one with fewer.
  local msgs
  msgs="$(grep -cE '^  [0-9]{3}  x' "${OUT}/EVIDENCE.md" || true)"
  record_case "evidence/distinct-messages" ok "${msgs:-0} distinct message(s) shown to a person"

  emit_junit evidence
}

stage_logs() {
  cases_begin logs
  stack_paths
  collect_server_log

  local log="${OUT}/logs/css-server.log"
  if [[ ! -s ${log} ]]; then
    record_case "logs/server-log-present" fail \
      "no server log to read; every assertion below would pass over nothing"
    emit_junit logs
    return 1
  fi
  record_case "logs/server-log-present" ok

  # --- audit writes that failed silently -----------------------------------
  # Named separately from the general ERROR sweep because the consequence is
  # specific: this is the record of who did what, and it is the one thing a
  # cooperative cannot reconstruct afterwards.
  local audit_failures
  audit_failures="$(grep -c 'Failed to save audit log' "${log}" || true)"
  if [[ ${audit_failures} -eq 0 ]]; then
    record_case "logs/no-audit-write-was-swallowed" ok
  else
    record_case "logs/no-audit-write-was-swallowed" fail \
      "${audit_failures} audit write(s) failed and were discarded: $(grep -m 2 -o 'Failed to save audit log.*' "${log}" | tr '\n' ' ')"
  fi

  # --- everything else the server called an error --------------------------
  #
  # Each exemption covers one message and says why. A pattern broad enough to
  # cover two things covers the next real one too.
  # One entry, and the list is meant to stay short. Two others lived here until
  # the handlers behind them stopped logging a correct 404 at ERROR level --
  # which was the right fix, and is why they are deleted rather than kept as
  # permanent skips. An exemption nobody has to justify again is an exemption
  # that outlives its reason.
  local expected=(
    # A device invite code is eight emoji and this suite's cluster is LATIN1, so
    # the row cannot be written at all. TESTING.md, "Known defects".
    #
    # ERROR is the right level and consistent with the rule `from_db` applies
    # elsewhere, because this route answers 500. It answers 500 because the
    # server generated the value that could not be stored -- the caller supplied
    # nothing -- so it is genuinely the server's failure. The operator whose
    # deployment cannot register any device is the person who needs to see it,
    # and they are the only one who can fix it, by changing the encoding.
    'Failed to insert device invite: character with byte sequence'
  )

  local unexpected=0 sample=''
  local line
  while IFS= read -r line; do
    local matched=0 pattern
    for pattern in "${expected[@]}"; do
      if [[ ${line} == *"${pattern}"* ]]; then
        matched=1
        break
      fi
    done
    if [[ ${matched} -eq 0 ]]; then
      unexpected=$((unexpected + 1))
      [[ -z ${sample} ]] && sample="${line}"
    fi
  done < <(grep 'ERROR' "${log}" || true)

  if [[ ${unexpected} -eq 0 ]]; then
    record_case "logs/no-unexpected-server-errors" ok
  else
    record_case "logs/no-unexpected-server-errors" fail \
      "${unexpected} ERROR line(s) the suite does not expect. First: ${sample:0:400}"
  fi

  # --- and the exemptions are not stale ------------------------------------
  # An exemption for a message that no longer appears is a claim about
  # behavior nobody is checking. It is removed, not left.
  # An exemption for a message that no longer appears is a claim about behavior
  # nobody is checking, so it is reported -- but as a skip rather than a
  # failure. These messages are produced by the fuzz tier reaching for things
  # that do not exist, and a short run legitimately may not reach them. A
  # failure here would make the stage's result depend on the fuzz iteration
  # count, which is exactly the kind of coupling that gets a check deleted.
  local pattern
  for pattern in "${expected[@]}"; do
    if grep -qF "${pattern}" "${log}"; then
      record_case "logs/exemption-still-needed: ${pattern:0:40}" ok
    else
      record_case "logs/exemption-still-needed: ${pattern:0:40}" skip \
        "exempted and did not occur in this run. If a full-length run does not \
produce it either, the exemption is stale and should be deleted."
    fi
  done

  emit_junit logs
}
# ===========================================================================
stage_down() {
  cases_begin down
  stack_paths
  collect_stack_logs
  stop_server
  stop_edge
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

# ---------------------------------------------------------------------------
# SUMMARY.md -- the human-facing report
# ---------------------------------------------------------------------------
# Written last, and written whatever happened. It is what somebody reads when
# they were not watching, so it has to answer three questions without them
# opening anything else: what ran, what did not run *and why*, and what this
# run is not claiming.
#
# The per-case detail is in junit/ and the server's own words are in logs/.
# This is the index.
{
  echo "# e2e run summary"
  echo
  echo "- finished:  $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  echo "- requested: ${STAGES}"
  echo "- available: ${STAGES_ALL}"
  echo "- provision: ${PROVISION}${ENGINE:+ (engine: ${ENGINE})}"
  echo "- cluster:   ${PG_ENCODING}, lc_collate=C, lc_ctype=C, TZ=${STACK_TZ}"
  echo

  if [[ -n ${FAILED_STAGES} ]]; then
    echo "## FAILED:${FAILED_STAGES}"
  else
    echo "## All requested stages passed."
  fi
  echo

  # --- per-stage counts, from the JUnit this run actually wrote -------------
  # Read back rather than accumulated in a variable, so the summary describes
  # the files that exist rather than what the driver believes it did.
  echo "## Stages"
  echo
  echo "| Stage | Cases | Failures | Skipped |"
  echo "|---|---|---|---|"
  local_stage=""
  for local_stage in ${STAGES//,/ }; do
    xml="${OUT}/junit/${local_stage}.xml"
    if [[ -f ${xml} ]]; then
      counts="$(sed -n 's/.*tests="\([0-9]*\)".*failures="\([0-9]*\)".*skipped="\([0-9]*\)".*/\1 | \2 | \3/p' "${xml}" | head -1)"
      echo "| ${local_stage} | ${counts:-? | ? | ?} |"
    else
      echo "| ${local_stage} | (no JUnit written -- the stage died before emitting one) | | |"
    fi
  done
  echo

  # --- seeds ---------------------------------------------------------------
  echo "## Replay"
  echo
  if [[ -s "${STACK_DIR:-}/fuzz-seed.txt" ]]; then
    seed="$(cat "${STACK_DIR}/fuzz-seed.txt")"
    echo "    CSS_FUZZ_SEED=${seed} ./e2e/run.sh --provision=${PROVISION} --only up,fuzz"
    echo
    echo "The seed reproduces the *sequence of choices*, not the run: entity ids"
    echo "differ between runs, so a replay follows a similar path rather than an"
    echo "identical one. Every finding in stack/fuzz-findings.json carries its"
    echo "whole request verbatim, which needs no replay at all."
  else
    echo "No fuzz seed recorded -- the fuzz stage did not run."
  fi
  echo

  # --- what this run is not claiming ---------------------------------------
  echo "## Narrowings in force"
  echo
  echo "- No WebAuthn ceremony is completed anywhere in this run. The passkey"
  echo "  endpoints are driven as far as register/begin and every refusal"
  echo "  around register/finish, but finishing one needs a signed assertion"
  echo "  from a real authenticator and there is none in any environment this"
  echo "  suite runs in. TESTING.md \S7 names the virtual authenticator that"
  echo "  would close it."
  if [[ ${PG_ENCODING} != "UTF8" ]]; then
    echo "- The invite-redemption race was NOT exercised: a device invite code is"
    echo "  eight emoji and this cluster (${PG_ENCODING}) cannot store one. The"
    echo "  finding is asserted instead. To exercise the race itself:"
    echo "  reaper test --profile utf8, or CSS_E2E_DB_ENCODING=UTF8 when running"
    echo "  e2e/run.sh directly. Setting that variable on the workstation does"
    echo "  NOT reach a reaper run -- run.cmd expands in the guest's shell."
  fi
  if [[ ${PROVISION} == "external" ]]; then
    echo "- --provision=external: postgres and the container images are the"
    echo "  caller's. The engine checks in preflight are skipped, because there"
    echo "  is no engine to check."
  fi
  echo "- Assertions named findings/... pin a known defect in place rather"
  echo "  than asserting correct behavior. They FAIL when the defect is fixed,"
  echo "  which is when they should be read and deleted. TESTING.md \S8 lists them."
  echo
  echo "## Where to look"
  echo
  echo "- junit/<stage>.xml  -- every case, written in a trap so a stage that"
  echo "                        died still leaves the failures it had"
  echo "- logs/              -- the components' own words, which is what a"
  echo "                        human reading a failure actually wants"
  echo "- e2e.log            -- the whole run"
  echo "- RUN.txt            -- which artifacts belong to *this* run"
} >"${OUT}/SUMMARY.md"

if [[ -n ${FAILED_STAGES} ]]; then
  die "failed stages:${FAILED_STAGES}"
fi
log "all requested stages passed"
