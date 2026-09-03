import { ALLOWED_LINK_SCHEMES, isSafeLinkUrl } from './urls'

/**
 * Why a training step's documentation URL cannot be used, or null if it can.
 *
 * The check itself is `isSafeLinkUrl`, unchanged and unwrapped: an allowlist of
 * http/https/mailto that also permits a relative reference, which is what makes
 * `/wiki/safety/lathe` and `https://example.org/manual.pdf` both expressible in
 * one field. See the reasoning in `lib/urls.ts` -- the same href-injection
 * hazard applies here, because `ToolTrainingModal` renders this value as a link
 * and Vue does not sanitise an href binding.
 *
 * This wrapper exists for the empty case, which differs from the curated-links
 * one. A home link with no URL is meaningless, so `isSafeLinkUrl('')` is false.
 * A training step with no documentation is entirely ordinary -- most steps are
 * a trainer standing next to somebody -- so blank has to be accepted here, and
 * a component reusing `isSafeLinkUrl` directly would reject every step that has
 * no document.
 *
 * Returns a message rather than a boolean so the two callers cannot word the
 * same refusal two different ways.
 */
export function materialsUrlError(raw: string | undefined | null): string | null {
  const trimmed = (raw ?? '').trim()
  if (trimmed === '') return null
  if (isSafeLinkUrl(trimmed)) return null
  return (
    'That documentation link cannot be used. Permitted: ' +
    `${ALLOWED_LINK_SCHEMES.join(', ')} or a path on this site.`
  )
}
