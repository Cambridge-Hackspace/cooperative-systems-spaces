#!/usr/bin/env bash
# Self-test for stack.sh's css-smtp-sink start and stop (#187).
#
# The defect was a sink from an earlier run holding the port with that run's
# certificate, unnoticed because "the port answers" was taken as "our sink is
# up". This drives the three shapes against a fake sink: a stale css-smtp-sink
# is stopped and replaced; a listener that is NOT a css-smtp-sink is refused;
# a clean start is a clean start. Then `stop_smtp_sink` with no pid file.
#
# The fake is a python script NAMED css-smtp-sink, so its command line names
# it the way the real binary's does -- which is what `listener_on_port` reports
# and what the stale-sink rule keys on. Runs anywhere bash, python3 and openssl
# are.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=e2e/lib.sh
source "${HERE}/../lib.sh"
# shellcheck source=e2e/stack.sh
source "${HERE}/../stack.sh"

TMP=$(mktemp -d "${TMPDIR:-/tmp}/smtp-sink-selftest.XXXXXX")
cleanup() {
  pkill -f "${TMP}" 2>/dev/null || true
  rm -rf "${TMP}"
}
trap cleanup EXIT

ROOT="${TMP}/root"
OUT="${TMP}/out"
STACK_DIR="${TMP}/stack"
SMTP_PORT="${CSS_E2E_SMTP_SELFTEST_PORT:-25925}"
mkdir -p "${ROOT}/e2e/artifacts" "${OUT}/logs" "${STACK_DIR}"

# The fake sink: binds what SMTP_SINK_BIND says and sits there.
cat >"${ROOT}/e2e/artifacts/css-smtp-sink" <<'PY'
#!/usr/bin/env python3
import os, socket, time
host, port = os.environ.get("SMTP_SINK_BIND", "127.0.0.1:25925").split(":")
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind((host, int(port)))
s.listen(1)
# The self-test's crash switch: a sink that dies the way the real one did in
# #187, with the port still answering elsewhere or not at all.
if os.path.exists(os.environ.get("SMTP_SINK_MAILDIR", "") + "/crash"):
    s.close()
    raise SystemExit('Error: Os { code: 98, kind: AddrInUse, message: "Address already in use" }')
while True:
    time.sleep(1)
PY
chmod +x "${ROOT}/e2e/artifacts/css-smtp-sink"
# A listener that is not a sink: the same thing under another name.
cp "${ROOT}/e2e/artifacts/css-smtp-sink" "${TMP}/some-other-daemon"

fails=0
ok() { printf '  ok    %s\n' "$1"; }
bad() {
  printf '  FAIL  %s\n' "$1"
  fails=$((fails + 1))
}
pid_of_sink_file() { cat "${STACK_DIR}/smtp-sink.pid" 2>/dev/null || echo ""; }
alive() { kill -0 "$1" 2>/dev/null; }

[[ -z $(listener_on_port "${SMTP_PORT}" || true) ]] || die "port ${SMTP_PORT} is busy; set CSS_E2E_SMTP_SELFTEST_PORT"

# ── 1. a clean start ────────────────────────────────────────────────────────
if start_smtp_sink >/dev/null 2>"${TMP}/err1"; then ok "clean start succeeds"; else bad "clean start failed: $(cat "${TMP}/err1")"; fi
first=$(pid_of_sink_file)
if [[ -n ${first} ]] && alive "${first}"; then ok "the pid file names a live sink"; else bad "no live sink behind the pid file"; fi
owner=$(listener_on_port "${SMTP_PORT}" || true)
if [[ ${owner%% *} == "${first}" ]]; then ok "the port is held by the sink we started"; else bad "port owner is '${owner}', expected pid ${first}"; fi
case "${owner#* }" in
  *css-smtp-sink*) ok "its command line names css-smtp-sink" ;;
  *) bad "command line is '${owner#* }'" ;;
esac

# ── 2. the pid file is lost; the sink is stale ──────────────────────────────
rm -f "${STACK_DIR}/smtp-sink.pid"
if start_smtp_sink >/dev/null 2>"${TMP}/err2"; then ok "a start over a stale sink succeeds"; else bad "start over a stale sink failed: $(cat "${TMP}/err2")"; fi
if grep -q "earlier run" "${TMP}/err2"; then ok "it said the sink was from an earlier run"; else bad "no explanation of what was stopped: $(cat "${TMP}/err2")"; fi
second=$(pid_of_sink_file)
if ! alive "${first}"; then ok "the stale sink was stopped"; else bad "the stale sink (pid ${first}) is still running"; fi
if [[ -n ${second} && ${second} != "${first}" ]] && alive "${second}"; then ok "a new sink took the port"; else bad "no new sink (pid file: '${second}')"; fi
owner=$(listener_on_port "${SMTP_PORT}" || true)
if [[ ${owner%% *} == "${second}" ]]; then ok "the port is held by the new sink"; else bad "port owner is '${owner}', expected ${second}"; fi

# ── 3. stop by port, with no pid file ───────────────────────────────────────
rm -f "${STACK_DIR}/smtp-sink.pid"
stop_smtp_sink >/dev/null 2>&1 || true
if ! alive "${second}"; then ok "stop_smtp_sink found the sink by port"; else bad "stop_smtp_sink left pid ${second} running without a pid file"; fi
if port_closed "${SMTP_PORT}"; then ok "the port is free afterwards"; else bad "the port is still held"; fi

# ── 4. something that is not a sink holds the port: refused, left alone ────
SMTP_SINK_BIND="127.0.0.1:${SMTP_PORT}" "${TMP}/some-other-daemon" >/dev/null 2>&1 &
other=$!
wait_for "the foreign listener" 10 tcp_open "${SMTP_PORT}" >/dev/null 2>&1 || die "the foreign listener never bound"
if start_smtp_sink >/dev/null 2>"${TMP}/err4"; then bad "start succeeded over a foreign listener"; else ok "start refuses over a foreign listener"; fi
if grep -q "not css-smtp-sink" "${TMP}/err4"; then ok "it named what it would not touch"; else bad "the refusal did not say why: $(cat "${TMP}/err4")"; fi
if alive "${other}"; then ok "the foreign listener was left alone"; else bad "the foreign listener was killed"; fi
kill "${other}" 2>/dev/null || true

# ── 5. the sink dies on start: said, with its own last words ────────────────
# What this does NOT prove: the race in which our sink dies while something
# else answers the port in the same second. start_smtp_sink checks that the
# process it started is still alive once the port answers, and that check is
# not reachable from here without staging the race; it is defence in depth
# behind the refusal in arm 4, not a tested promise.
touch "${STACK_DIR}/mail/crash"
if start_smtp_sink >/dev/null 2>"${TMP}/err5"; then bad "start succeeded though the sink died"; else ok "a sink that dies on start fails the start"; fi
if grep -q "AddrInUse" "${TMP}/err5"; then ok "the failure quotes the sink's own last line"; else bad "the sink's log was not quoted: $(cat "${TMP}/err5")"; fi
rm -f "${STACK_DIR}/mail/crash"

printf '%s\n' "smtp-sink self-test: $((fails)) failure(s)"
[[ ${fails} -eq 0 ]]
