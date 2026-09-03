import { onActivated, onMounted } from 'vue'

/**
 * Load on mount, and load again whenever a cached component is shown again.
 *
 * `FacilityManagement.vue` wraps its tab chain in `<KeepAlive>`, which is the
 * right call -- it keeps a half-filled form, a selection and a scroll position
 * across a tab switch, and re-fetching everything on every click would make the
 * tabs feel worse, not better. The cost is that `onMounted` fires exactly once
 * per page load. A tab that reads data another tab owns is therefore stale from
 * the moment somebody edits it, and the only thing that fixes it is a manual
 * refresh.
 *
 * Issue #11 is the sharpest version: add rooms on Places, open "+ New door",
 * and be told there are no places to pick from. That the reporter's workaround
 * was pressing refresh is the tell -- refreshing is the only thing that
 * remounts the component.
 *
 * ## Why both hooks
 *
 * `onActivated` fires on the initial mount *as well as* on every later
 * activation, but only when the component is inside a `<KeepAlive>`. Outside
 * one it never fires at all -- and these components are also mounted
 * standalone, from `/admin/doors` and friends. So `onMounted` is what covers
 * the standalone case, and the first activation is skipped so the KeepAlive
 * case does not load everything twice on first paint.
 *
 * The flag is a plain `let`, not a `ref`: nothing renders it, and a ref here
 * would suggest to a reader that it participates in reactivity.
 *
 * @param load  Loads everything the component shows. Called with no arguments,
 *              and its result is awaited only so a rejection surfaces as an
 *              unhandled rejection rather than vanishing.
 */
export function useReloadOnReactivate(load: () => void | Promise<void>): void {
  onMounted(() => {
    void load()
  })

  let firstActivation = true
  onActivated(() => {
    // The initial mount inside a KeepAlive fires `mounted` and then
    // `activated`. Reloading here would double every request on first paint.
    if (firstActivation) {
      firstActivation = false
      return
    }
    void load()
  })
}
