// Tier 11's measurable half: is the text readable, in every theme?
//
// Fourteen themes ship. Nobody looks at all fourteen, and the ones nobody looks
// at are exactly where an unreadable pairing survives — a `text-error` that is
// dark red on a dark card renders, passes every other test in this repository,
// and cannot be read.
//
// So the contrast ratios are computed rather than eyeballed. WCAG's relative
// luminance, over the colors the themes actually declare, for the pairings the
// application actually renders.
//
// WHAT IT CHECKS, and why these pairs. daisyUI's `text-error`, `text-success`,
// `text-warning` and `text-info` set the foreground to the semantic color and
// leave the background to whatever card the element sits in — which in this
// application is `base-100`, `base-200` or `base-300`. `ProfileField.vue` puts
// its required marker and its error message in `text-error` inside a
// `form-control`; `ToolCard.vue` sits on `bg-base-300`. So a semantic color has
// to be legible on all three bases, not just on white.
//
// WHAT THIS DOES NOT PROVE. It reads the declared palette, not a rendered page.
// A class that does not exist, a color overridden by a stylesheet, an element
// whose real background is an ancestor three levels up — none of that is
// visible here. That is Tier 10's, in a browser, with `getComputedStyle`. This
// is the cheap version that runs on any machine in milliseconds and catches the
// case where the *palette itself* is unreadable.
//
// It also says nothing about color used as the only signal. A red badge and a
// green badge can both pass this and still be indistinguishable to somebody who
// cannot tell them apart, because the ratio against the background is identical.

import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

const ROOT = process.cwd()

// ---------------------------------------------------------------------------
// Color
// ---------------------------------------------------------------------------

/** Linear-light sRGB components in [0, 1]. */
type Linear = [number, number, number]

function linearizeChannel(srgb: number): number {
  return srgb <= 0.04045 ? srgb / 12.92 : Math.pow((srgb + 0.055) / 1.055, 2.4)
}

function fromHex(hex: string): Linear | null {
  const m = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(hex.trim())
  if (!m?.[1]) return null
  let h = m[1]
  if (h.length === 3)
    h = h
      .split('')
      .map((c) => c + c)
      .join('')
  const n = parseInt(h, 16)
  return [
    linearizeChannel(((n >> 16) & 255) / 255),
    linearizeChannel(((n >> 8) & 255) / 255),
    linearizeChannel((n & 255) / 255),
  ]
}

/**
 * `oklch(L% C H)` to linear-light sRGB.
 *
 * daisyUI's built-in themes are declared in OKLCH, so half the palette is
 * unreadable to this file without the conversion. The matrices are Björn
 * Ottosson's published OKLab coefficients; the output is *linear* already,
 * which is what WCAG's luminance formula wants — gamma-encoding it to a hex
 * string and linearizing it back would only add rounding.
 */
function fromOklch(value: string): Linear | null {
  const m = /^oklch\(\s*([\d.]+)%\s+([\d.]+)\s+([\d.]+)\s*\)$/i.exec(value.trim())
  if (!m) return null
  const L = Number(m[1]) / 100
  const C = Number(m[2])
  const H = (Number(m[3]) * Math.PI) / 180

  const a = C * Math.cos(H)
  const b = C * Math.sin(H)

  const l_ = L + 0.3963377774 * a + 0.2158037573 * b
  const m_ = L - 0.1055613458 * a - 0.0638541728 * b
  const s_ = L - 0.0894841775 * a - 1.291485548 * b

  const l = l_ * l_ * l_
  const mm = m_ * m_ * m_
  const s = s_ * s_ * s_

  const clamp = (x: number) => Math.min(1, Math.max(0, x))
  return [
    clamp(4.0767416621 * l - 3.3077115913 * mm + 0.2309699292 * s),
    clamp(-1.2684380046 * l + 2.6097574011 * mm - 0.3413193965 * s),
    clamp(-0.0041960863 * l - 0.7034186147 * mm + 1.707614701 * s),
  ]
}

function parse(value: string): Linear | null {
  return fromHex(value) ?? fromOklch(value)
}

function luminance([r, g, b]: Linear): number {
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

/** Parse, or fail loudly. For the known-answer tests, where null is the bug. */
function must(value: string): Linear {
  const c = parse(value)
  if (!c) throw new Error(`the color parser could not read ${value}`)
  return c
}

/** WCAG contrast ratio, 1 (identical) to 21 (black on white). */
function contrast(a: Linear, b: Linear): number {
  const la = luminance(a)
  const lb = luminance(b)
  const [hi, lo] = la > lb ? [la, lb] : [lb, la]
  return (hi + 0.05) / (lo + 0.05)
}

// ---------------------------------------------------------------------------
// The palettes
// ---------------------------------------------------------------------------

type Palette = Record<string, string>

/** The eight themes this project declares in tailwind.config.js. */
function customThemes(): Record<string, Palette> {
  const src = readFileSync(join(ROOT, 'tailwind.config.js'), 'utf8')
  const out: Record<string, Palette> = {}

  // Each is `name: {` or `'name': {` followed by `key: '#hex',` lines.
  const re = /^\s*'?([a-zA-Z0-9-]+)'?:\s*\{$/gm
  let m: RegExpExecArray | null
  while ((m = re.exec(src)) !== null) {
    const name = m[1]
    // A capture that came back undefined would key the result on "undefined"
    // and put a whole palette under a theme nobody declared.
    if (!name) continue
    if (name === 'colors' || name === 'extend' || name === 'theme' || name === 'daisyui') continue
    const rest = src.slice(m.index + m[0].length)
    const end = rest.indexOf('\n        },')
    if (end < 0) continue
    const body = rest.slice(0, end)
    const palette: Palette = {}
    for (const line of body.split('\n')) {
      const kv = /^\s*'?([a-z0-9-]+)'?:\s*'(#[0-9a-fA-F]{3,8})'/.exec(line)
      // Both groups checked. A capture that came back undefined would key the
      // palette on "undefined" and put a color where no theme declares one.
      const key = kv?.[1]
      const value = kv?.[2]
      if (key && value) palette[key] = value
    }
    if (Object.keys(palette).length >= 5) out[name] = palette
  }
  return out
}

/** The six daisyUI built-ins this project ships, from daisyUI's own data. */
async function builtinThemes(): Promise<Record<string, Palette>> {
  const mod = (await import('daisyui/src/theming/themes.js')) as unknown as {
    default?: Record<string, Palette>
  }
  const all = mod.default ?? (mod as unknown as Record<string, Palette>)
  const shipped = ['lofi', 'black', 'light', 'dark', 'cupcake', 'corporate']
  const out: Record<string, Palette> = {}
  for (const name of shipped) {
    const palette = all[name]
    if (palette) out[name] = palette
  }
  return out
}

/** Backgrounds an element carrying a semantic text color actually sits on. */
const BASES = ['base-100', 'base-200', 'base-300'] as const

/**
 * Foregrounds daisyUI's `text-*` utilities set, which this application uses.
 *
 * `error` is the one that matters most: `ProfileField.vue` puts both the
 * required marker and the validation message in `text-error`, so a theme where
 * it fails is a theme where the reason a form will not submit is unreadable.
 */
const SEMANTIC = ['error', 'warning', 'success', 'info'] as const

/** WCAG AA for normal-size text. */
const AA_NORMAL = 4.5

describe('the palettes were read', () => {
  it('finds all fourteen themes', async () => {
    // Either loader silently returning nothing would make every assertion below
    // pass over an empty set -- which for an accessibility check is the worst
    // possible failure, because the report says "all themes pass".
    const custom = customThemes()
    const builtin = await builtinThemes()
    expect(Object.keys(custom).length, `custom: ${Object.keys(custom).join(',')}`).toBe(8)
    expect(Object.keys(builtin).length, `builtin: ${Object.keys(builtin).join(',')}`).toBe(6)
  })

  it('parses every color it finds', async () => {
    const all = { ...customThemes(), ...(await builtinThemes()) }
    const unparsed: string[] = []
    for (const [theme, palette] of Object.entries(all)) {
      for (const [key, value] of Object.entries(palette)) {
        // daisyUI theme objects carry CSS custom properties alongside the
        // colors -- --rounded-box, --animation-btn and so on. They are not
        // colors and skipping them is not a narrowing; the assertion is about
        // color values the audit could not read.
        if (key.startsWith('--') || key === 'color-scheme') continue
        if (parse(value) === null) unparsed.push(`${theme}.${key} = ${value}`)
      }
    }
    expect(
      unparsed,
      'these colors were skipped, so every ratio computed from them is missing ' +
        'rather than wrong -- which is how an audit reports a clean bill of health ' +
        'for a palette it could not read'
    ).toEqual([])
  })
})

describe('the reference implementation is right', () => {
  // Three known answers. Without these, a sign error in the OKLab matrices
  // produces plausible-looking ratios for every theme and the whole file is
  // confidently wrong.
  it('black on white is 21:1', () => {
    expect(contrast(must('#000000'), must('#ffffff'))).toBeCloseTo(21, 1)
  })

  it('a color against itself is 1:1', () => {
    expect(contrast(must('#3b82f6'), must('#3b82f6'))).toBeCloseTo(1, 5)
  })

  it('agrees with a published value', () => {
    // #767676 on #ffffff is the canonical "exactly AA" grey: 4.54:1.
    expect(contrast(must('#767676'), must('#ffffff'))).toBeCloseTo(4.54, 1)
  })

  it('converts OKLCH to the same place as the equivalent hex', () => {
    // oklch(100% 0 0) is white; oklch(0% 0 0) is black.
    expect(luminance(must('oklch(100% 0 0)'))).toBeCloseTo(1, 2)
    expect(luminance(must('oklch(0% 0 0)'))).toBeCloseTo(0, 2)
  })
})

describe('semantic text is legible on every surface it lands on', () => {
  it('reports every pairing below AA, per theme', async () => {
    const all = { ...customThemes(), ...(await builtinThemes()) }
    const failures: string[] = []
    let checked = 0

    for (const [theme, palette] of Object.entries(all)) {
      for (const fg of SEMANTIC) {
        const declared = palette[fg]
        const f = declared ? parse(declared) : null
        if (!f) continue // not declared by this theme; daisyUI derives it
        for (const bg of BASES) {
          const surface = palette[bg]
          const b = surface ? parse(surface) : null
          if (!b) continue
          checked += 1
          const ratio = contrast(f, b)
          if (ratio < AA_NORMAL) {
            failures.push(
              `${theme}: text-${fg} on ${bg} is ${ratio.toFixed(2)}:1 ` +
                `(${palette[fg]} on ${palette[bg]}), below ${AA_NORMAL}`
            )
          }
        }
      }
    }

    expect(checked, 'no pairings were checked at all').toBeGreaterThan(50)

    // Reported rather than asserted clean, for now. Several themes fail, and
    // changing a designer's palette from a test is not this file's business --
    // the number is what makes the conversation possible. When the palettes are
    // fixed, change this to `toEqual([])` and delete the snapshot.
    //
    // The count is pinned so it can only go down: a new theme that adds an
    // unreadable pairing fails here, and fixing one fails here too, which is
    // the moment to lower the number.
    expect(
      failures.length,
      `semantic text below WCAG AA:\n${failures.join('\n')}\n\n` +
        'These are not hypothetical: ProfileField.vue renders both the required ' +
        'marker and the validation message in text-error, so a theme in this ' +
        'list is a theme where the reason a form will not submit cannot be read. ' +
        'If the number went DOWN, lower it here. If it went UP, a palette got worse.'
    ).toBe(EXPECTED_FAILURES)
  })
})

/**
 * How many semantic/base pairings are currently below AA.
 *
 * Measured, not chosen. It exists so the number can only go down — the same
 * shape as the blanket-500 ratchet in `checks/`, and for the same reason: the
 * alternative to a ratchet here is either a red suite forever or a palette
 * change made from a test.
 */
const EXPECTED_FAILURES = 36

// NOT CHECKED HERE: the separation between base-100, base-200 and base-300.
//
// The obvious thing to do with WCAG's 3:1 for non-text components is to point
// it at the surface tints, and it is wrong. That criterion is about the
// boundary of a *control* against its adjacent background -- an input's border,
// a focus ring. Card surfaces are deliberately close: a subtle elevation is the
// point of having three of them, and every one of the seven themes that
// declares all three lands between 1.1:1 and 1.7:1.
//
// Asserting 3:1 there would manufacture six findings out of a criterion that
// does not apply, which is worse than not checking: it puts noise in the report
// and teaches the next reader to discount it. What *would* be checkable is a
// control's border against the surface it sits on -- and that is a rendered
// value, not a palette value, so it is Tier 10's.

describe('at least one theme is meaningfully dark and one is meaningfully light', () => {
  // A sanity check on the whole exercise. If every theme resolved to the same
  // near-white base, the ratios above would all be similar and the audit would
  // be measuring one theme fourteen times.
  it('the base backgrounds span a real range', async () => {
    const all = { ...customThemes(), ...(await builtinThemes()) }
    const lums = Object.values(all)
      .map((p) => {
        const base = p['base-100']
        return base ? parse(base) : null
      })
      .filter((c): c is Linear => c !== null)
      .map(luminance)

    expect(lums.length).toBeGreaterThan(10)
    expect(Math.max(...lums), 'no theme has a light background').toBeGreaterThan(0.7)
    expect(Math.min(...lums), 'no theme has a dark background').toBeLessThan(0.1)
  })
})
