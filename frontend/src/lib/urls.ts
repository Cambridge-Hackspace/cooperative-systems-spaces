/**
 * Which URL schemes a curated link may use.
 *
 * `HomeLinkManagement` writes a URL that `HomeView.vue:85` renders as
 * `:href="l.url"` on the *public* home page. Vue does not sanitise an href
 * binding, so a `javascript:` URL saved there becomes a live script handler
 * served to signed-out visitors -- and neither the form nor
 * `api/home_links.rs:239`, which checks only that the string is non-empty,
 * stopped one.
 *
 * It takes an admin to save one, so this is not an unauthenticated hole. What
 * it is: a compromised or careless admin account turning into persistent script
 * execution against every visitor, out of a field that looks like it only picks
 * a destination. There is no legitimate `javascript:` link in a list of
 * destinations, so this is a missing check rather than a trade-off.
 *
 * An allowlist rather than a denylist, deliberately. `javascript:` is the
 * obvious one; `data:`, `vbscript:` and `blob:` are the ones a denylist forgets,
 * and browsers keep adding schemes. Naming what is permitted stays correct as
 * that list grows.
 *
 * What this does NOT do: validate that the URL resolves, or that the host is
 * one you trust. It answers "can this string execute script when rendered as an
 * href", and nothing else.
 */
const ALLOWED_SCHEMES = ['http:', 'https:', 'mailto:'] as const

export const ALLOWED_LINK_SCHEMES: readonly string[] = ALLOWED_SCHEMES

/**
 * True when `raw` is a URL safe to render as an `href`.
 *
 * A relative URL -- `/tools`, `#anchor` -- is permitted: it cannot carry a
 * scheme, so it cannot execute, and linking to a page of this site is an
 * obvious thing to want.
 */
export function isSafeLinkUrl(raw: string): boolean {
  const trimmed = raw.trim()
  if (trimmed === '') return false

  // `new URL` needs a base to parse a relative reference, and the base is what
  // tells the two apart: a string carrying its own scheme ignores the base.
  let parsed: URL
  try {
    parsed = new URL(trimmed, 'https://placeholder.invalid')
  } catch {
    return false
  }

  // A relative reference resolves against the placeholder, so it is safe by
  // construction -- it had no scheme of its own to smuggle in.
  if (parsed.origin === 'https://placeholder.invalid' && !/^[a-z][a-z0-9+.-]*:/i.test(trimmed)) {
    return true
  }

  return ALLOWED_SCHEMES.includes(parsed.protocol as (typeof ALLOWED_SCHEMES)[number])
}
