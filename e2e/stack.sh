# shellcheck shell=bash
#
# The stack: Postgres, an MQTT broker, and css-server, brought up the way the
# tiers below Tier 6 cannot -- as real processes talking to a real database.
#
# Sourced by run.sh, never executed.
#
# ---------------------------------------------------------------------------
# Why everything lives on the host's loopback
# ---------------------------------------------------------------------------
# `[server] bind_address = "127.0.0.1:4399"`. That is not a stylistic choice
# this suite can work around: a container-per-service layout with a network
# namespace each cannot reach a server bound to another namespace's loopback,
# and nothing in the suite would be able to say why. So every component either
# runs on the host directly or runs with `--network host`, and 127.0.0.1 means
# the same thing to all of them. It also means the ports are real ports on a
# real machine, which is why they are all overridable -- see the CSS_E2E_*_PORT
# variables below.
#
# ---------------------------------------------------------------------------
# Why css-server runs in a container here and as a host process under external
# ---------------------------------------------------------------------------
# The binaries are built in `rust:1.97-bookworm` and link libpq dynamically. The
# reaper guest is an Ubuntu template carrying podman, ZFS and rsync and no
# toolchain at all -- no libpq5, and no way to add one that would survive the
# next ephemeral session. So under podman/docker the server runs in a
# bookworm-slim image with libpq5, which is also what the shipping Dockerfile
# does.
#
# Under --provision=external there is by definition no engine, so it runs as a
# host process. That path is CI's, where the runner installs libpq-dev for the
# build anyway. Both paths are therefore exercised by something real on every
# push, rather than one of them being a fallback nobody runs.

# ---------------------------------------------------------------------------
# What the sourcing script must provide
# ---------------------------------------------------------------------------
# Declared rather than left implicit, the same way e2e/lib.sh declares its own.
# The check-unassigned-uppercase rule found every one of these referenced and
# never assigned, and it was right: this file's dependency on run.sh and on
# images.env was a convention nobody had written down.
#
# `OUT`, `ROOT`, `PROVISION`  come from run.sh, which assigns them after
#                             sourcing this file -- so these defaults are
#                             overwritten before any function here runs.
# `IMG_*`                     come from e2e/images.env.
OUT="${OUT:-}"
ROOT="${ROOT:-}"
PROVISION="${PROVISION:-}"
IMG_POSTGRES="${IMG_POSTGRES:-}"
IMG_MOSQUITTO="${IMG_MOSQUITTO:-}"
IMG_RUNTIME="${IMG_RUNTIME:-}"
IMG_NODE="${IMG_NODE:-}"

# Defaulting an image name to the empty string to satisfy a linter is exactly
# the kind of quieting that makes a later failure unreadable: without this,
# a missing `source images.env` becomes `podman run -d --network host ""`,
# which fails complaining about an invalid reference format rather than about
# the file that was not read. The defaults above are for the linter; this is
# for the human.
stack_require_images() {
  local v
  for v in IMG_POSTGRES IMG_MOSQUITTO IMG_RUNTIME IMG_NODE; do
    [[ -n ${!v} ]] || die "${v} is empty -- e2e/images.env was not sourced"
  done
}

STACK_DIR=""
PG_PORT="${CSS_E2E_PG_PORT:-5432}"
MQTT_PORT="${CSS_E2E_MQTT_PORT:-1883}"
SERVER_PORT="${CSS_E2E_SERVER_PORT:-4399}"
PG_USER="css_user"
PG_PASS="css_pass"
PG_DB="css"
# LATIN1 + C, per the hostile-start rationale in TESTING.md. Overridable so the
# nightly can run the harsher SQL_ASCII case without editing the suite.
PG_ENCODING="${CSS_E2E_DB_ENCODING:-LATIN1}"
# Not UTC. A suite that runs entirely in UTC proves nothing about timezone
# handling, and this application converts between a configured space timezone
# and UTC on every schedule comparison.
STACK_TZ="${CSS_E2E_TZ:-America/Chicago}"

C_PG="css-e2e-postgres"
C_MQTT="css-e2e-mosquitto"
C_SERVER="css-e2e-server"
IMG_SERVER_LOCAL="css-e2e-runtime:local"

# ---------------------------------------------------------------------------
# HTTP without curl
# ---------------------------------------------------------------------------
# The guest template carries no curl and no python3, and adding a dependency to
# every environment for the sake of a readiness probe is a poor trade. bash's
# /dev/tcp does the whole job for status-line assertions, and the stages that
# need real request bodies run their drivers in the pinned Node image, where
# fetch is a builtin.
#
# What this does NOT do: chunked decoding, redirects, TLS, or keep-alive. It
# sends HTTP/1.0 so the server closes the connection and `cat` terminates.

# http_status <path> [port] -- prints the numeric status, or nothing on failure.
http_status() {
  local path="$1" port="${2:-${SERVER_PORT}}" line
  line="$(http_head "${path}" "${port}" | head -1 || true)"
  printf '%s' "${line}" | awk '{print $2}'
}

# http_head <path> [port] -- prints the full response (headers and body).
http_head() {
  local path="$1" port="${2:-${SERVER_PORT}}"
  exec 3<>"/dev/tcp/127.0.0.1/${port}" 2>/dev/null || return 1
  printf 'GET %s HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n' "${path}" >&3
  cat <&3
  exec 3<&-
}

# tcp_open <port> -- true when something is listening.
tcp_open() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }

# wait_for <label> <seconds> <command...> -- poll until the command succeeds.
wait_for() {
  local label="$1" limit="$2"
  shift 2
  local waited=0
  while ! "$@" >/dev/null 2>&1; do
    if [[ ${waited} -ge ${limit} ]]; then
      warn "${label}: still not ready after ${limit}s"
      return 1
    fi
    sleep 1
    waited=$((waited + 1))
  done
  log "  ${label} ready after ${waited}s"
  return 0
}

# ---------------------------------------------------------------------------
# Database access, read-only by construction
# ---------------------------------------------------------------------------
# The one place this suite is allowed to touch the database directly. Every row
# the tests assert on is created through the shipping HTTP API or the shipping
# CLI -- never inserted here -- because a test that seeds its own rows proves
# the schema accepts them and nothing about whether the application can produce
# them.
#
# PGOPTIONS makes that a property of the connection rather than a promise: the
# *server* refuses the write, so a `sql_ro "INSERT ..."` that slips in fails
# instead of silently working. e2e/lint.sh greps for any psql invocation outside
# this function, so the rule is enforced rather than documented.
sql_ro() {
  local query="$1"
  if [[ ${PROVISION} == "external" ]]; then
    PGPASSWORD="${PG_PASS}" PGOPTIONS='-c default_transaction_read_only=on' \
      psql -h 127.0.0.1 -p "${PG_PORT}" -U "${PG_USER}" -d "${PG_DB}" -tAc "${query}"
  else
    pm exec \
      -e PGPASSWORD="${PG_PASS}" \
      -e PGOPTIONS='-c default_transaction_read_only=on' \
      "${C_PG}" psql -h 127.0.0.1 -p "${PG_PORT}" -U "${PG_USER}" -d "${PG_DB}" -tAc "${query}"
  fi
}

# ---------------------------------------------------------------------------
# Bring-up
# ---------------------------------------------------------------------------
stack_paths() {
  stack_require_images
  STACK_DIR="${OUT}/stack"
  mkdir -p "${STACK_DIR}" "${OUT}/logs"
}

stack_rm_quiet() {
  [[ ${PROVISION} == "external" ]] && return 0
  local c
  for c in "${C_SERVER}" "${C_MQTT}" "${C_PG}"; do
    pm rm -f "${c}" >/dev/null 2>&1 || true
  done
  return 0
}

# The data directory. Placed on reaper's rollback dataset when there is one, so
# `reset` returns the cluster to the moment before css-server first connected,
# and left in the run directory when there is not, so CI is unaffected.
stack_pgdata() {
  if [[ -n ${REAPER_STATE:-} ]]; then
    printf '%s' "${REAPER_STATE}/pgdata"
  else
    printf '%s' "${STACK_DIR}/pgdata"
  fi
}

start_postgres() {
  local pgdata
  pgdata="$(stack_pgdata)"

  # The postgres image drops to uid 999 and refuses a PGDATA it does not own,
  # exiting immediately with a message that reads like a corrupt volume. Naming
  # the ownership here means that failure never happens.
  mkdir -p "${pgdata}"
  if [[ ${PROVISION} != "external" ]]; then
    # Rootless podman maps the container's 999 to a subuid of the invoking
    # user, so chown-ing to 999 on the host is wrong under rootless and right
    # under docker. --userns=keep-id:uid=999,gid=999 would be the tidy answer
    # but is podman-only. Instead the directory is world-writable and the
    # image's own initdb sets the modes it wants; PGDATA is scratch state that
    # is destroyed with the session.
    chmod 0777 "${pgdata}"
  fi

  log "starting postgres (encoding=${PG_ENCODING}, locale=C, TZ=${STACK_TZ})"
  pm run -d --name "${C_PG}" --network host \
    -e POSTGRES_USER="${PG_USER}" \
    -e POSTGRES_PASSWORD="${PG_PASS}" \
    -e POSTGRES_DB="${PG_DB}" \
    -e POSTGRES_INITDB_ARGS="--encoding=${PG_ENCODING} --lc-collate=C --lc-ctype=C" \
    -e PGDATA=/var/lib/postgresql/e2e \
    -e TZ="${STACK_TZ}" \
    -e PGTZ="${STACK_TZ}" \
    -v "${pgdata}:/var/lib/postgresql/e2e" \
    "${IMG_POSTGRES}" \
    -c port="${PG_PORT}" \
    -c max_connections=200 \
    >/dev/null
}

start_mosquitto() {
  cat >"${STACK_DIR}/mosquitto.conf" <<EOF
# Written by e2e/stack.sh. Mosquitto 2 refuses anonymous connections and binds
# only to loopback unless told otherwise; both are correct defaults and both
# have to be stated here, or css-server's boot fails with a broker error that
# reads like a configuration fault in the application.
listener ${MQTT_PORT} 127.0.0.1
allow_anonymous true
log_dest stdout
EOF
  log "starting mosquitto on ${MQTT_PORT}"
  pm run -d --name "${C_MQTT}" --network host \
    -v "${STACK_DIR}/mosquitto.conf:/mosquitto/config/mosquitto.conf:ro" \
    "${IMG_MOSQUITTO}" \
    >/dev/null
}

# The runtime image: the shipping Dockerfile's runtime stage, minus the parts
# that only matter in production. Built here rather than pulled because there is
# no published image carrying these binaries, and built from a digest-pinned
# base so the only unpinned bytes are the two apt packages.
build_runtime_image() {
  cat >"${STACK_DIR}/Containerfile" <<EOF
FROM ${IMG_RUNTIME}
RUN apt-get update \\
 && apt-get install -y --no-install-recommends libpq5 ca-certificates tzdata \\
 && rm -rf /var/lib/apt/lists/*
EOF
  log "building the runtime image"
  pm build -t "${IMG_SERVER_LOCAL}" -f "${STACK_DIR}/Containerfile" "${STACK_DIR}" \
    >"${OUT}/logs/runtime-image.log" 2>&1
}

# ---------------------------------------------------------------------------
# The stack's configuration
# ---------------------------------------------------------------------------
# Written here in full rather than derived from server/config.toml, because the
# tracked file is somebody's working configuration and a suite that inherits it
# reports different results on different checkouts. Every value the tiers depend
# on -- the profile fields the toolguard and door tiers read, registration being
# open, the initial-setup admin address, the timezone -- is stated where a
# failing assertion can be traced back to it.
write_stack_config() {
  cat >"${STACK_DIR}/config.toml" <<EOF
[site]
site_name = "CSS E2E"
site_url = "http://127.0.0.1:${SERVER_PORT}"
admin_url = "http://127.0.0.1:${SERVER_PORT}/admin"
timezone = "${STACK_TZ}"
max_session_age = 1440
debug = false
secret_key = "e2e-secret-key-not-a-production-value"
https = false

[email]
host = "localhost"
port = 587
username = ""
password = ""
use_tls = false
use_ssl = false
from_email = "noreply@example.invalid"
from_name = "CSS E2E"
enabled = false

[theme]
primary_color = "#007bff"
secondary_color = "#6c757d"
accent_color = "#28a745"
background_color = "#ffffff"
text_color = "#212529"
dark_mode_enabled = true

[stripe]
publishable_key = ""
secret_key = ""
webhook_secret = ""
enabled = false
currency = "USD"
test_mode = true

[reports]
usage_reports_enabled = true
member_reports_enabled = true
financial_reports_enabled = false
generation_frequency_hours = 24
max_reports_retained = 30
default_export_format = "csv"
email_reports = false

[space_directory]
enabled = true
coordinate_format = "decimal"
max_spaces_per_directory = 1000
search_enabled = true
filtering_enabled = true
default_visibility = "public"
allow_anonymous_viewing = true

[sentry]
environment = "e2e"
traces_sample_rate = 0.0
enabled = false
performance_monitoring = false

[database]
url = "postgresql://${PG_USER}:${PG_PASS}@127.0.0.1:${PG_PORT}/${PG_DB}"
host = "127.0.0.1"
port = ${PG_PORT}
database = "${PG_DB}"
username = "${PG_USER}"
password = "${PG_PASS}"
# Above the concurrency tier's fan-out on purpose. A pool smaller than the
# number of concurrent requests serialises them, which makes a race disappear
# and the tier report a pass it did not earn.
max_connections = 64
min_connections = 2
connect_timeout_seconds = 30
idle_timeout_seconds = 600
log_statements = false
auto_migrate = true

[server]
bind_address = "127.0.0.1:${SERVER_PORT}"
log_requests = true
request_timeout_seconds = 30
max_request_body_size = 16777216
cors_enabled = true
cors_origins = ["http://127.0.0.1:${SERVER_PORT}"]

[auth]
jwt_secret = "e2e-jwt-secret-not-a-production-value"
jwt_expiration_hours = 24
allow_registration = true
require_email_verification = false
password_min_length = 8
session_timeout_minutes = 1440
password_reset_enabled = true

[registration_challenge]
enabled = false
hint = ""
phrase = ""
# Off, and the reason is a property of the tiers rather than convenience: the
# fuzz and journey tiers register hundreds of accounts from one address space,
# and a throttle would turn their findings into 429s that look like defects.
# The throttle itself is covered by its own unit tests and by a dedicated case
# in the accounts stage, which turns it on for the length of that case.
throttle_enabled = false
throttle_attempts = 3
throttle_seconds = 180
terms_of_service_checkbox = false
terms_of_service_md = ""
recaptcha_enabled = false
recaptcha_site_key = ""
recaptcha_secret_key = ""

[initial_setup]
setup_enabled = true
setup_admin_email = "admin@e2e.invalid"

[user]
profiles_enabled = true

[[user.profile_fields]]
key = "card_id"
label = "Access Card"
field_type = "Text"
required = false
help_text = "RFID card identifier"

[[user.profile_fields]]
key = "bio"
label = "Bio"
field_type = "Text"
required = false
help_text = "Tell us about yourself"

[[user.profile_fields]]
key = "phone"
label = "Phone Number"
field_type = "Phone"
required = false
help_text = "Contact number"

[tools]

[[tools.tool_categories]]
value = "saw"
label = "Saw"

[[tools.tool_categories]]
value = "powertool"
label = "Power Tools"

[[tools.tool_categories]]
value = "other"
label = "Other"

[toolguard]
enabled = true
profile_field = "card_id"

[calendar]
enabled = false
cache_duration_minutes = 15
max_events_display = 10
lookahead_days = 30

[pages]
# Null on purpose, and asserted null by the schema stage. PagesService::new
# git-clones whatever is here into a hardcoded /tmp path at boot; a stack that
# inherited the tracked config would clone two GitHub repositories on every
# bring-up and fail closed the moment the network did.
wiki_link = "None"
wiki_auto_enabled = false
wiki_period = 600
wiki_readme = false
site_link = "None"
site_auto_enabled = false
site_period = 600
site_embed_index = "INDEX.md"
site_readme = false
users_pages_enabled = false
user_profile_field = "user_page_repository"
user_period = 900
user_readme = false

[edge]
edge_enabled = true

[edge.edge_mqtt_config]
mqtt_instance_url = "tcp://127.0.0.1:${MQTT_PORT}"
mqtt_client_id = "css-e2e-server"
mqtt_namespace = "css-e2e"
EOF
}

start_server() {
  local frontend="${ROOT}/frontend/dist"
  log "starting css-server on ${SERVER_PORT}"
  if [[ ${PROVISION} == "external" ]]; then
    CONFIG_PATH="${STACK_DIR}/config.toml" \
      FRONTEND_PATH="${frontend}" \
      RUST_LOG="${CSS_E2E_RUST_LOG:-info}" \
      TZ="${STACK_TZ}" \
      "${ROOT}/e2e/artifacts/css-server" >"${OUT}/logs/css-server.log" 2>&1 &
    echo $! >"${STACK_DIR}/server.pid"
  else
    pm run -d --name "${C_SERVER}" --network host \
      -e CONFIG_PATH=/stack/config.toml \
      -e FRONTEND_PATH=/frontend \
      -e RUST_LOG="${CSS_E2E_RUST_LOG:-info}" \
      -e TZ="${STACK_TZ}" \
      -v "${ROOT}/e2e/artifacts:/artifacts:ro" \
      -v "${STACK_DIR}:/stack:ro" \
      -v "${frontend}:/frontend:ro" \
      "${IMG_SERVER_LOCAL}" /artifacts/css-server \
      >/dev/null
  fi
}

stop_server() {
  if [[ ${PROVISION} == "external" ]]; then
    if [[ -f "${STACK_DIR}/server.pid" ]]; then
      kill "$(cat "${STACK_DIR}/server.pid")" 2>/dev/null || true
      rm -f "${STACK_DIR}/server.pid"
    fi
  else
    pm rm -f "${C_SERVER}" >/dev/null 2>&1 || true
  fi
}

# Server logs, wherever they went. Appended rather than overwritten so a
# restart does not destroy the boot the previous stage asserted on.
collect_server_log() {
  if [[ ${PROVISION} != "external" ]]; then
    pm logs "${C_SERVER}" >>"${OUT}/logs/css-server.log" 2>&1 || true
  fi
}

collect_stack_logs() {
  collect_server_log
  if [[ ${PROVISION} != "external" ]]; then
    pm logs "${C_PG}" >"${OUT}/logs/postgres.log" 2>&1 || true
    pm logs "${C_MQTT}" >"${OUT}/logs/mosquitto.log" 2>&1 || true
  fi
}

# ---------------------------------------------------------------------------
# Node drivers
# ---------------------------------------------------------------------------
# Stages needing request bodies, JSON and concurrency run a .mjs driver rather
# than more shell. The driver runs in the pinned Node image under
# podman/docker -- with --network host, so 127.0.0.1 still means the stack --
# and on the host under external, where CI has already installed Node for the
# frontend jobs.
#
# The driver's contract, in both environments: it writes a TSV of
# `name<TAB>status<TAB>message` to $CASES_OUT and exits non-zero if any case
# failed. Nothing parses its stdout, so a driver is free to log.
run_node() {
  local script="$1"
  shift
  local cases_out="${STACK_DIR}/driver-cases.tsv"
  : >"${cases_out}"

  if [[ ${PROVISION} == "external" ]]; then
    command -v node >/dev/null 2>&1 || die "--provision=external needs node on PATH for the API stages"
    CASES_OUT="${cases_out}" \
      CSS_BASE_URL="http://127.0.0.1:${SERVER_PORT}" \
      CSS_STACK_DIR="${STACK_DIR}" \
      node "${ROOT}/e2e/drivers/${script}" "$@"
  else
    pm run --rm --network host \
      -e CASES_OUT=/stack/driver-cases.tsv \
      -e CSS_BASE_URL="http://127.0.0.1:${SERVER_PORT}" \
      -e CSS_STACK_DIR=/stack \
      -v "${ROOT}/e2e/drivers:/drivers:ro" \
      -v "${STACK_DIR}:/stack" \
      "${IMG_NODE}" node "/drivers/${script}" "$@"
  fi
}

# Fold a driver's TSV into this stage's cases. Separated from run_node so a
# driver that exits non-zero still has its individual results recorded --
# "the driver failed" is not a useful test report.
absorb_driver_cases() {
  local cases_out="${STACK_DIR}/driver-cases.tsv"
  local n=0
  if [[ ! -s ${cases_out} ]]; then
    record_case "driver/produced-results" fail \
      "the driver wrote no cases; its own output is in the stage log"
    return 1
  fi
  while IFS=$'\t' read -r name status message; do
    [[ -z ${name} ]] && continue
    record_case "${name}" "${status}" "${message}"
    n=$((n + 1))
  done <"${cases_out}"
  log "  absorbed ${n} case(s) from the driver"
  return 0
}
