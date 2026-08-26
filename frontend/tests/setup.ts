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
// module-initialisation order problem nobody can debug.
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
