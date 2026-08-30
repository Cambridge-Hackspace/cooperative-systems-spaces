import { afterEach, beforeEach, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

// `utils/api.ts` imports `stores/auth.ts` and `stores/auth.ts` imports
// `utils/api.ts`. The cycle is benign at module-evaluation time because
// `useAuthStore()` is only *called* inside the axios interceptor callbacks
// (api.ts:53 and api.ts:73), never at the top level — but it does mean anything
// that reaches an interceptor needs an active Pinia. Installing one for every
// test costs nothing and removes a whole class of "getActivePinia() was called
// with no active Pinia" failures that read like a bug in the component under
// test rather than in the harness.
//
// tests/structure/store-cycle.spec.ts asserts `useAuthStore` is never hoisted
// out of those callbacks, so the cycle stays benign rather than becoming a
// module-initialization order problem nobody can debug.
beforeEach(() => {
  setActivePinia(createPinia())
  localStorage.clear()
  sessionStorage.clear()

  // A frozen clock, so date and schedule rendering is a constant rather than a
  // function of when CI happened to run. `shouldAdvanceTime` keeps
  // promise-based code from deadlocking; a test that genuinely wants real
  // timers calls `vi.useRealTimers()` itself and says why.
  vi.useFakeTimers({ shouldAdvanceTime: true })
  vi.setSystemTime(new Date('2026-01-15T12:00:00.000Z'))
})

afterEach(() => {
  vi.useRealTimers()
})

// ---------------------------------------------------------------------------
// A Vue warning is a test failure
// ---------------------------------------------------------------------------
// `[Vue warn]` in a component test is almost never noise. It means an
// unresolved component, a prop of the wrong type, a missing required prop, a
// duplicate key in a v-for, or a ref used after unmount -- every one of which is
// a defect the test was too permissive to notice. Left as console output it
// scrolls past, and the run that eventually does contain a real warning looks
// exactly like the fifty before it.
//
// There is deliberately no allowlist. An allowlist is where a warning goes to
// be forgotten, and the two ways a genuine one arises are both better handled
// at the call site: an unresolved component wants a stub, and a prop warning
// wants the prop. If a warning ever turns out to be unavoidable, the right
// answer is to say so in the test that provokes it, not here.
//
// `config.warnHandler` is Vue's own hook, so this catches warnings raised
// during render as well as ones printed from a lifecycle hook.
import { config } from '@vue/test-utils'

const vueWarnings: string[] = []

config.global.config.warnHandler = (msg, _instance, trace) => {
  vueWarnings.push(`${msg}${trace ? `\n${trace}` : ''}`)
}

beforeEach(() => {
  vueWarnings.length = 0
})

afterEach(() => {
  if (vueWarnings.length > 0) {
    const count = vueWarnings.length
    const collected = vueWarnings.join('\n\n')
    // Cleared before throwing, or one warning fails every test after it too.
    vueWarnings.length = 0
    throw new Error(
      `Vue raised ${count} warning(s) during this test:\n\n${collected}\n\n` +
        'Fix the cause rather than silencing it: an unresolved component wants a ' +
        'stub in `global.stubs`, a prop warning wants the prop.'
    )
  }
})

// ---------------------------------------------------------------------------
// `window.matchMedia`
// ---------------------------------------------------------------------------
// jsdom does not implement it, and `utils/theme.ts` calls it at module scope --
// so importing anything that transitively reaches the theme util throws
// `window.matchMedia is not a function` before a single test runs, which
// presents as a suite that failed to collect rather than as a missing DOM API.
//
// Deliberately a working implementation rather than a stub returning
// `{ matches: false }`. A frozen `false` would make `resolveTheme` untestable
// for the dark case, and every test asserting the light branch would pass for
// the wrong reason. `setPrefersDark()` drives it, and dispatches `change` to
// the listeners `onSystemThemeChange` registers, so the subscription path is
// exercised too.
const mediaListeners = new Set<(e: MediaQueryListEvent) => void>()
let prefersDarkNow = false

export function setPrefersDark(value: boolean): void {
  if (value === prefersDarkNow) return
  prefersDarkNow = value
  const event = { matches: value, media: '(prefers-color-scheme: dark)' } as MediaQueryListEvent
  for (const listener of mediaListeners) listener(event)
}

Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: (query: string) => ({
    get matches() {
      return query.includes('prefers-color-scheme: dark') ? prefersDarkNow : false
    },
    media: query,
    onchange: null,
    addEventListener: (_: string, cb: (e: MediaQueryListEvent) => void) => mediaListeners.add(cb),
    removeEventListener: (_: string, cb: (e: MediaQueryListEvent) => void) =>
      mediaListeners.delete(cb),
    addListener: (cb: (e: MediaQueryListEvent) => void) => mediaListeners.add(cb),
    removeListener: (cb: (e: MediaQueryListEvent) => void) => mediaListeners.delete(cb),
    dispatchEvent: () => false,
  }),
})

// The preference is process-global and `utils/theme.ts` captures the
// MediaQueryList once, so a test that turns it on must not leak into the next.
afterEach(() => {
  setPrefersDark(false)
})
