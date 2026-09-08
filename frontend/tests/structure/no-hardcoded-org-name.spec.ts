import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * The organization's display name is configurable (`site.site_name`, surfaced
 * through the config store's `siteName()` getter). No view may hardcode it: an
 * operator who renames their space in config must see the new name everywhere,
 * not a literal baked into the bundle.
 *
 * This guards the de-hardcoding sweep (the marketing/legal views and App.vue).
 * It scans the whole app source for the exact two-word proper noun and asserts
 * it appears nowhere.
 *
 * Deliberately NOT flagged (these are not display labels):
 *   - the GitHub org / container-registry owner `Cambridge-Hackspace` (hyphen),
 *   - external URLs / emails like `cambridgehackspace.com`,
 *   - the lowercase common noun "hackspace".
 * The needle is the spaced proper noun only, so none of those match.
 *
 * `process.cwd()` is the frontend directory under vitest, as in the sibling
 * audit-event-types.spec.ts.
 */
const FRONTEND_ROOT = process.cwd()
const SRC = join(FRONTEND_ROOT, 'src')
const NEEDLE = 'Cambridge Hackspace'
const EXTENSIONS = ['.vue', '.ts', '.js', '.html']

function sourceFiles(dir: string): string[] {
  const out: string[] = []
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name)
    if (entry.isDirectory()) {
      out.push(...sourceFiles(full))
    } else if (EXTENSIONS.some((e) => entry.name.endsWith(e))) {
      out.push(full)
    }
  }
  return out
}

describe('no view hardcodes the organization name', () => {
  it('the exact display name appears nowhere under src/', () => {
    const files = sourceFiles(SRC)

    // Anti-vacuity: a walk that found nothing would pass while checking nothing.
    expect(files.length, 'no source files scanned; the walk is broken').toBeGreaterThan(50)

    const offenders = files.filter((f) => readFileSync(f, 'utf8').includes(NEEDLE))
    expect(
      offenders.map((f) => f.slice(FRONTEND_ROOT.length + 1)),
      `these files hardcode "${NEEDLE}" instead of using the config store's ` +
        'siteName() getter. Bind the configurable org name instead.'
    ).toEqual([])
  })

  it('the detector actually detects the needle (self-test of the oracle)', () => {
    // Feed the check the very string a regression would introduce, and confirm
    // the substring test the scan relies on would catch it. A guard never seen
    // to fire is indistinguishable from one that cannot.
    const brokenWorld = `<template><h2>Welcome to ${NEEDLE}</h2></template>`
    expect(brokenWorld.includes(NEEDLE)).toBe(true)
  })
})
