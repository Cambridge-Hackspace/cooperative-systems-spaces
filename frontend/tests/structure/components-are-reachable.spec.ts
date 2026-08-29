import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * Components that nothing imports.
 *
 * A `.vue` file under `src/components` that no other file references cannot
 * render. It still type-checks, still lints, still gets a tier-2 spec written
 * for it, and still looks from the outside like part of the product — which is
 * exactly how two of them came to carry defects nobody had reported:
 *
 *   - `AssignTrainerModal` never emits `assigned` on success, so the parent is
 *     never told an assignment happened. There is no parent.
 *   - `ToolTrainingCard` opens its template with a red `background: red` debug
 *     banner reading "ToolTrainingCard is rendering!". Nobody has seen it.
 *
 * Both defects are real and both specs pin them, because the day either
 * component is wired up the defects arrive with it. What the specs must not
 * claim is that users are hitting them today, and this check is what keeps the
 * two facts attached to each other.
 *
 * A ratchet: the list may shrink, by wiring a component up or deleting it. It
 * may not grow silently.
 *
 * What this does NOT prove: that a referenced component is ever actually
 * rendered — a reference inside a `v-if` that is always false would still
 * count. It is a lower bound on deadness, not an upper one.
 */
const FRONTEND_ROOT = process.cwd()

/** Components known to be referenced by nothing, with why they still exist. */
const UNREFERENCED: Record<string, string> = {
  AssignTrainerModal:
    'Superseded by the assign form inlined in TrainerManagement.vue, which is ' +
    'the one that works. This copy never emits `assigned`.',
  ToolTrainingCard:
    'Superseded by ToolTrainingModal, which renders the same training flow. ' +
    'This copy still carries a red debug banner in its template.',
}

function componentNames(): string[] {
  return readdirSync(join(FRONTEND_ROOT, 'src/components'))
    .filter((f) => f.endsWith('.vue'))
    .map((f) => f.replace(/\.vue$/, ''))
}

function sourceFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(join(FRONTEND_ROOT, dir), { withFileTypes: true })) {
    const rel = `${dir}/${entry.name}`
    if (entry.isDirectory()) sourceFiles(rel, out)
    else if (rel.endsWith('.vue') || rel.endsWith('.ts')) out.push(rel)
  }
  return out
}

function referencedBy(name: string): string[] {
  return sourceFiles('src').filter(
    (f) =>
      !f.endsWith(`/components/${name}.vue`) &&
      new RegExp(`\\b${name}\\b`).test(readFileSync(join(FRONTEND_ROOT, f), 'utf8'))
  )
}

describe('every component is reachable from something', () => {
  it('scans a plausible number of components', () => {
    // Anti-vacuity: a directory read that returned nothing would make the
    // assertions below trivially true.
    expect(componentNames().length).toBeGreaterThan(30)
  })

  it('has no unreferenced component that is not recorded here', () => {
    const orphans = componentNames().filter((n) => referencedBy(n).length === 0)
    const unlisted = orphans.filter((n) => !(n in UNREFERENCED))

    expect(
      unlisted,
      'these components are imported by nothing, so they cannot render. A ' +
        'component nobody mounts still type-checks and still looks like part of ' +
        'the product, which is how the two already listed here came to carry ' +
        'defects nobody had reported. Wire it up, delete it, or record it above ' +
        'with the reason it stays.'
    ).toEqual([])
  })

  it('has no recorded orphan that is actually referenced', () => {
    const stale = Object.keys(UNREFERENCED).filter((n) => referencedBy(n).length > 0)

    expect(
      stale,
      'these are recorded as unreferenced but something imports them now. If ' +
        'they were wired up, remove them from UNREFERENCED -- and re-read their ' +
        'tier-2 specs, which say the defects they pin are not reachable by users.'
    ).toEqual([])
  })

  it('gives every recorded orphan a reason', () => {
    for (const [name, reason] of Object.entries(UNREFERENCED)) {
      expect(reason.length, `the reason for ${name} is too short to be one`).toBeGreaterThan(40)
    }
  })
})
