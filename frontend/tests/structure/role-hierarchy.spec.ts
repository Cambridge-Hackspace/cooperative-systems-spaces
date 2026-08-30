import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * The role hierarchy is written out verbatim in two separate TypeScript files,
 * and a third time on the server. Nothing makes them agree.
 *
 * `stores/auth.ts` gates what the UI shows; `router/index.ts` gates what a URL
 * will open. If they drift, a user is shown a link to a page the router will
 * bounce them off, or — worse in the other direction — the router admits
 * someone the UI never intended to let in.
 *
 * This asserts both against a third copy written out here. That is the point:
 * the methodology is explicit that a check derived from the structure it is
 * checking agrees with itself no matter what, so the duplication *is* the
 * check. When the map is eventually extracted into `src/lib/roles.ts`, this
 * test keeps its own copy and therefore keeps its value.
 */
const EXPECTED: Record<string, number> = {
  unknown: 0,
  newbie: 1,
  member: 2,
  staff: 3,
  admin: 4,
}

// `process.cwd()` rather than `new URL(..., import.meta.url)`.
//
// Under the jsdom environment the global `URL` resolves relative references
// against the document's base — `http://localhost:3000/` — not against the
// file URL passed as the base argument, so `new URL('../../src/x', import.meta.url)`
// silently yields an http URL and `fileURLToPath` then throws or, worse,
// produces a path that happens to exist. vitest sets cwd to `test.root`, which
// this config pins to the frontend directory, so this is deterministic.
const FRONTEND_ROOT = process.cwd()
const src = (rel: string) => readFileSync(join(FRONTEND_ROOT, 'src', rel), 'utf8')

/**
 * Pull `'name': N` pairs out of the first `roleHierarchy` object literal in a
 * file. Deliberately narrow: it matches the literal only, so a comment
 * mentioning `'admin': 4` elsewhere cannot satisfy it.
 */
function roleHierarchyIn(file: string): Record<string, number> {
  const text = src(file)
  const start = text.indexOf('roleHierarchy')
  expect(start, `no roleHierarchy literal in src/${file}`).toBeGreaterThan(-1)

  const open = text.indexOf('{', start)
  const close = text.indexOf('}', open)
  expect(close, `unterminated roleHierarchy literal in src/${file}`).toBeGreaterThan(open)

  const body = text.slice(open + 1, close)
  const found: Record<string, number> = {}
  for (const m of body.matchAll(/['"]?([a-z]+)['"]?\s*:\s*(\d+)/g)) {
    // Narrowed rather than asserted with `!`. A capture group can be undefined
    // if the pattern ever changes, and silently writing `found[undefined]`
    // would corrupt the comparison the whole test rests on.
    const [, name, level] = m
    if (name === undefined || level === undefined) continue
    found[name] = Number(level)
  }
  return found
}

describe('role hierarchy', () => {
  it('is identical in the auth store and the router guard', () => {
    const store = roleHierarchyIn('stores/auth.ts')
    const router = roleHierarchyIn('router/index.ts')
    expect(store).toEqual(EXPECTED)
    expect(router).toEqual(EXPECTED)
  })

  it('was actually found in both files, so the comparison is not vacuous', () => {
    // Guards the guard: a scraper that silently returned {} would make the
    // assertion above compare two empty objects and pass.
    expect(Object.keys(roleHierarchyIn('stores/auth.ts'))).toHaveLength(5)
    expect(Object.keys(roleHierarchyIn('router/index.ts'))).toHaveLength(5)
  })
})

describe('the api.ts <-> auth.ts import cycle', () => {
  /**
   * `utils/api.ts` imports `stores/auth.ts`, which imports `utils/api.ts`.
   *
   * The cycle is benign *only* because `useAuthStore()` is called inside the
   * axios interceptor callbacks, at request time, rather than at module scope.
   * Hoist it out of those callbacks and the cycle becomes a
   * module-initialization-order problem whose symptom — an undefined import,
   * somewhere else entirely — is very hard to trace back here.
   *
   * This asserts the shape that keeps it benign.
   */
  it('never calls useAuthStore at module scope in api.ts', () => {
    const text = src('utils/api.ts')
    const offenders = text
      .split('\n')
      .map((line, i) => [i + 1, line] as const)
      .filter(([, line]) => line.includes('useAuthStore()'))
      // A call at module scope starts at column 0 or 2; the interceptor bodies
      // are nested at least four spaces deep.
      .filter(([, line]) => /^\s{0,3}(const|let|var)?\s*\w*\s*=?\s*useAuthStore\(\)/.test(line))

    expect(
      offenders,
      'useAuthStore() must stay inside the interceptor callbacks; calling it at ' +
        'module scope turns a benign import cycle into an initialization-order bug'
    ).toEqual([])
  })

  it('still has the cycle it is guarding, so the guard is not vacuous', () => {
    expect(src('utils/api.ts')).toContain('useAuthStore')
    expect(src('stores/auth.ts')).toMatch(/from '@\/utils\/api'/)
  })
})
