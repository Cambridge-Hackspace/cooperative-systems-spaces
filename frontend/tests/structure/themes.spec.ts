// Tier 3: the theme list exists in three places and they have to agree.
//
//   1. `tailwind.config.js` -> `daisyui.themes` decides which themes daisyUI
//      actually compiles into the stylesheet;
//   2. `src/components/ThemePicker.vue` decides which ones a person can pick;
//   3. `tests/fixtures/themes.json` is what the contrast audit iterates.
//
// A theme in the picker but not in the config is a button that switches to a
// theme with no CSS — daisyUI silently falls back and the page looks unchanged,
// so it reads as a broken button rather than a missing theme. A theme in the
// config but not in the picker is dead weight in every stylesheet the site
// ships. And a fixture out of step with either makes the accessibility audit
// approximate while reporting as exhaustive.
//
// The comment in ThemePicker.vue already says "must match the config exactly".
// This is what turns that from a comment into a failing test.
//
// The fixture is the third copy on purpose. Deriving it from either source
// would make the audit agree with whatever that source said; keeping it
// separate means the audit has its own statement of what it covers, and a theme
// added to both code paths still has to be added to the thing that tests them.

import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

// `process.cwd()`, not `import.meta.url`. jsdom installs a global `URL` that
// resolves against the *document* base (http://localhost:3000/), so
// `new URL('../../x', import.meta.url)` silently produces an http URL and
// `readFileSync` fails with something that reads like a missing file.
const ROOT = process.cwd()
const read = (p: string) => readFileSync(join(ROOT, p), 'utf8')

/** Theme names from `daisyui.themes` in tailwind.config.js, in order. */
function configThemes(): string[] {
  const src = read('tailwind.config.js')
  const start = src.indexOf('themes: [')
  if (start < 0) throw new Error('no `themes: [` in tailwind.config.js')

  // Walk to the matching bracket rather than regexing to the first `]`: the
  // list contains object literals full of their own brackets.
  let depth = 0
  let end = -1
  for (let i = src.indexOf('[', start); i < src.length; i += 1) {
    if (src[i] === '[') depth += 1
    else if (src[i] === ']') {
      depth -= 1
      if (depth === 0) {
        end = i
        break
      }
    }
  }
  if (end < 0) throw new Error('unterminated themes list in tailwind.config.js')
  const body = src.slice(start, end)

  const names: string[] = []
  for (const line of body.split('\n')) {
    // `?? ''` because `split` returns `string | undefined` under
    // noUncheckedIndexedAccess, and a scraper that silently skips a line it
    // could not read is a scraper that reports agreement it did not verify.
    const code = line.split('//')[0] ?? ''
    // A custom theme is an object key -- quoted when it contains a hyphen
    // ('css-light') and bare when it does not (afterdark), because that is what
    // a JavaScript object literal and prettier between them produce. Matching
    // only the quoted form dropped six of the fourteen and left the comparison
    // reporting differences that were entirely the scraper's.
    const custom = code.match(/^\s*'?([a-zA-Z0-9-]+)'?:\s*\{/)
    if (custom?.[1]) {
      names.push(custom[1])
      continue
    }
    // A built-in is a bare string in the list.
    const builtin = code.match(/^\s*'([a-zA-Z0-9-]+)',?\s*$/)
    if (builtin?.[1]) names.push(builtin[1])
  }
  return names
}

/** Theme values from the `themes` array in ThemePicker.vue, in order. */
function pickerThemes(): Array<{ value: string; label: string; group: string }> {
  const src = read('src/components/ThemePicker.vue')
  const start = src.indexOf('const themes = [')
  if (start < 0) throw new Error('no `const themes = [` in ThemePicker.vue')
  const end = src.indexOf(']', start)
  const body = src.slice(start, end)

  const out: Array<{ value: string; label: string; group: string }> = []
  for (const line of body.split('\n')) {
    const m = line.match(/value:\s*'([^']+)',\s*label:\s*'([^']+)',\s*group:\s*'([^']+)'/)
    // Every group is checked rather than asserted. A capture group can be
    // undefined even when the match succeeded, and pushing `undefined` here
    // would make the comparison below report a theme named "undefined" -- a
    // failure describing the parser rather than the thing being parsed.
    if (m?.[1] && m[2] && m[3]) out.push({ value: m[1], label: m[2], group: m[3] })
  }
  return out
}

function fixtureThemes(): string[] {
  // Annotated rather than trusted. `JSON.parse` is `any`, and an `any` flowing
  // into a comparison is how a fixture that lost its `themes` key would compare
  // `undefined` against the config list and fail with a message about the wrong
  // thing.
  const doc = JSON.parse(read('tests/fixtures/themes.json')) as { themes?: unknown }
  if (!Array.isArray(doc.themes)) {
    throw new Error('tests/fixtures/themes.json has no themes array')
  }
  return doc.themes as string[]
}

describe('the theme list', () => {
  it('is parsed from all three places', () => {
    // The guard on the guards. Any of the three scrapers silently returning
    // nothing would make every comparison below pass over empty sets.
    expect(configThemes().length, 'tailwind.config.js').toBeGreaterThanOrEqual(14)
    expect(pickerThemes().length, 'ThemePicker.vue').toBeGreaterThanOrEqual(14)
    expect(fixtureThemes().length, 'themes.json').toBeGreaterThanOrEqual(14)
  })

  it('is the same set in the config and the picker', () => {
    const config = configThemes()
    const picker = pickerThemes().map((t) => t.value)

    const onlyInPicker = picker.filter((t) => !config.includes(t))
    const onlyInConfig = config.filter((t) => !picker.includes(t))

    expect(
      onlyInPicker,
      'these themes are offered in the picker but daisyUI does not compile them; ' +
        'selecting one switches to a theme with no CSS, which looks like a broken button'
    ).toEqual([])
    expect(
      onlyInConfig,
      'these themes are compiled into every stylesheet the site ships and nobody can select them'
    ).toEqual([])
  })

  it('is the same order in the config and the picker', () => {
    // Order is not cosmetic here: it is the order the buttons appear in, and
    // the order the contrast audit reports its results in. Two lists that agree
    // as sets and disagree as sequences make a reader match up the wrong rows.
    expect(pickerThemes().map((t) => t.value)).toEqual(configThemes())
  })

  it('is exactly what the audit fixture claims to cover', () => {
    expect(fixtureThemes()).toEqual(configThemes())
  })
})

describe('the picker entries themselves', () => {
  it('gives every theme a label and a group', () => {
    for (const theme of pickerThemes()) {
      expect(theme.label.length, theme.value).toBeGreaterThan(0)
      expect(theme.group.length, theme.value).toBeGreaterThan(0)
    }
  })

  it('uses only groups the picker knows how to order', () => {
    // `themeGroups` filters against a fixed order array, so a theme in an
    // unlisted group is dropped from the UI entirely -- present in the source,
    // absent from the page, and nothing anywhere reports it.
    const src = read('src/components/ThemePicker.vue')
    const m = src.match(/const order = \[([^\]]+)\]/)
    expect(m, 'ThemePicker no longer declares a group order').not.toBeNull()
    const order = [...(m?.[1] ?? '').matchAll(/'([^']+)'/g)].map((x) => x[1])

    const groups = [...new Set(pickerThemes().map((t) => t.group))]
    const unordered = groups.filter((g) => !order.includes(g))
    expect(
      unordered,
      'themes in these groups are silently dropped by themeGroups, because the ' +
        '`order` array filters them out'
    ).toEqual([])
  })

  it('has no duplicate values', () => {
    const values = pickerThemes().map((t) => t.value)
    expect(new Set(values).size).toBe(values.length)
  })
})
