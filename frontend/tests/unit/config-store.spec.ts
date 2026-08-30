// Tier 1: the config store's feature-visibility helpers, against configs that
// are missing things.
//
// These four functions decide whether the wiki and site links appear in the
// navigation. They are called from computeds that run during `App.vue`'s
// render, and that placement is what makes them worth this much attention: **a
// throw here does not produce an error message, it freezes the application.**
// Vue stops patching a component whose render function throws, so App.vue holds
// whatever it last drew — which, during boot, is the full-screen
// `fixed inset-0 ... z-50` loading overlay. The page renders correctly and then
// accepts no input, with nothing visible anywhere except a console warning.
//
// That is exactly what happened. They read `config.value?.pages.wiki_enabled`:
// the optional chain covered `config` and stopped, so a config whose `pages`
// block was absent threw `Cannot read properties of undefined`.
//
// The type's own comment says feature blocks "default to false on older servers
// that don't yet emit these blocks", so a server that omits one is an expected
// case — the guard was simply one level too shallow to survive it.
//
// Every case below is a config a real server could send.

import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

import { useConfigStore } from '@/stores/config'
import type { PublicConfig } from '@/stores/config'

/** Seed the store's config directly; the helpers are pure given it. */
function withConfig(config: unknown) {
  const store = useConfigStore()
  ;(store as unknown as { config: unknown }).config = config
  return store
}

const HELPERS = [
  'shouldShowWikiInNav',
  'shouldShowSiteInNav',
  'shouldShowWikiOnHomePage',
  'shouldShowSiteOnHomePage',
] as const

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('a config the server did not fully populate', () => {
  // Each of these is a shape a server can actually produce, and every one of
  // them used to throw.
  const INCOMPLETE: Array<[string, unknown]> = [
    ['no config at all, before the first fetch', null],
    ['an older server that omits the pages block', { site: { site_name: 'X' } }],
    ['pages present but null', { site: { site_name: 'X' }, pages: null }],
    ['pages present but empty', { site: { site_name: 'X' }, pages: {} }],
  ]

  it.each(INCOMPLETE)('%s: every helper answers false rather than throwing', (_why, config) => {
    const store = withConfig(config)
    for (const helper of HELPERS) {
      expect(
        () => store[helper](),
        `${helper} threw. It is called from a computed during App.vue's render, ` +
          'so a throw here stops Vue patching the component and freezes the ' +
          'application behind its loading overlay.'
      ).not.toThrow()
      expect(store[helper](), helper).toBe(false)
    }
  })
})

describe('a complete config', () => {
  function complete(pages: Partial<PublicConfig['pages']>): unknown {
    return {
      site: { site_name: 'Test Space' },
      pages: {
        wiki_enabled: false,
        wiki_link: 'None',
        site_enabled: false,
        site_link: 'None',
        ...pages,
      },
    }
  }

  it('shows nothing when the feature is disabled, whatever the link says', () => {
    const store = withConfig(complete({ wiki_enabled: false, wiki_link: 'Both' }))
    expect(store.shouldShowWikiInNav()).toBe(false)
    expect(store.shouldShowWikiOnHomePage()).toBe(false)
  })

  it.each([
    ['Navigation', true, false],
    ['HomePage', false, true],
    ['Both', true, true],
    ['None', false, false],
  ])('wiki_link=%s puts it in the nav=%s and on the home page=%s', (link, inNav, onHome) => {
    const store = withConfig(complete({ wiki_enabled: true, wiki_link: link }))
    expect(store.shouldShowWikiInNav()).toBe(inNav)
    expect(store.shouldShowWikiOnHomePage()).toBe(onHome)
  })

  it('decides the site link independently of the wiki link', () => {
    // They read different keys and it would be easy for one to read the
    // other's -- two near-identical functions four lines apart.
    const store = withConfig(
      complete({
        wiki_enabled: true,
        wiki_link: 'Navigation',
        site_enabled: true,
        site_link: 'HomePage',
      })
    )
    expect(store.shouldShowWikiInNav()).toBe(true)
    expect(store.shouldShowSiteInNav()).toBe(false)
    expect(store.shouldShowWikiOnHomePage()).toBe(false)
    expect(store.shouldShowSiteOnHomePage()).toBe(true)
  })

  it('treats an unrecognized link location as "do not show"', () => {
    // The value is a Rust enum serialized as a string. A variant the frontend
    // does not know about must not default to showing something.
    const store = withConfig(complete({ wiki_enabled: true, wiki_link: 'Sidebar' }))
    expect(store.shouldShowWikiInNav()).toBe(false)
    expect(store.shouldShowWikiOnHomePage()).toBe(false)
  })
})
