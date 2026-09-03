/**
 * Tier 3: every cached facility tab has to reload when it is shown again.
 *
 * `FacilityManagement.vue` wraps its `v-if` tab chain in `<KeepAlive>`. That is
 * the right call and should stay -- it keeps a half-filled form, a selection
 * and a scroll position across a tab switch. The cost is that `onMounted` fires
 * exactly once per page load, so a tab reading data another tab owns is stale
 * from the moment somebody edits it, and only a manual page refresh fixes it.
 *
 * That is issue #11: add rooms on Places, open "+ New door", and be told there
 * are no places to pick from. The reporter's workaround was pressing refresh,
 * which is the tell -- refreshing is the only thing that remounts the tab.
 *
 * The fix is `useReloadOnReactivate`, and the pairing is what this file pins:
 * caching without an activation hook reintroduces the bug, and the two live in
 * different files, so nothing else would notice them drifting apart.
 *
 * ## Why this is a structure test and not a behavioral one
 *
 * `tests/components/FacilityManagement.spec.ts` stubs `KeepAlive` transparently
 * and says why: KeepAlive calls `activate`/`deactivate` on the instances it
 * caches, and a template-string stub has neither, so mounting through a real
 * one throws on the second tab switch. That file already records what the
 * limitation costs. This is the cheap half that can be stated without solving
 * it -- and it is the half that fails if somebody deletes the KeepAlive, which
 * is exactly what a mutation check found nothing else catching.
 *
 * `DoorManagement.spec.ts` owns the behavioral half, mounting the real
 * component inside a real KeepAlive that toggles.
 *
 * What this does NOT prove: that the reload actually refreshes the right data.
 * That is the component tier's job, and it does it.
 */

import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const read = (rel: string) => readFileSync(resolve(process.cwd(), rel), 'utf8')

const HOST = read('src/components/FacilityManagement.vue')

/** The body between `<KeepAlive>` and `</KeepAlive>` in the tab host. */
function cachedBody(): string {
  const open = HOST.indexOf('<KeepAlive>')
  const close = HOST.indexOf('</KeepAlive>')
  expect(
    open,
    'FacilityManagement no longer wraps its tabs in <KeepAlive>.\n\n' +
      'If that was deliberate, the tabs now remount on every switch and ' +
      'useReloadOnReactivate is redundant -- delete it and this file rather ' +
      'than leaving a rule nobody can trace back to a reason. If it was not ' +
      'deliberate, tab state is being thrown away on every click.'
  ).toBeGreaterThan(-1)
  expect(close).toBeGreaterThan(open)
  // After the opening tag, not from it: including `<KeepAlive>` itself makes
  // the tag-matching regex below report `KeepAlive` as one of the tabs, and the
  // rule then goes looking for `src/components/KeepAlive.vue`.
  return HOST.slice(open + '<KeepAlive>'.length, close)
}

/**
 * The components rendered inside the KeepAlive, derived rather than listed.
 *
 * Derived so a fifth tab cannot be added without being held to the same rule.
 * A hardcoded list would go quietly out of date, which is the failure mode this
 * whole tier exists to catch.
 */
function cachedTabs(): string[] {
  // `m[1]` is `string | undefined` under `noUncheckedIndexedAccess`, and the
  // filter is how that becomes a fact rather than an assertion. A capture group
  // that matched nothing would otherwise arrive as `undefined` and be looked up
  // as `src/components/undefined.vue`.
  return [...cachedBody().matchAll(/<([A-Z][A-Za-z0-9]*)\b/g)]
    .map((m) => m[1])
    .filter((name): name is string => name !== undefined)
}

describe('the facility tab host', () => {
  it('found the tabs it is meant to be checking', () => {
    // Anti-vacuity. Every assertion below iterates this list, and an empty one
    // would make all of them pass while checking nothing -- the exact shape
    // this tier is written to avoid.
    const tabs = cachedTabs()
    expect(
      tabs.length,
      `parsed ${tabs.length} tab components out of the KeepAlive`
    ).toBeGreaterThan(2)
    expect(tabs).toContain('DoorManagement')
    expect(tabs).toContain('PlaceManagement')
  })

  it.each(['DoorManagement', 'PlaceManagement', 'ScheduleManagement', 'FacilityGraph'])(
    '%s is one of the cached tabs',
    (name) => {
      // Guards the derivation itself: if the regex stopped matching, the list
      // would shrink and the rule below would quietly cover fewer components.
      expect(cachedTabs()).toContain(name)
    }
  )

  it('every cached tab reloads when it is shown again', () => {
    for (const name of cachedTabs()) {
      const source = read(`src/components/${name}.vue`)
      expect(
        source,
        `${name} is rendered inside the KeepAlive but does not use ` +
          'useReloadOnReactivate, so it loads its data once per page load and ' +
          'never again. Anything it reads that another tab owns is stale from ' +
          'the moment somebody edits it -- issue #11.'
      ).toContain('useReloadOnReactivate')
    }
  })

  it('no cached tab still loads only on mount', () => {
    // The other half of the same rule. A component that kept `onMounted(load)`
    // *and* added the composable would load everything twice on first paint,
    // which is a real cost on a page that fetches five collections.
    for (const name of cachedTabs()) {
      const source = read(`src/components/${name}.vue`)
      expect(
        source.includes('onMounted(load'),
        `${name} loads on mount as well as on reactivation, so first paint ` +
          'fetches everything twice'
      ).toBe(false)
    }
  })

  it('the composable covers the standalone case too', () => {
    // These components are also mounted outside any KeepAlive. `onActivated`
    // never fires there, so the composable has to call `onMounted` itself --
    // and if it ever stopped, every standalone mount would render empty.
    const composable = read('src/composables/useReloadOnReactivate.ts')
    expect(composable).toContain('onMounted')
    expect(composable).toContain('onActivated')
  })
})
