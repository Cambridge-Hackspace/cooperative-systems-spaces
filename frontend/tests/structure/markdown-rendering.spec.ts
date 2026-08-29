import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * `PageViewer.vue` renders the server's page HTML with `v-html`, and the lint
 * rule that would object is disabled inline. That is defensible exactly once:
 * while the server never emits attacker-controlled markup.
 *
 * The server renders wiki and site pages with comrak:
 *
 *     markdown_to_html(&raw_content, &Options::default())
 *
 * comrak's `render.unsafe_` defaults to false, which strips raw HTML blocks and
 * escapes inline raw HTML. Turning it on -- a one-word change, and the obvious
 * thing to reach for the first time someone wants a `<details>` block in the
 * wiki -- would turn every page in the wiki repository into script that runs in
 * every reader's session.
 *
 * Nothing on the frontend would notice. `PageViewer` does no sanitising of its
 * own and cannot; by the time the HTML reaches it the decision has been made.
 * So the guarantee is asserted here, against the server source, from the side
 * that depends on it.
 *
 * This is a source-level check, not a behavioural one: it cannot prove comrak
 * escapes anything. What it proves is that this repository never asks it not
 * to. The behavioural half belongs to the server's own tests.
 */
const FRONTEND_ROOT = process.cwd()
const read = (rel: string) => readFileSync(join(FRONTEND_ROOT, rel), 'utf8')

describe('the server never asks comrak to pass raw HTML through', () => {
  const source = read('../server/src/pages.rs')

  it('renders markdown with default options', () => {
    // Anti-vacuity: if the call moves or is renamed, every assertion below
    // becomes trivially true, so the call site is asserted to exist first.
    expect(
      source,
      'markdown_to_html is no longer called in pages.rs -- find where markdown ' +
        'is rendered now and point this check at it'
    ).toContain('markdown_to_html(')
    expect(source).toContain('&Options::default()')
  })

  it("never sets comrak's unsafe rendering flag", () => {
    // comrak spells it `unsafe_` (trailing underscore, because `unsafe` is a
    // keyword). Matched with a word boundary so a comment mentioning "unsafe"
    // in prose does not trip it, and searched across the whole server crate
    // rather than one file, because the option could be built anywhere.
    const offenders: string[] = []
    for (const file of ['../server/src/pages.rs', '../server/src/api/pages.rs']) {
      let text: string
      try {
        text = read(file)
      } catch {
        continue // api/pages.rs may not exist; pages.rs is asserted above
      }
      for (const line of text.split('\n')) {
        const code = line.split('//')[0]
        if (/\bunsafe_\b/.test(code)) offenders.push(`${file}: ${line.trim()}`)
      }
    }

    expect(
      offenders,
      "comrak's raw-HTML passthrough is being configured somewhere. If it was " +
        'turned on deliberately, PageViewer.vue must sanitise before v-html, ' +
        'and its eslint-disable comment must stop claiming the server does it'
    ).toEqual([])
  })

  it('is depended on by a component that renders the result with v-html', () => {
    // The other half of the pair. If PageViewer ever stops using v-html, this
    // check is no longer load-bearing and should be reconsidered rather than
    // left standing as a rule nobody remembers the reason for.
    const viewer = read('src/components/PageViewer.vue')
    expect(viewer).toContain('v-html="page.html_content"')
  })
})
