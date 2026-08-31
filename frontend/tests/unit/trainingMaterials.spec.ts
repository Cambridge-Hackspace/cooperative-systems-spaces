// Tier 1: the training-materials URL rule.
//
// A thin wrapper over `isSafeLinkUrl`, and the wrapper is the whole point: the
// empty case differs. A curated home link with no URL is meaningless, so
// `isSafeLinkUrl('')` is false. A training step with no documentation is
// entirely ordinary -- most steps are a trainer standing next to somebody --
// so a component reusing `isSafeLinkUrl` directly would reject every step that
// has no document.
//
// What this does NOT prove: that the URL resolves, that the host is one you
// trust, or that the page behind it says anything about safety. It answers
// "can this string execute script when rendered as an href", and nothing else.

import { describe, expect, it } from 'vitest'
import { materialsUrlError } from '@/lib/trainingMaterials'

describe('a documentation URL that may be used', () => {
  it('accepts a path on this site, which is how a wiki page is linked', () => {
    for (const ok of ['/wiki/safety/lathe', '/page/rules', '#section']) {
      expect(materialsUrlError(ok), ok).toBeNull()
    }
  })

  it('accepts an external document', () => {
    for (const ok of ['https://example.org/manual.pdf', 'http://10.0.0.9/lathe']) {
      expect(materialsUrlError(ok), ok).toBeNull()
    }
  })

  it('accepts nothing at all, and treats whitespace as nothing', () => {
    for (const blank of [undefined, null, '', '   ']) {
      expect(materialsUrlError(blank), JSON.stringify(blank)).toBeNull()
    }
  })
})

describe('a documentation URL that may not', () => {
  it('refuses every scheme that can execute', () => {
    for (const bad of [
      'javascript:alert(1)',
      'JavaScript:alert(1)',
      'data:text/html,<script>alert(1)</script>',
      'vbscript:msgbox(1)',
      'blob:https://example.org/abc',
    ]) {
      expect(materialsUrlError(bad), bad).not.toBeNull()
    }
  })

  it('says what is permitted rather than only that the value is wrong', () => {
    // One message, produced here rather than worded separately by each of the
    // two forms that can refuse.
    const message = materialsUrlError('javascript:alert(1)')
    expect(message).toContain('https:')
    expect(message).toContain('path on this site')
  })
})
