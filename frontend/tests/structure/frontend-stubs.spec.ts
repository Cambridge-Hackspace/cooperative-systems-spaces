import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * UI that is finished enough to render and not finished enough to work.
 *
 * `checks/tests/unimplemented_endpoints.rs` pins the server's 501 handlers on
 * the grounds that "from outside the codebase a registered route is a promise".
 * A rendered control is a stronger promise: a route has to be found, and a
 * button is offered.
 *
 * These are the sites where a comment says the implementation is standing in
 * for something. Each is pinned in its component's tier-2 spec as well; this
 * check is the list, so that adding a sixth is a deliberate act rather than a
 * comment nobody aggregates.
 *
 * A ratchet. The list may shrink -- by finishing one, which fails this test and
 * is the moment to delete its entry -- and may not grow silently.
 *
 * What this does NOT prove: that these are the only stubs. It finds the ones
 * whose authors wrote a comment saying so, which is a floor, not a ceiling.
 */
const FRONTEND_ROOT = process.cwd()

/** Phrases an author uses when standing something in for the real thing. */
const MARKERS = ['In a real implementation', "For now, we'll", 'not fully implemented', '@TODO']

interface Stub {
  file: string
  /** What is standing in, and what for. */
  note: string
}

const KNOWN: Stub[] = [
  {
    file: 'src/components/TrainingProgressModal.vue',
    note:
      'The loader fetches nothing at all; every filter, statistic and row in ' +
      'the 694-line component is unreachable. There is no endpoint for per-step ' +
      'progress across users.',
  },
  {
    file: 'src/components/StartTrainingModal.vue',
    note:
      'The instructor list is the admin roster filtered to staff and admin, ' +
      'standing in for a certified-instructor endpoint. Members get a 403 and an ' +
      'empty list.',
  },
  {
    file: 'src/components/ToolTrainingCard.vue',
    note:
      '`isInstructor` returns staff-or-admin as a proxy for certification. ' +
      'The component is also imported by nothing -- see components-are-reachable.',
  },
  {
    file: 'src/components/PrerequisitesModal.vue',
    note:
      '`getToolName` returns a truncated UUID because no tools lookup is ' +
      'passed in, so the picker labels each option "Tool 3f2a91b0...".',
  },
  {
    file: 'src/views/PlatformView.vue',
    note:
      'The release page claims the server serves its own source, marked ' +
      '@TODO in the rendered prose where a visitor can read it. This is an ' +
      'AGPL offer, so the @TODO is a compliance gap, not a cosmetic one. ' +
      'Moved here from AboutView when the platform page was split out.',
  },
]

function sourceFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(join(FRONTEND_ROOT, dir), { withFileTypes: true })) {
    const rel = `${dir}/${entry.name}`
    if (entry.isDirectory()) sourceFiles(rel, out)
    else if (rel.endsWith('.vue') || rel.endsWith('.ts')) out.push(rel)
  }
  return out
}

function filesWithMarkers(): string[] {
  return sourceFiles('src').filter((f) => {
    const text = readFileSync(join(FRONTEND_ROOT, f), 'utf8')
    return MARKERS.some((m) => text.includes(m))
  })
}

describe('frontend stubs', () => {
  it('scans a plausible number of source files', () => {
    // Anti-vacuity: a walk that returned nothing would make everything below
    // trivially true.
    expect(sourceFiles('src').length).toBeGreaterThan(50)
  })

  it('has no stub that is not recorded here', () => {
    const known = new Set(KNOWN.map((s) => s.file))
    const unlisted = filesWithMarkers().filter((f) => !known.has(f))

    expect(
      unlisted,
      'these files carry a comment saying the implementation is standing in for ' +
        'something. A rendered control is a promise to whoever sees it, so the ' +
        'ones we ship are listed deliberately. Finish it, or add it above with a ' +
        'note saying what is standing in and what for.'
    ).toEqual([])
  })

  it('has no recorded stub that no longer looks like one', () => {
    const found = new Set(filesWithMarkers())
    const finished = KNOWN.filter((s) => !found.has(s.file)).map((s) => s.file)

    expect(
      finished,
      'these are recorded as stubs but no longer carry the marker. If one was ' +
        'finished, delete its entry -- and re-read its tier-2 spec, which pins ' +
        'the stub behavior and will now be asserting something untrue.'
    ).toEqual([])
  })

  it('says what each stub stands in for', () => {
    for (const s of KNOWN) {
      expect(s.note.length, `the note for ${s.file} is too short to be one`).toBeGreaterThan(60)
    }
  })
})
