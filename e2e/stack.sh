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
# Set by write_stack_config; the address a human types to reach this stack.
export STACK_HOST="127.0.0.1"
SERVER_PORT="${CSS_E2E_SERVER_PORT:-4399}"
SMTP_PORT="${CSS_E2E_SMTP_PORT:-2525}"
GROUPSIO_PORT="${CSS_E2E_GROUPSIO_PORT:-4390}"
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

  # POSTGRES_INITDB_ARGS is only honoured when PGDATA is empty: the image runs
  # initdb once and never again. So a PGDATA that already exists pins the
  # cluster's encoding forever, and asking for a different one does nothing.
  #
  # That is not hypothetical here, it is the normal case. `reaper test` is
  # sync -> build -> reset -> run, and reset rolls tank/state back to the
  # @pristine snapshot -- which holds whatever encoding the cluster was first
  # built with. `reaper test --profile utf8` on a session that had already run
  # LATIN1 came up LATIN1, while this function's log line cheerfully announced
  # UTF8, because it printed the request rather than the result.
  #
  # schema/encoding catches the mismatch and did. But a check that fires on
  # every run of a profile that cannot work is not a knob, so: if the existing
  # cluster was built for a different encoding, discard it and let initdb run
  # again. PGDATA is scratch state destroyed with the session, which is what
  # makes this safe.
  #
  # The marker lives beside PGDATA rather than inside it, because initdb
  # refuses a directory that is not empty.
  #
  # In practice it is never found, and the rebuild therefore happens on every
  # run. @pristine is taken once and reaper does not re-take it, so the
  # rollback always restores a PGDATA from before this marker existed. That was
  # measured, not assumed: two consecutive default-profile runs both logged
  # "an unrecorded encoding".
  #
  # Left as it is rather than moved somewhere that survives a rollback. The
  # rebuild costs about a second -- postgres went from ready-after-1s to
  # ready-after-2s -- and the alternative is a marker on a dataset the rollback
  # does not cover, which can then disagree with the cluster it describes. An
  # unconditional rebuild cannot. The consequence worth knowing is that the
  # state rollback no longer does anything for PGDATA specifically; every run
  # gets a genuinely fresh cluster, which is what the snapshot was approximating
  # anyway.
  #
  # schema/encoding is the guarantee regardless: it asks the running server
  # what the encoding actually is, and it is what caught this whole class.
  local marker="${pgdata%/}.encoding"
  local existing=""
  [[ -f ${marker} ]] && existing="$(cat "${marker}" 2>/dev/null || true)"

  if [[ -n $(ls -A "${pgdata}" 2>/dev/null || true) && ${existing} != "${PG_ENCODING}" ]]; then
    log "PGDATA was built as ${existing:-an unrecorded encoding}; rebuilding it for ${PG_ENCODING}"
    # Guarded: this is an rm -rf, and the only path it may ever take is the one
    # stack_pgdata composes.
    case "${pgdata}" in
      */pgdata) rm -rf "${pgdata}" ;;
      *)
        echo "refusing to remove ${pgdata}: not a pgdata directory" >&2
        exit 1
        ;;
    esac
    mkdir -p "${pgdata}"
    [[ ${PROVISION} != "external" ]] && chmod 0777 "${pgdata}"
  fi
  printf '%s' "${PG_ENCODING}" >"${marker}"

  log "starting postgres (requested encoding=${PG_ENCODING}, locale=C, TZ=${STACK_TZ})"
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

  # Under --provision=external there is no engine, so the broker runs as a host
  # process. That path is CI's, and it exists rather than the alternative --
  # switching edge_enabled off there -- because `MqttService::new` connects
  # during boot and `main.rs` propagates the failure. Turning it off in one
  # environment would mean CI never executes the boot path most likely to break,
  # and the first anybody heard of a regression would be a session run or a
  # deployment.
  #
  # A service container cannot do this job: mosquitto 2 binds to loopback inside
  # its own namespace unless told otherwise, and a GitHub service container
  # takes no command override with which to point one at a config file.
  if [[ ${PROVISION} == "external" ]]; then
    command -v mosquitto >/dev/null 2>&1 \
      || die "--provision=external needs mosquitto on PATH (apt-get install mosquitto)"
    mosquitto -c "${STACK_DIR}/mosquitto.conf" >"${OUT}/logs/mosquitto.log" 2>&1 &
    echo $! >"${STACK_DIR}/mosquitto.pid"
    return 0
  fi

  pm run -d --name "${C_MQTT}" --network host \
    -v "${STACK_DIR}/mosquitto.conf:/mosquitto/config/mosquitto.conf:ro" \
    "${IMG_MOSQUITTO}" \
    >/dev/null
}

stop_mosquitto() {
  if [[ -f "${STACK_DIR}/mosquitto.pid" ]]; then
    kill "$(cat "${STACK_DIR}/mosquitto.pid")" 2>/dev/null || true
    rm -f "${STACK_DIR}/mosquitto.pid"
  fi
}

# Somewhere for the mailer to deliver.
#
# A host process in BOTH provisioning modes, unlike mosquitto, and the asymmetry
# is worth a sentence: mosquitto needs a container under podman because it is
# not installed on the host, whereas css-smtp-sink is built from this repository
# into e2e/artifacts alongside css-server. There is nothing to pull and no image
# to pin, which is the whole reason it exists as an in-repo binary rather than
# as a mail-catcher image -- the same reasoning as css-webhook-recvr.
#
# Bound to loopback: it accepts and stores anything sent to it, so it has no
# business being reachable from off the machine.
start_smtp_sink() {
  log "starting css-smtp-sink on ${SMTP_PORT}"
  mkdir -p "${STACK_DIR}/mail"

  # A throwaway certificate for STARTTLS, generated per run.
  #
  # `use_tls = true` on 587 is what a real deployment uses, and it is the branch
  # of MailService::transport most likely to break against a real relay --
  # certificate verification, the upgrade handshake, and lettre's Tls::Required
  # refusing to fall back. Without a certificate here that path would be
  # "covered" by a unit test asserting which enum variant gets picked, which is
  # a different claim entirely.
  #
  # The SAN is the IP because that is what [email] host names and what lettre
  # will verify against; a CN-only certificate is not accepted by OpenSSL 3.
  # Two days of validity because nothing should ever reuse it, and it never
  # leaves this machine.
  openssl req -x509 -newkey rsa:2048 -sha256 -days 2 -nodes \
    -keyout "${STACK_DIR}/smtp-key.pem" \
    -out "${STACK_DIR}/smtp-cert.pem" \
    -subj "/CN=127.0.0.1" \
    -addext "subjectAltName=IP:127.0.0.1,DNS:localhost" \
    >"${OUT}/logs/smtp-cert.log" 2>&1 \
    || die "could not generate the SMTP sink's certificate; see logs/smtp-cert.log"

  SMTP_SINK_BIND="127.0.0.1:${SMTP_PORT}" \
    SMTP_SINK_MAILDIR="${STACK_DIR}/mail" \
    SMTP_SINK_TLS_CERT="${STACK_DIR}/smtp-cert.pem" \
    SMTP_SINK_TLS_KEY="${STACK_DIR}/smtp-key.pem" \
    "${ROOT}/e2e/artifacts/css-smtp-sink" >"${OUT}/logs/smtp-sink.log" 2>&1 &
  echo $! >"${STACK_DIR}/smtp-sink.pid"
}

stop_smtp_sink() {
  if [[ -f "${STACK_DIR}/smtp-sink.pid" ]]; then
    kill "$(cat "${STACK_DIR}/smtp-sink.pid")" 2>/dev/null || true
    rm -f "${STACK_DIR}/smtp-sink.pid"
  fi
}

# The Groups.io destination. A plain HTTP sink, so unlike css-smtp-sink it needs
# no certificate; a host process in both provisioning modes, like the other
# in-repo sinks. State is in-memory and control endpoints let the driver seed and
# read the fake group's roster.
start_groupsio_sink() {
  log "starting css-groupsio-sink on ${GROUPSIO_PORT}"
  GROUPSIO_SINK_BIND="127.0.0.1:${GROUPSIO_PORT}" \
    "${ROOT}/e2e/artifacts/css-groupsio-sink" >"${OUT}/logs/groupsio-sink.log" 2>&1 &
  echo $! >"${STACK_DIR}/groupsio-sink.pid"
}

stop_groupsio_sink() {
  if [[ -f "${STACK_DIR}/groupsio-sink.pid" ]]; then
    kill "$(cat "${STACK_DIR}/groupsio-sink.pid")" 2>/dev/null || true
    rm -f "${STACK_DIR}/groupsio-sink.pid"
  fi
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
  # Where the server listens, and what it calls itself.
  #
  # Both default to 127.0.0.1, so the substitutions below are no-ops and the
  # battery's config is byte-identical to what it has always been. `devlive`
  # sets CSS_E2E_BIND=0.0.0.0 to make the instance reachable off the box; that
  # is a development convenience and never a test configuration, because a
  # loopback-only stack is one fewer thing a test run can collide with.
  #
  # CSS_E2E_HOST is what the server writes into its own URLs (site_url, the
  # door QR template, CORS). Binding 0.0.0.0 does not tell it which address a
  # human will type, so when it is not given we ask the routing table for the
  # address this machine uses to reach the world.
  local bind host
  bind="${CSS_E2E_BIND:-127.0.0.1}"
  host="${CSS_E2E_HOST:-}"
  if [[ -z ${host} ]]; then
    if [[ ${bind} == "0.0.0.0" ]]; then
      host="$(ip -4 route get 1.1.1.1 2>/dev/null \
        | awk '{for (i = 1; i <= NF; i++) if ($i == "src") { print $(i + 1); exit }}')"
      host="${host:-127.0.0.1}"
    else
      host="${bind}"
    fi
  fi
  export STACK_HOST="${host}"

  # Substituted from e2e/stack-config.toml, which is valid TOML on disk and is
  # parsed by server/tests/stack_config_parses.rs before any stack exists.
  sed \
    -e "s|@SERVER_PORT@|${SERVER_PORT}|g" \
    -e "s|@STACK_TZ@|${STACK_TZ}|g" \
    -e "s|@PG_USER@|${PG_USER}|g" \
    -e "s|@PG_PASS@|${PG_PASS}|g" \
    -e "s|@PG_PORT@|${PG_PORT}|g" \
    -e "s|@PG_DB@|${PG_DB}|g" \
    -e "s|@MQTT_PORT@|${MQTT_PORT}|g" \
    -e "s|@SMTP_PORT@|${SMTP_PORT}|g" \
    -e "s|@GROUPSIO_PORT@|${GROUPSIO_PORT}|g" \
    -e "s|^bind_address = \"127\.0\.0\.1:|bind_address = \"${bind}:|" \
    -e "s|http://127\.0\.0\.1:|http://${host}:|g" \
    "${ROOT}/e2e/stack-config.toml" >"${STACK_DIR}/config.toml"

  # A token that survived substitution would reach the server as literal text
  # and produce a parse error naming a line rather than a variable.
  if grep -q "@[A-Z_]*@" "${STACK_DIR}/config.toml"; then
    die "unsubstituted token in the stack config: $(grep -o "@[A-Z_]*@" "${STACK_DIR}/config.toml" | sort -u | tr '\n' " ")"
  fi
}

# The stack directory is mounted read-WRITE, and that is a decision rather than
# an oversight.
#
# It began read-only, and the read-only mount did catch something: it turned
# `AppConfig::from_file`'s silent rewrite-and-exit(0) into a visible error. But
# it also broke `update_profile_config`, which writes `profiles_enabled` back to
# the config file after committing the version row -- so the admin path the
# contract stage exists to assert answered 500 for a reason that was the
# fixture's.
#
# Protecting the file by making it unwritable is protection by accident. The
# restart stage now asserts the file's contents are unchanged after a boot,
# which is the same protection stated deliberately and survives the file being
# writable for the reasons it has to be.
start_server() {
  local frontend="${ROOT}/frontend/dist"
  log "starting css-server on ${SERVER_PORT}"
  if [[ ${PROVISION} == "external" ]]; then
    CONFIG_PATH="${STACK_DIR}/config.toml" \
      FRONTEND_PATH="${frontend}" \
      RUST_LOG="${CSS_E2E_RUST_LOG:-info}" \
      TZ="${STACK_TZ}" \
      SSL_CERT_FILE="${STACK_DIR}/smtp-cert.pem" \
      "${ROOT}/e2e/artifacts/css-server" >"${OUT}/logs/css-server.log" 2>&1 &
    echo $! >"${STACK_DIR}/server.pid"
  else
    pm run -d --name "${C_SERVER}" --network host \
      -e CONFIG_PATH=/stack/config.toml \
      -e FRONTEND_PATH=/frontend \
      -e RUST_LOG="${CSS_E2E_RUST_LOG:-info}" \
      -e TZ="${STACK_TZ}" \
      -e SSL_CERT_FILE=/stack/smtp-cert.pem \
      -v "${ROOT}/e2e/artifacts:/artifacts:ro" \
      -v "${STACK_DIR}:/stack" \
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
# The whole e2e/ tree is mounted, not just e2e/drivers: the drivers read
# ../corpus/hostile.json and ../corpus/endpoints.json, and mounting only the
# scripts made the fuzz stage die on ENOENT before its first assertion.
#
# The driver's contract, in both environments: it writes a TSV of
# `name<TAB>status<TAB>message` to $CASES_OUT and exits non-zero if any case
# failed. Nothing parses its stdout, so a driver is free to log.
#
# Node discovery for the host-run helpers below.
#
# e2e/build.sh bootstraps a checksum-pinned Node into $REAPER_CACHE_NODE (or
# e2e/.node) so a toolchain exists where the environment supplies none. An
# already-present `node` wins, which keeps CI on the one its own setup step
# installed. Note the reaper caveat: that bootstrap runs inside the build
# container, so on a guest its node is not on the host -- see run_reader_node.
# The directory holding a bootstrapped node binary, or empty. No die: a caller
# decides what a missing host node means -- ensure_node treats it as fatal,
# run_reader_node falls back to a container.
bootstrapped_node_dir() {
  local n
  n="$(find "${REAPER_CACHE_NODE:-${ROOT}/e2e/.node}" -maxdepth 3 -type f -name node -perm -u+x 2>/dev/null | head -1)"
  [[ -n ${n} ]] && dirname "${n}"
}

ensure_node() {
  command -v node >/dev/null 2>&1 && return 0
  local dir
  dir="$(bootstrapped_node_dir)"
  [[ -n ${dir} ]] \
    || die "no node on PATH and none bootstrapped by e2e/build.sh"
  PATH="${dir}:${PATH}"
  export PATH
}

# Run the Tier 11 evidence reader over a transcript, printing the rendered
# document to stdout.
#
# Prefers a node on the host -- the reader's point is running with no engine and
# no stack, which is where somebody sits down to read evidence (the FreeBSD
# workstation, a laptop). But `exec = "host"` on a reaper guest has no host
# node: build.sh bootstraps one, yet build runs inside a container
# (exec = "container"), so its node lands in that container's cache and never on
# the guest host -- `find /` on the guest turns up no node at all. Every other
# driver sidesteps this by running node in the pinned image via run_node; the
# reader was the one host-node user, and it is why the evidence stage aborted the
# whole run on a guest even though every tier passed.
#
# So: host node if there is one; otherwise the same image run_node uses, with
# e2e read-only and the stack dir (which holds the transcript) mounted at
# /stack. `--provision=external` means the caller owns the engine and images, so
# there is nothing to fall back to -- return non-zero and let the stage record a
# graceful failure rather than die mid-run.
run_reader_node() { # run_reader_node <transcript-path-under-stack-dir>
  local src="$1"
  if command -v node >/dev/null 2>&1 || [[ -n "$(bootstrapped_node_dir)" ]]; then
    ensure_node
    node "${ROOT}/e2e/evidence/transcript.mjs" "${src}"
  elif [[ ${PROVISION} != "external" ]]; then
    pm run --rm \
      -v "${ROOT}/e2e:/e2e:ro" \
      -v "${STACK_DIR}:/stack" \
      "${IMG_NODE}" node "/e2e/evidence/transcript.mjs" "/stack/$(basename "${src}")"
  else
    return 1
  fi
}

run_node() {
  local script="$1"
  shift
  local cases_out="${STACK_DIR}/driver-cases.tsv"
  : >"${cases_out}"

  if [[ ${PROVISION} == "external" ]]; then
    # e2e/build.sh bootstraps a checksum-pinned Node into $REAPER_CACHE_NODE (or
    # e2e/.node), so the drivers have a toolchain even where the environment
    # supplies none. An already-present `node` wins, which keeps CI on the one
    # its own setup step installed.
    ensure_node
    CASES_OUT="${cases_out}" \
      CSS_BASE_URL="http://127.0.0.1:${SERVER_PORT}" \
      CSS_STACK_DIR="${STACK_DIR}" \
      CSS_DB_ENCODING="${PG_ENCODING}" \
      CSS_GROUPSIO_SINK_URL="http://127.0.0.1:${GROUPSIO_PORT}" \
      node "${ROOT}/e2e/drivers/${script}" "$@"
  else
    # Configuration the drivers read, forwarded explicitly.
    #
    # The host branch above gets these for free: it runs node in this shell, so
    # anything exported into the run reaches it. A container inherits nothing,
    # and every one of these silently took its default instead -- so under
    # podman the fuzzer always ran 400 iterations however it was configured, the
    # concurrency tier always used its default rounds and fanout, and
    # CSS_FUZZ_SEED did nothing at all. SUMMARY.md printed a replay command
    # built around that seed, which could not have worked.
    #
    # Forwarded only when set, rather than as `-e VAR=${VAR:-}`: an empty value
    # is not the same as an absent one. `Number(process.env.CSS_FUZZ_ITERATIONS
    # ?? 400)` reads an empty string as 0, so passing a blank would run the
    # fuzzer zero times and report success.
    local -a passthrough=()
    local v
    for v in CSS_FUZZ_ITERATIONS CSS_FUZZ_SEED CSS_FUZZ_BATCH \
      CSS_RACE_ROUNDS CSS_RACE_FANOUT CSS_RUN_TAG \
      CSS_JOURNEY_ITERATIONS CSS_JOURNEY_SEED CSS_JOURNEY_CHECK_EVERY \
      REAPER_CONTROL; do
      [[ -n ${!v:-} ]] && passthrough+=(-e "${v}=${!v}")
    done

    pm run --rm --network host \
      ${passthrough[@]+"${passthrough[@]}"} \
      -e CASES_OUT=/stack/driver-cases.tsv \
      -e CSS_BASE_URL="http://127.0.0.1:${SERVER_PORT}" \
      -e CSS_STACK_DIR=/stack \
      -e CSS_DB_ENCODING="${PG_ENCODING}" \
      -e CSS_GROUPSIO_SINK_URL="http://127.0.0.1:${GROUPSIO_PORT}" \
      -v "${ROOT}/e2e:/e2e:ro" \
      -v "${STACK_DIR}:/stack" \
      "${IMG_NODE}" node "/e2e/drivers/${script}" "$@"
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

# ---------------------------------------------------------------------------
# css-edge
# ---------------------------------------------------------------------------
# Started the same two ways css-server is, for the same reason: under
# podman/docker in the bookworm runtime image, and as a host process under
# --provision=external.
#
# `start_edge <binary> <frontend-path>` -- the binary is a name under
# e2e/artifacts, so the caller chooses between `css-edge` (release, which
# embeds its bundle) and `css-edge-dbg` (debug, which serves the path).
C_EDGE="css-e2e-edge"

start_edge() {
  local binary="$1" frontend="$2"
  if [[ ${PROVISION} == "external" ]]; then
    RUST_LOG="${CSS_E2E_RUST_LOG:-info}" \
      TZ="${STACK_TZ}" \
      "${ROOT}/e2e/artifacts/${binary}" \
      --config "${STACK_DIR}/edge.config.toml" \
      --frontend-path "${frontend}" \
      >>"${OUT}/logs/css-edge.log" 2>&1 &
    echo $! >"${STACK_DIR}/edge.pid"
    return 0
  fi

  pm run -d --name "${C_EDGE}" --network host \
    -e RUST_LOG="${CSS_E2E_RUST_LOG:-info}" \
    -e TZ="${STACK_TZ}" \
    -v "${ROOT}/e2e/artifacts:/artifacts:ro" \
    -v "${STACK_DIR}:/stack" \
    "${IMG_SERVER_LOCAL}" "/artifacts/${binary}" \
    --config /stack/edge.config.toml \
    --frontend-path "/stack/${frontend##*/}" \
    >/dev/null
}

stop_edge() {
  if [[ ${PROVISION} == "external" ]]; then
    if [[ -f "${STACK_DIR}/edge.pid" ]]; then
      kill "$(cat "${STACK_DIR}/edge.pid")" 2>/dev/null || true
      rm -f "${STACK_DIR}/edge.pid"
    fi
  else
    pm logs "${C_EDGE}" >>"${OUT}/logs/css-edge.log" 2>&1 || true
    pm rm -f "${C_EDGE}" >/dev/null 2>&1 || true
  fi
  # The port has to actually be free before the next binary is started, or the
  # second half of the devices stage asserts against the first half's process.
  local waited=0
  while tcp_open 8080 && [[ ${waited} -lt 15 ]]; do
    sleep 1
    waited=$((waited + 1))
  done
}
