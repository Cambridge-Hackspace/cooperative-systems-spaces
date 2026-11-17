#!/usr/bin/env bash
#
# macos-verdict.sh <jobs.json> <event-name>
#
# Judges the macOS leg of the release matrix, and prints why.
#
# GitHub-hosted macOS runners are scarce. When none is free, GitHub queues the
# job for fifteen minutes, gives up with "The job was not acquired by Runner of
# type hosted even after multiple attempts", and records it as cancelled with no
# runner and no steps. Nothing in the job ran, so nothing about this project
# failed. That happened twice on 2026-11-16 (runs 43888433011 and 43930212321)
# while the same leg built clean on other runs that day.
#
# So this tells the two apart:
#
#   - the leg ran and every step passed or was skipped  -> pass
#   - the leg was never given a runner, on a pull request -> pass, with a warning
#   - anything else, including a real build failure      -> fail
#
# A missing runner passes ONLY on a pull request. On a push to dev or master,
# or a manual run, the macOS binaries are what that run is for, so a run that
# produced none is reported as failed and wants a re-run.
#
# The "never given a runner" signature is exact: completed, cancelled or
# failed, an empty runner name, and zero steps. A job that started and was
# then cancelled has steps, and fails here. A skipped job also has no runner
# and no steps, and fails here too, because this verdict only runs when the
# release matrix ran, so a skipped macOS leg means something else is wrong.
#
# Steps are judged rather than the job's own conclusion, because the macOS leg
# runs with continue-on-error on pull requests, and that must not be able to
# turn a failed build into a pass here.
#
# <jobs.json> is {"jobs": [...]} as GitHub's "list jobs for a workflow run" API
# returns it, all pages merged. .github/scripts/macos-verdict-test.sh is its
# test, and the CI job runs that test before trusting this.
set -Eeuo pipefail

if [[ $# -ne 2 ]]; then
  echo 'usage: macos-verdict.sh <jobs.json> <event-name>' >&2
  exit 2
fi
jobs_file="$1"
event="$2"

# The matrix legs are named by their matrix values: "release (macos-latest, ...".
mapfile -t matches < <(jq -c '.jobs[] | select(.name | startswith("release (macos-latest,"))' "${jobs_file}")

if [[ ${#matches[@]} -ne 1 ]]; then
  echo "FAIL: expected exactly one macOS release job, found ${#matches[@]}."
  echo 'The matrix may have been renamed; this script matches "release (macos-latest,".'
  exit 1
fi
job="${matches[0]}"

name=$(jq -r '.name' <<<"${job}")
status=$(jq -r '.status' <<<"${job}")
conclusion=$(jq -r '.conclusion // "none"' <<<"${job}")
runner=$(jq -r '.runner_name // ""' <<<"${job}")
nsteps=$(jq -r '.steps | length' <<<"${job}")

if [[ ${status} != completed ]]; then
  echo "FAIL: ${name} is ${status}, not completed."
  exit 1
fi

if [[ ${nsteps} -eq 0 ]]; then
  if [[ -z ${runner} && (${conclusion} == cancelled || ${conclusion} == failure) ]]; then
    if [[ ${event} == pull_request ]]; then
      echo "::warning::The macOS build did not run: GitHub had no macOS runner free for it. Not counted against this pull request. Re-run the job to build it."
      echo "PASS: ${name} was never given a runner (${conclusion}, no steps); allowed on a pull request."
      exit 0
    fi
    echo "FAIL: ${name} was never given a runner (${conclusion}, no steps). On a ${event} run the macOS binaries are required: re-run the job."
    exit 1
  fi
  echo "FAIL: ${name} ran no steps (conclusion ${conclusion}, runner '${runner}'), and that is not the no-runner signature."
  exit 1
fi

bad=$(jq -r '[.steps[] | select(.conclusion != "success" and .conclusion != "skipped") | "\(.name) (\(.conclusion))"] | join("; ")' <<<"${job}")
if [[ -n ${bad} ]]; then
  echo "FAIL: ${name}: ${bad}"
  exit 1
fi

echo "PASS: ${name} built (${nsteps} steps, all passed or skipped)."
