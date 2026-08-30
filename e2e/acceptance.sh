#!/usr/bin/env bash
#
# The acceptance test for the whole exercise.
#
# From §15 of the methodology: take defects you have already found and fixed by
# hand, revert the fixes, and confirm the harness rediscovers them. A
# methodology that cannot rediscover your known bugs is not yet measuring
# anything.
#
# The corpus is the four fixes at the head of feature/tests. Each is reverted
# *surgically* -- the behavioral change only, not the whole commit -- because
# two of them also added a migration, a route and a component restructure, and
# reverting those wholesale produces a tree that fails for reasons that have
# nothing to do with the defect.
#
#   e2e/acceptance.sh break     apply the four reverts
#   e2e/acceptance.sh restore   put everything back
#   e2e/acceptance.sh check     report which files are currently broken
#
# Then run the suite in between:
#
#   e2e/acceptance.sh break && reaper test ; e2e/acceptance.sh restore
#
# ---------------------------------------------------------------------------
# A note on two of the four
# ---------------------------------------------------------------------------
# 5c2fa3c and 11c4f42 are both guard changes on /api/profiles/config, and the
# contract tier's route table states what each guard should be. Reverting the
# guard alone is therefore caught by `checks/tests/route_table_matches.rs`
# during the build verb -- correctly, and *before* a stack is ever brought up.
#
# That is a real answer, and it is not the one being tested here. So these two
# reverts update the route table to match, which is what a regression looks like
# when somebody changes a guard deliberately and keeps the table in step. Only a
# tier that talks to a running server can catch that, which is exactly the claim
# the contract stage exists to support.
set -Eeuo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${HERE}/.." && pwd)"
cd "${ROOT}"

MARK=".acceptance-broken"

say() { printf '%s\n' "$*"; }

require_clean() {
  if [[ -n "$(git status --porcelain)" ]]; then
    say "The working tree has changes. Commit or stash them first: this script"
    say "edits tracked files and restores them with git checkout."
    exit 1
  fi
}

# ---------------------------------------------------------------------------
# 92afb4c -- silent failures in door check-in
# ---------------------------------------------------------------------------
# The fix is the `|| 'Failed to load door'` fallback. Only a transport-level
# rejection reaches it: axios attaches a `response` to an HTTP error, so a suite
# injecting 500s never executes the branch and the alert renders empty.
break_92afb4c() {
  perl -0pi -e "s{\Qe?.response?.data?.error || 'Failed to load door'\E}{e?.response?.data?.error}" \
    frontend/src/views/DoorCheckinView.vue
  perl -0pi -e "s{\Qe?.response?.data?.error || 'Check-in failed'\E}{e?.response?.data?.error}" \
    frontend/src/views/DoorCheckinView.vue
}

# ---------------------------------------------------------------------------
# 5c2fa3c -- the profile page for non-admins
# ---------------------------------------------------------------------------
# GET /api/profiles/config required AdminUser, so every non-admin's profile page
# got a 403 on load and fell back to treating profiles as unconfigured.
break_5c2fa3c() {
  perl -0pi -e "s{async fn get_profile_config\(\n    _auth_user: AuthUser,}{async fn get_profile_config(\n    _admin_user: AdminUser,}" \
    server/src/api/profiles.rs
  perl -pi -e 's{R\("GET", "/api/profiles/config", Guard::Auth\)}{R("GET", "/api/profiles/config", Guard::Admin)}' \
    server/tests/common/mod.rs
}

# ---------------------------------------------------------------------------
# fdc887c -- --frontend-path on the edge debug web server
# ---------------------------------------------------------------------------
# The flag was parsed and never passed on, so the debug router always served a
# hardcoded ./frontend_edge/dist. Only running the debug binary can see it: the
# release build has a different `create_router` and ignores the flag by design.
break_fdc887c() {
  perl -0pi -e "s{let serve_dir = ServeDir::new\(&state\.frontend_path\)\.fallback\(ServeFile::new\(format!\(\n        \"\{\}/index\.html\",\n        state\.frontend_path\n    \)\)\);}{let serve_dir = ServeDir::new(\"./frontend_edge/dist\")\n        .fallback(ServeFile::new(\"./frontend_edge/dist/index.html\"));}" \
    edge/src/web_server.rs
}

# ---------------------------------------------------------------------------
# 11c4f42 -- profile configuration restricted to admins
# ---------------------------------------------------------------------------
# The mutating PUT took any authenticated user, so any member could rewrite the
# profile field schema for the whole space.
break_11c4f42() {
  perl -0pi -e "s{async fn update_profile_config\(\n    admin_user: AdminUser,}{async fn update_profile_config(\n    admin_user: AuthUser,}" \
    server/src/api/profiles.rs
  perl -pi -e 's{R\("PUT", "/api/profiles/config", Guard::Admin\)}{R("PUT", "/api/profiles/config", Guard::Auth)}' \
    server/tests/common/mod.rs
}

# ---------------------------------------------------------------------------
# Driver
# ---------------------------------------------------------------------------
FILES=(
  frontend/src/views/DoorCheckinView.vue
  server/src/api/profiles.rs
  server/tests/common/mod.rs
  edge/src/web_server.rs
)

case "${1:-}" in
  break)
    require_clean
    say "Reverting four fixes. The suite is now EXPECTED to fail."
    say ""
    for fix in 92afb4c 5c2fa3c fdc887c 11c4f42; do
      "break_${fix}"
      say "  reverted ${fix}"
    done

    # Every revert has to have actually changed something. A perl substitution
    # that matched nothing leaves the tree correct and the run green, and the
    # acceptance test then reports a pass for work it did not do -- which is the
    # single failure this whole document is about.
    changed="$(git status --porcelain -- "${FILES[@]}" | wc -l | tr -d ' ')"
    if [[ ${changed} -ne ${#FILES[@]} ]]; then
      say ""
      say "FATAL: expected ${#FILES[@]} modified files, git reports ${changed}:"
      git status --porcelain -- "${FILES[@]}"
      say ""
      say "One of the substitutions matched nothing, so the tree is not broken"
      say "the way this script claims. Restoring."
      git checkout -- "${FILES[@]}"
      exit 1
    fi

    date -u '+%Y-%m-%dT%H:%M:%SZ' >"${MARK}"
    say ""
    say "Four files modified. Now run the suite -- for example:"
    say ""
    say "    reaper test"
    say ""
    say "and then: e2e/acceptance.sh restore"
    ;;

  restore)
    git checkout -- "${FILES[@]}"
    rm -f "${MARK}"
    say "Restored. git status should be clean:"
    git status --porcelain
    ;;

  check)
    if [[ -f ${MARK} ]]; then
      say "BROKEN since $(cat "${MARK}") -- these files carry reverted fixes:"
      git status --porcelain -- "${FILES[@]}"
    else
      say "Not broken."
    fi
    ;;

  *)
    say "usage: e2e/acceptance.sh break|restore|check"
    exit 1
    ;;
esac
