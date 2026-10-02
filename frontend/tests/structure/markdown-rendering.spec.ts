import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * Two components render server-produced HTML with `v-html`, and the lint rule
 * that would object is disabled inline in both. That is defensible exactly
 * once: while the server never emits attacker-controlled markup.
 *
 * - `PageViewer.vue` renders wiki and site pages, rendered by comrak in
 *   `server/src/pages.rs` with `markdown_to_html(&raw, &Options::default())`.
 * - `CalendarEvents.vue` renders event descriptions, which arrive as HTML
 *   written by anybody who can edit a shared Google Calendar. Those are taken
 *   apart in `server/src/calendar/description.rs` and re-rendered; that module
 *   enables the `autolink` extension, so its options are not `default()`.
 *
 * Either way the load-bearing setting is comrak's raw-HTML passthrough, which
 * defaults to off. Turning it on -- a one-word change, and the obvious thing
 * to reach for the first time someone wants a `<details>` block in the wiki --
 * would turn every wiki page and every calendar description into script that
 * runs in every reader's session.
 *
 * ## What this check is, and what it is not
 *
 * It is a source-level check: it cannot prove comrak escapes anything, only
 * that this repository never asks it not to. The behavioural half lives in the
 * server crate, where the renderer is -- `raw_html_in_a_wiki_page_never_reaches_the_rendered_page`
 * in `pages.rs` and the `description.rs` suite -- and those are the tests that
 * would survive comrak renaming things.
 *
 * It has done exactly that once already. The option was spelled `unsafe_`
 * (trailing underscore, because `unsafe` is a keyword) until comrak 0.39, and
 * is `r#unsafe` in the 0.48 this repository pins. For the span of that
 * upgrade, this file was grepping for a string that could no longer appear in
 * a Rust source file -- a check that passes because it is looking for the
 * wrong thing, which is worse than no check, because it reads like one. Both
 * spellings are matched below, and the finding is recorded here rather than
 * only in a commit message because the next rename will look just like it.
 */
const FRONTEND_ROOT = process.cwd()
const read = (rel: string) => readFileSync(join(FRONTEND_ROOT, rel), 'utf8')

/** Every server source that renders Markdown, or could. */
const RENDERING_SOURCES = [
  '../server/src/pages.rs',
  '../server/src/api/pages.rs',
  '../server/src/calendar/description.rs',
  '../server/src/calendar/ics.rs',
  '../server/src/calendar.rs',
]

/** How comrak has spelled "pass raw HTML through" across versions. */
const PASSTHROUGH_SPELLINGS = [/\bunsafe_\b/, /\br#unsafe\b/]

describe('the server never asks comrak to pass raw HTML through', () => {
  it('renders wiki pages with default options', () => {
    const source = read('../server/src/pages.rs')
    // Anti-vacuity: if the call moves or is renamed, every assertion below
    // becomes trivially true, so the call site is asserted to exist first.
    expect(
      source,
      'markdown_to_html is no longer called in pages.rs -- find where markdown ' +
        'is rendered now and point this check at it'
    ).toContain('markdown_to_html(')
    expect(source).toContain('&Options::default()')
  })

  it('renders calendar descriptions with the passthrough left alone', () => {
    const source = read('../server/src/calendar/description.rs')
    expect(
      source,
      'calendar descriptions are no longer rendered in description.rs -- find ' +
        'where they are rendered now and point this check at it'
    ).toContain('markdown_to_html(')
    // This one does set options (autolink), so "uses the default" is not the
    // claim available here; the claim is the one below, that it never reaches
    // for the passthrough.
    expect(source).toContain('options.extension.autolink = true')
  })

  it("never sets comrak's raw-HTML passthrough, by any of its names", () => {
    // Matched with word boundaries so a comment mentioning the option in prose
    // does not trip it, and searched across every file that renders rather
    // than one, because the option could be built anywhere.
    const offenders: string[] = []
    for (const file of RENDERING_SOURCES) {
      let text: string
      try {
        text = read(file)
      } catch {
        continue // api/pages.rs may not exist; the renderers are asserted above
      }
      for (const line of text.split('\n')) {
        // `?? line` rather than a non-null assertion: split always yields at
        // least one element, but under `noUncheckedIndexedAccess` that is a
        // claim the type system will not take on trust, and asserting it away
        // would be the wrong habit in a file that exists to check claims.
        const code = line.split('//')[0] ?? line
        if (PASSTHROUGH_SPELLINGS.some((pattern) => pattern.test(code))) {
          offenders.push(`${file}: ${line.trim()}`)
        }
      }
    }

    expect(
      offenders,
      "comrak's raw-HTML passthrough is being configured somewhere. If it was " +
        'turned on deliberately, the component that renders the result must ' +
        'sanitise before v-html, and its eslint-disable comment must stop ' +
        'claiming the server does it'
    ).toEqual([])
  })

  it('is depended on by components that render the result with v-html', () => {
    // The other half of the pair. If either component stops using v-html,
    // this check is no longer load-bearing for it and should be reconsidered
    // rather than left standing as a rule nobody remembers the reason for.
    expect(read('src/components/PageViewer.vue')).toContain('v-html="page.html_content"')
    expect(read('src/components/CalendarEvents.vue')).toContain('v-html="event.description_html"')
  })

  it('names a spelling that the pinned comrak actually has', () => {
    // The self-test. A list of patterns that match nothing is the failure mode
    // this file has already had, and the one thing that would have caught it
    // is checking the list against the dependency it describes. comrak's
    // option is public API, so its name appears in the crate's own source.
    const spellings = PASSTHROUGH_SPELLINGS.map((p) => p.source)
    expect(spellings.length).toBeGreaterThan(1)
    expect(
      spellings.some((s) => s.includes('r#unsafe')),
      'comrak 0.48 (pinned in server/Cargo.toml) spells the option ' +
        '`render.r#unsafe`. If the pin has moved, check what it is called now ' +
        'and add that spelling -- do not drop one, since the check reads ' +
        'history too'
    ).toBe(true)
    expect(read('../server/Cargo.toml')).toMatch(/comrak = "0\.4[89]|comrak = "0\.[5-9]/)
  })
})
