#!/usr/bin/env node
// Tier 11: turn a journey transcript into something a person can read.
//
// Every other tier answers a question with an assertion. This one cannot: "does
// this make sense to a newcomer" has no oracle, because the thing being judged
// is whether English written for a human does its job. So this tier produces
// evidence and asks somebody to look at it -- and the only defensible way to do
// that is to make looking cheap.
//
// Two outputs, and the second is the one that earns the tier.
//
//   1. The run as prose. What happened, in order, in sentences.
//
//   2. Every distinct message the system showed, with how often and to whom.
//      This is where the value is. A suite can prove a route answers 404; only
//      a reader can notice that the 404 said "Requested resource not found"
//      when somebody mistyped a tool name, or that a message naming a database
//      encoding was shown to a member who cannot change one.
//
// Zero dependencies and no stack: it reads a file. That is deliberate -- this
// runs on the FreeBSD workstation, where css-server cannot be built, so the
// evidence a person reads is available on the machine they are sitting at.

import { readFileSync } from 'node:fs'

const path = process.argv[2]
if (!path) {
  console.error('usage: transcript.mjs <journey-transcript.jsonl>')
  process.exit(2)
}

let lines
try {
  lines = readFileSync(path, 'utf8').split('\n').filter((l) => l.trim())
} catch (e) {
  console.error(`cannot read ${path}: ${e.message}`)
  process.exit(2)
}

const steps = []
for (const [i, line] of lines.entries()) {
  try {
    steps.push(JSON.parse(line))
  } catch {
    // A partial last line is expected if the run died mid-write. Named rather
    // than swallowed, because "the transcript is short" and "the run was short"
    // are different facts.
    console.error(`(line ${i + 1} is not valid JSON and was skipped)`)
  }
}

if (steps.length === 0) {
  console.error('the transcript is empty; the journey stage recorded nothing')
  process.exit(1)
}

/** 2xx is agreement, 4xx is refusal, 5xx is the server breaking. */
function outcome(status) {
  if (status === null) return 'got no answer at all'
  if (status < 300) return 'The system agreed.'
  if (status < 500) return 'The system refused.'
  return 'The system broke.'
}

console.log('# What the suite did, as prose\n')
console.log(`${steps.length} recorded step(s), from ${path}\n`)

let lastActor = null
for (const s of steps) {
  // A structural annotation when the actor changes, so a reader can see the
  // shape of the run without counting lines.
  if (s.actor !== lastActor) {
    console.log(`\n[${s.actor}]`)
    lastActor = s.actor
  }
  const said = s.message ? ` It said: "${s.message}"` : ''
  console.log(`  ${String(s.step).padStart(4)}. ${s.actor} ${s.action}. ${outcome(s.status)}${said}`)
}

console.log('\n\n# Every message a person was shown\n')
console.log('Read these as a stranger would. A message is doing its job if it')
console.log('says what happened and what the reader can do about it.\n')

const byMessage = new Map()
for (const s of steps) {
  if (!s.message) continue
  const key = `${s.status} ${s.message}`
  const e = byMessage.get(key) ?? { status: s.status, message: s.message, count: 0, actors: new Set() }
  e.count += 1
  e.actors.add(s.actor)
  byMessage.set(key, e)
}

if (byMessage.size === 0) {
  console.log('  (none: nothing in this run was refused)')
} else {
  const sorted = [...byMessage.values()].sort((a, b) => b.count - a.count)
  for (const e of sorted) {
    console.log(`  ${e.status}  x${e.count}  "${e.message}"`)
    console.log(`        shown to: ${[...e.actors].join(', ')}`)
  }
}

// Server errors are listed, and this tier deliberately does NOT fail on them.
//
// The first version of this exited non-zero on any 5xx, which felt rigorous and
// was wrong: `fuzz`, `logs` and `audit` already assert no-5xx, and a fourth
// assertion means a fourth place to exempt the same known finding when a
// deliberate one exists. Exemptions that multiply stop being read.
//
// They are listed because a person reading the transcript should see them in
// context -- what was being attempted, and what the user was told -- which is
// the thing the other three tiers cannot show.
const broke = steps.filter((s) => s.status !== null && s.status >= 500)
console.log('\n\n# Server errors in this run\n')
if (broke.length === 0) {
  console.log('  (none)')
} else {
  for (const s of broke) {
    console.log(`  step ${s.step}: ${s.actor} ${s.action} -> ${s.status} ${s.message ?? ''}`)
  }
  console.log(
    `\n  ${broke.length} step(s) ended in a server error. Whether that is a ` +
    'finding is asserted by the fuzz, logs and audit tiers; this tier shows you ' +
    'what the person was doing and what they were told.'
  )
}
process.exit(0)
