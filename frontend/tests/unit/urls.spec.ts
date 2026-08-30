// Tier 1: the link-URL allowlist.
//
// `HomeLinkManagement` writes a URL that `HomeView.vue:85` renders as
// `:href="l.url"` on the public home page, and Vue does not sanitise an href
// binding. Neither the form nor `api/home_links.rs:239` -- which checks only
// that the string is non-empty -- stopped a `javascript:` URL from being saved
// there and becoming a live script handler for every signed-out visitor.
//
// What this does NOT prove: that the server also rejects one. It should, and
// the check here is the client half. A determined admin can still POST
// directly.

import { describe, expect, it } from 'vitest'
import { ALLOWED_LINK_SCHEMES, isSafeLinkUrl } from '@/lib/urls'

describe('what a curated link may be', () => {
  it('permits the schemes a destination actually uses', () => {
    expect(isSafeLinkUrl('https://wiki.example.org')).toBe(true)
    expect(isSafeLinkUrl('http://10.0.0.9:8080/status')).toBe(true)
    expect(isSafeLinkUrl('mailto:committee@example.org')).toBe(true)
  })

  it('permits a relative link to this site', () => {
    // No scheme to smuggle, and linking to a page of this site is an obvious
    // thing for a curated list to want.
    expect(isSafeLinkUrl('/tools')).toBe(true)
    expect(isSafeLinkUrl('#opening-hours')).toBe(true)
    expect(isSafeLinkUrl('calendar')).toBe(true)
  })

  it('refuses javascript:, in every spelling a browser still honours', () => {
    for (const url of [
      'javascript:alert(1)',
      'JavaScript:alert(1)',
      '  javascript:alert(1)  ',
      'jAvAsCrIpT:fetch("/api/v1/users")',
    ]) {
      expect(isSafeLinkUrl(url), url).toBe(false)
    }
  })

  it('refuses the schemes a denylist forgets', () => {
    // This is why the check is an allowlist. `javascript:` is the one everybody
    // remembers; these execute or exfiltrate just as well, and browsers keep
    // adding more.
    for (const url of [
      'data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==',
      'vbscript:msgbox(1)',
      'blob:https://example.org/9f8a',
      'file:///etc/passwd',
    ]) {
      expect(isSafeLinkUrl(url), url).toBe(false)
    }
  })

  it('refuses a blank or whitespace-only URL', () => {
    expect(isSafeLinkUrl('')).toBe(false)
    expect(isSafeLinkUrl('   ')).toBe(false)
  })

  it('names what it permits, so a reader does not have to infer it', () => {
    expect([...ALLOWED_LINK_SCHEMES].sort()).toEqual(['http:', 'https:', 'mailto:'])
  })
})
