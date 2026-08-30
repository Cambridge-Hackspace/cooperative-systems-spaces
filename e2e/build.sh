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
#
# One of these is for e2e/lint.sh rather than for the build: the shell linter.
# It is installed here because a gate that only exists in CI is a gate you
# discover by pushing, then fixing, then pushing again.
#
# The shell *formatter* is deliberately not from apt -- see the pinned bootstrap
# below.
if ! command -v cmake >/dev/null 2>&1 || [ ! -e /usr/include/postgresql/libpq-fe.h ] \
  || ! command -v shellcheck >/dev/null 2>&1; then
  log "installing system dependencies"
  apt-get -qq update
  apt-get -qq install -y --no-install-recommends \
    libpq-dev cmake build-essential libasound2-dev libudev-dev shellcheck
fi

# ---------------------------------------------------------------------------
# shfmt, pinned
# ---------------------------------------------------------------------------
# Version-pinned and checksum-verified, exactly like Node below, and for a
# reason that cost a build to find: Debian bookworm ships shfmt 3.6.0, whose
# `-s` rewrites `${VAR:-}` to `${VAR-}`. Those are not the same expression --
# `:-` substitutes for unset *or empty*, `-` only for unset -- and later
# versions stopped doing it. Running whatever the distribution happens to
# package meant the formatter disagreed with itself across machines and
# demanded a rewrite that changes shell semantics.
#
# A formatter is only "total and non-negotiable" if every machine runs the same
# one. This is that.
SHFMT_VERSION="v3.13.1"
SHFMT_SHA256="fb096c5d1ac6beabbdbaa2874d025badb03ee07929f0c9ff67563ce8c75398b1"

SHFMT_ROOT="${REAPER_CACHE_NODE:-${ROOT}/e2e/.node}/shfmt-${SHFMT_VERSION}"
if [ ! -x "${SHFMT_ROOT}/shfmt" ]; then
  log "bootstrapping shfmt ${SHFMT_VERSION}"
  mkdir -p "${SHFMT_ROOT}"
  curl -fsSL -o "${SHFMT_ROOT}/shfmt.download" \
    "https://github.com/mvdan/sh/releases/download/${SHFMT_VERSION}/shfmt_${SHFMT_VERSION}_linux_amd64"
  if ! printf '%s  %s\n' "${SHFMT_SHA256}" "shfmt.download" \
    | (cd "${SHFMT_ROOT}" && sha256sum -c -); then
    rm -f "${SHFMT_ROOT}/shfmt.download"
    echo "shfmt checksum mismatch; refusing to install it" >&2
    exit 1
  fi
  chmod +x "${SHFMT_ROOT}/shfmt.download"
  mv "${SHFMT_ROOT}/shfmt.download" "${SHFMT_ROOT}/shfmt"
fi
export PATH="${SHFMT_ROOT}:${PATH}"

# curl and xz are used by the Node bootstrap below. Both are in the Rust image;
# checked rather than assumed, because the failure otherwise is a 127 in the
# middle of a download and reads like a network fault.
for tool in curl tar sha256sum xz; do
  command -v "${tool}" >/dev/null 2>&1 || {
    echo "${tool} is not on PATH in the build image" >&2
    exit 1
  }
done

# ---------------------------------------------------------------------------
# Node
# ---------------------------------------------------------------------------
# The build verb runs in `rust:1.97-bookworm`, which has no Node, and there is
# no canonical Rust+Node image. Inventing one nobody has built would be worse
# than this: the tenant contract names checksum-pinning as the sanctioned third
# option next to a digest and a tag, and that is what this is. The tarball is
# fetched once per session into $REAPER_CACHE_NODE and verified before it is
# unpacked -- an unverified download in a build that then compiles a binary and
# ships it is the supply-chain shape this whole exercise is meant to avoid.
NODE_VERSION="v24.20.0"
NODE_SHA256="2f2c0da162318f0de47665410c7c8c2ed3d36c8f3105de4bbc61176c70a7cbf2"
NODE_TARBALL="node-${NODE_VERSION}-linux-x64.tar.xz"

NODE_ROOT="${REAPER_CACHE_NODE:-${ROOT}/e2e/.node}"
NODE_HOME="${NODE_ROOT}/node-${NODE_VERSION}-linux-x64"

if [ ! -x "${NODE_HOME}/bin/node" ]; then
  log "bootstrapping Node ${NODE_VERSION}"
  mkdir -p "${NODE_ROOT}"
  curl -fsSL -o "${NODE_ROOT}/${NODE_TARBALL}" \
    "https://nodejs.org/dist/${NODE_VERSION}/${NODE_TARBALL}"
  # The verification is the point of pinning, so it is a hard failure and the
  # downloaded file is removed -- a corrupt tarball left in a session cache
  # would fail identically on every later run with no clue why.
  if ! printf '%s  %s\n' "${NODE_SHA256}" "${NODE_TARBALL}" \
    | (cd "${NODE_ROOT}" && sha256sum -c -); then
    rm -f "${NODE_ROOT}/${NODE_TARBALL}"
    echo "node tarball checksum mismatch; refusing to unpack it" >&2
    exit 1
  fi
  tar -xJf "${NODE_ROOT}/${NODE_TARBALL}" -C "${NODE_ROOT}"
  rm -f "${NODE_ROOT}/${NODE_TARBALL}"
fi

export PATH="${NODE_HOME}/bin:${PATH}"
export npm_config_cache="${REAPER_CACHE_NPM:-${ROOT}/e2e/.npm}"

log "node toolchain"
node --version
npm --version

# ---------------------------------------------------------------------------
# The two frontends
# ---------------------------------------------------------------------------
# frontend_edge FIRST, and not as a preference: `edge/src/lib.rs` embeds
# frontend_edge/dist with include_dir!, which is evaluated when the crate is
# compiled. Building it after cargo would produce a css-edge carrying
# build.rs's placeholder -- a binary that compiles cleanly and serves a "UI not
# built" page as its own interface, which is exactly the failure the CI
# `mkdir -p /builds/...` line was papering over.
# The same gate CI runs on this directory, in the same order, before the build.
#
# It is here and not only in CI because the alternative is the loop this whole
# tool exists to avoid: push, watch a lint job fail twenty minutes later, commit
# a fix, push again. That happened twice while the tier-2 specs were being
# written, and both fixups are in the history. reaper is the pre-push loop, so
# anything CI can reject has to be rejectable here first.
#
# `test:coverage` rather than `test`, matching CI: the coverage provider
# instruments differently and has failed on its own before.
frontend_gate() { # frontend_gate <dir>
  local dir="$1"
  log "gating ${dir}"
  (
    cd "${dir}"
    npm run format:check
    npm run lint
    npm run type-check
    npm run type-check:strict
    # frontend_edge has no test suite yet. Asserted rather than assumed, so the
    # day it gets one this stops silently skipping it.
    if node -e 'process.exit(require("./package.json").scripts["test:coverage"] ? 0 : 1)'; then
      npm run test:coverage
    else
      echo "  ${dir}: no test:coverage script -- nothing to run"
    fi
  )
}

build_frontend() { # build_frontend <dir>
  local dir="$1"
  log "building ${dir}"
  (
    cd "${dir}"
    if [ -f package-lock.json ]; then
      npm ci --no-audit --no-fund
    else
      npm install --no-audit --no-fund
    fi
  )
  # Gate before build, so a lint failure costs seconds rather than a bundle.
  frontend_gate "${dir}"
  (
    cd "${dir}"
    npm run build
  )
  # An empty bundle compiles and serves 404 for the whole UI, so "the command
  # exited zero" is not the assertion worth making here.
  if [ ! -s "${dir}/dist/index.html" ]; then
    echo "${dir}/dist/index.html is missing or empty after a successful build" >&2
    exit 1
  fi
  echo "  ${dir}/dist/index.html: $(wc -c <"${dir}/dist/index.html") bytes"
}

build_frontend frontend_edge
build_frontend frontend

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
# ---------------------------------------------------------------------------
# The shell gate
# ---------------------------------------------------------------------------
# CI runs this as its own job. It costs under a second and it is the only thing
# checking the scripts this file and run.sh are made of, so running it here as
# well means a shellcheck failure is caught before a session is spent.
log "e2e/lint.sh"
./e2e/lint.sh

# `cargo fmt --check` is a CI job on its own, and it is the cheapest thing in
# this file. Running it before the build means a formatting failure costs a
# second rather than a full compile.
log "cargo fmt --check"
cargo fmt --all -- --check

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
install -m 0755 "${CARGO_TARGET_DIR}/release/css-server" e2e/artifacts/css-server
install -m 0755 "${CARGO_TARGET_DIR}/release/css-cli" e2e/artifacts/css-cli
install -m 0755 "${CARGO_TARGET_DIR}/release/css-webhook-recvr" e2e/artifacts/css-webhook-recvr
install -m 0755 "${CARGO_TARGET_DIR}/release/css-edge" e2e/artifacts/css-edge
install -m 0755 "${CARGO_TARGET_DIR}/debug/css-edge" e2e/artifacts/css-edge-dbg

# run.sh's preflight stage asserts this commit matches the working tree, so a
# run that skipped the build cannot silently test yesterday's binaries.
{
  echo "built:  $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  echo "commit: $(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "dirty:  $(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')"
  echo "rustc:  $(rustc --version)"
} >e2e/artifacts/BUILD.txt

log "build complete"
cat e2e/artifacts/BUILD.txt
