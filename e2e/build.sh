#!/usr/bin/env bash
#
# The `build` verb.
#
# Runs inside the pinned Rust image, which has a Rust toolchain and nothing
# else -- no container engine, no Node, no libpq. Its job is to produce
# everything e2e/run.sh needs, so that the run verb can be pure orchestration.
#
# It also runs the whole non-containerized Rust battery. That is deliberate:
# `reaper test` is sync -> build -> reset -> run, so a failing unit or contract
# test fails the build verb *before* a session spends time bringing a stack up,
# and @pristine is never taken on the strength of a suite that did not pass.
#
# Everything here is idempotent and cache-warm on the second run.
set -Eeuo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${HERE}/.." && pwd)"
cd "${ROOT}"

log() { printf '\n=== %s ===\n' "$*"; }

# ---------------------------------------------------------------------------
# System dependencies
# ---------------------------------------------------------------------------
# The honest cost of using a stock Rust image rather than inventing a builder
# image nobody has built. diesel links libpq; paho-mqtt-sys compiles the bundled
# Paho C library with cmake; the edge crate's transitive GUI dependencies want
# alsa and udev headers. Guarded, so after the first run this is one lookup.
if ! command -v cmake >/dev/null 2>&1 || [ ! -e /usr/include/postgresql/libpq-fe.h ]; then
  log "installing system dependencies"
  apt-get -qq update
  apt-get -qq install -y --no-install-recommends \
    libpq-dev cmake build-essential libasound2-dev libudev-dev
fi

# ---------------------------------------------------------------------------
# Cargo
# ---------------------------------------------------------------------------
# Both live on the session's pool via the declared caches. The guest's boot disk
# has under 4 GiB free and a cold target directory is several times that, so
# letting cargo default to $HOME would run the root filesystem out of space.
export CARGO_HOME="${REAPER_CACHE_CARGO:-${ROOT}/e2e/.cargo}"
export CARGO_TARGET_DIR="${REAPER_CACHE_TARGET:-${ROOT}/target}"
export CARGO_TERM_COLOR=always

log "toolchain"
rustc --version
cargo --version

# default-members is server, cli, edge, css_lib. --all-targets picks up the
# tests/ and benches/ directories that `cargo test --bin <name>` silently
# ignored, which is the reason CI never ran an integration test.
log "cargo build"
cargo build --locked --all-targets

log "cargo test"
cargo test --locked --all-targets

# Doc tests are a separate target and --all-targets does not include them.
log "cargo test --doc"
cargo test --locked --doc

# kiosk and the two toolguard UIs are outside default-members on purpose
# (d78227d) because bevy and egui are heavy. Compile-checking them stops them
# rotting without paying to link them.
log "cargo check (full workspace, incl. the GUI crates)"
cargo check --locked --workspace --all-targets

# ---------------------------------------------------------------------------
# Artifacts for the run verb
# ---------------------------------------------------------------------------
log "release binaries"
cargo build --locked --release \
  --bin css-server --bin css-cli --bin css-webhook-recvr --bin css-edge

# Both edge profiles, on purpose. web_server.rs::create_router has a
# #[cfg(debug_assertions)] arm that honours --frontend-path and a
# #[cfg(not(...))] arm that serves the include_dir! embedding, so a release
# binary ignores that flag entirely. Building only one leaves a cfg branch that
# nothing anywhere executes -- and one of those branches is the subject of a fix
# on this very branch (fdc887c).
cargo build --locked --bin css-edge

mkdir -p e2e/artifacts
install -m 0755 "${CARGO_TARGET_DIR}/release/css-server"        e2e/artifacts/css-server
install -m 0755 "${CARGO_TARGET_DIR}/release/css-cli"           e2e/artifacts/css-cli
install -m 0755 "${CARGO_TARGET_DIR}/release/css-webhook-recvr" e2e/artifacts/css-webhook-recvr
install -m 0755 "${CARGO_TARGET_DIR}/release/css-edge"          e2e/artifacts/css-edge
install -m 0755 "${CARGO_TARGET_DIR}/debug/css-edge"            e2e/artifacts/css-edge-dbg

# run.sh's preflight stage asserts this commit matches the working tree, so a
# run that skipped the build cannot silently test yesterday's binaries.
{
  echo "built:  $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  echo "commit: $(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "dirty:  $(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')"
  echo "rustc:  $(rustc --version)"
} > e2e/artifacts/BUILD.txt

log "build complete"
cat e2e/artifacts/BUILD.txt
