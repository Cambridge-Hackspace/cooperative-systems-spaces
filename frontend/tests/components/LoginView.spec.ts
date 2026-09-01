// Tier 2: LoginView, both steps.
//
// Nothing tested this view, which means nothing tested the only screen where a
// second factor is actually demanded. `MfaSettings.spec.ts` covers enrollment
// -- setting a factor up -- and the contract tier proves `/api/auth/mfa/verify`
// is reachable without a credential. Between those two sits the flow that
// decides whether a password gets you in, and it had no coverage at any tier.
//
// The store is real here rather than stubbed. `@/utils/api` is mocked at
// `apiClient` *and* `mfaApi`, so a test drives the genuine
// login -> challenge -> verify sequence through `stores/auth.ts` and asserts
// what the user sees at each step. That is deliberate: the interesting defects
// in this flow are disagreements between the view and the store about whether
// the session is authenticated yet, and a stubbed store cannot disagree with
// anything.
//
// What this does NOT prove: that a WebAuthn ceremony works. `webauthnGet` is
// mocked, so what is asserted is that the view calls it with the options the
// server sent and does the right thing with each of its two outcomes. The
// ceremony itself -- a real P-256 signature, verified by webauthn-rs -- is
// completed against the real stack in `tests/live/passkey.spec.ts`.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  post: vi.fn(),
  put: vi.fn(),
  delete: vi.fn(),
  verify: vi.fn(),
  webauthnGet: vi.fn(),
  push: vi.fn(),
}))

vi.mock('@/utils/api', () => ({
  apiClient: { get: mocks.get, post: mocks.post, put: mocks.put, delete: mocks.delete },
  mfaApi: { verify: mocks.verify },
}))
vi.mock('@github/webauthn-json', () => ({ get: mocks.webauthnGet }))

const route = { value: { query: {} as Record<string, string> } }
vi.mock('vue-router', () => ({
  useRouter: () => ({ push: mocks.push, currentRoute: route }),
}))

import LoginView from '@/views/LoginView.vue'
import { useConfigStore, type PublicConfig } from '@/stores/config'
import { UserRole, type User } from '@/types'

const USER = {
  id: 'u1',
  username: 'ada',
  email: 'ada@example.invalid',
  full_name: 'Ada Lovelace',
  role: UserRole.Member,
  is_active: true,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
} as User

// The stub renders `to` as an href, so a test can assert where a link goes.
// Without that it renders an anchor pointing nowhere -- which is exactly the
// defect this file now has a regression test for, reproduced in the harness.
const stubs = { 'router-link': { props: ['to'], template: '<a :href="to"><slot /></a>' } }

function challenge(methods: Array<'totp' | 'webauthn' | 'recovery'>, options: unknown = null) {
  return {
    success: true,
    data: {
      mfa_required: true,
      challenge_token: 'chal-abc',
      methods,
      webauthn_options: options,
    },
  }
}

const verified = (over: Record<string, unknown> = {}) => ({
  success: true,
  data: { token: 'jwt-after-mfa', user: USER, expires_in: 86400, ...over },
})

beforeEach(() => {
  setActivePinia(createPinia())
  for (const m of Object.values(mocks)) m.mockReset()
  route.value.query = {}
})

/**
 * Mounting in one place, so `Wrapper` below is the concrete wrapper type
 * rather than `VueWrapper<any, any>`. A bare `ReturnType<typeof mount>` is the
 * unbound generic, which types every helper argument as `any` and quietly
 * switches off the type checking these helpers exist to get.
 */
function mountLogin() {
  return mount(LoginView, { global: { stubs } })
}

type Wrapper = ReturnType<typeof mountLogin>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labeled ${JSON.stringify(label)}`)
  return b
}

/** The tab with this label, or undefined. For asserting one is NOT offered. */
const tabNamed = (w: Wrapper, label: string) =>
  w.findAll('[role="tab"]').find((t) => t.text().trim() === label)

/**
 * The tab with this label, or a failure naming it.
 *
 * Separate from `tabNamed` on purpose. `tabNamed(w, 'Recovery')?.classes()`
 * reads fine and asserts nothing when the tab is absent -- `toContain` on
 * `undefined` is a failure, but one that reports a missing method rather than a
 * missing tab. Anywhere a tab is expected to exist, this is the one to use.
 */
function tab(w: Wrapper, label: string) {
  const found = tabNamed(w, label)
  if (!found) {
    const offered = w.findAll('[role="tab"]').map((t) => t.text().trim())
    throw new Error(`no tab labeled ${JSON.stringify(label)}; offered: ${offered.join(', ')}`)
  }
  return found
}

const errorText = (w: Wrapper) =>
  w.find('.alert-error').exists() ? w.find('.alert-error').text() : ''

/** Sign in with a password and land wherever the server sent us. */
async function signIn(w: Wrapper, password = 'pw') {
  await w.find('#login-username').setValue('ada')
  await w.find('#login-password').setValue(password)
  await w.find('form').trigger('submit')
  await flushPromises()
}

/** Mounted, password submitted, server answered with a challenge. */
async function atChallenge(
  methods: Array<'totp' | 'webauthn' | 'recovery'> = ['totp', 'recovery'],
  options: unknown = null
) {
  mocks.post.mockResolvedValue(challenge(methods, options))
  const w = mountLogin()
  await signIn(w)
  return w
}

describe('the password step', () => {
  it('is what a fresh visitor sees', () => {
    const w = mountLogin()
    expect(w.find('.card-title').text()).toBe('Login')
    expect(w.find('#login-username').exists()).toBe(true)
    expect(w.find('#login-password').exists()).toBe(true)
    expect(w.find('[role="tablist"]').exists()).toBe(false)
  })

  it('labels both fields for a screen reader', () => {
    // The file's own comment says these were unlabeled and were fixed. This is
    // what stops them regressing: the association, not the visible text.
    const w = mountLogin()
    for (const id of ['login-username', 'login-password']) {
      const label = w.find(`label[for="${id}"]`)
      expect(label.exists(), `no label is associated with #${id}`).toBe(true)
      expect(label.text().trim().length).toBeGreaterThan(0)
    }
  })

  it('goes straight in when the password was enough', async () => {
    mocks.post.mockResolvedValue({
      success: true,
      data: { token: 'jwt', user: USER, expires_in: 1 },
    })
    const w = mountLogin()
    await signIn(w)

    expect(mocks.push).toHaveBeenCalledWith('/')
    expect(w.find('[role="tablist"]').exists()).toBe(false)
  })

  it('honors the redirect the guard put in the query', async () => {
    route.value.query = { redirect: '/tools/42' }
    mocks.post.mockResolvedValue({
      success: true,
      data: { token: 'jwt', user: USER, expires_in: 1 },
    })
    const w = mountLogin()
    await signIn(w)

    expect(mocks.push).toHaveBeenCalledWith('/tools/42')
  })

  it('sends a user who must enroll to the MFA settings page instead', async () => {
    mocks.post.mockResolvedValue({
      success: true,
      data: { token: 'jwt', user: USER, expires_in: 1, must_enroll_mfa: true },
    })
    const w = mountLogin()
    await signIn(w)

    expect(mocks.push).toHaveBeenCalledWith('/profile/mfa')
  })

  it("shows the server's refusal and stays put", async () => {
    mocks.post.mockResolvedValue({ success: false, error: 'Invalid credentials' })
    const w = mountLogin()
    await signIn(w, 'wrong')

    expect(errorText(w)).toContain('Invalid credentials')
    expect(mocks.push).not.toHaveBeenCalled()
    expect(w.find('#login-password').exists()).toBe(true)
  })
})

describe('the challenge step', () => {
  it('replaces the password form rather than sitting beside it', async () => {
    // Both on screen at once would let a user re-submit a password while a
    // challenge is outstanding, which starts a second challenge and strands
    // the first token.
    const w = await atChallenge()

    expect(w.find('.card-title').text()).toBe('Two-factor verification')
    expect(w.find('#login-password').exists()).toBe(false)
    expect(w.find('[role="tablist"]').exists()).toBe(true)
    expect(mocks.push, 'a challenge is not an authentication').not.toHaveBeenCalled()
  })

  it('offers exactly the methods the server named', async () => {
    const w = await atChallenge(['totp', 'recovery'])
    expect(tab(w, 'Code').exists()).toBe(true)
    expect(tab(w, 'Recovery').exists()).toBe(true)
    expect(
      tabNamed(w, 'Security key'),
      'a method the account has not enrolled was offered'
    ).toBeUndefined()
  })

  it('offers only the one method when that is all there is', async () => {
    const w = await atChallenge(['totp'])
    expect(w.findAll('[role="tab"]')).toHaveLength(1)
  })

  it('opens on the security key when one is enrolled', async () => {
    // The strongest factor, and the one that needs no typing. Preferring it is
    // the view's own rule; this pins it.
    const w = await atChallenge(['totp', 'webauthn', 'recovery'], { publicKey: {} })
    expect(tab(w, 'Security key').classes()).toContain('tab-active')
  })

  it('opens on the first method offered when there is no security key', async () => {
    const w = await atChallenge(['recovery', 'totp'])
    expect(tab(w, 'Recovery').classes()).toContain('tab-active')
  })

  it('labels the code fields for a screen reader too', async () => {
    // The sibling of the password-field fix, on the other form every enrolled
    // user must complete.
    const w = await atChallenge(['totp', 'recovery'])
    const totpLabel = w.find('label[for="mfa-totp-code"]')
    expect(totpLabel.exists(), 'the TOTP field has no associated label').toBe(true)
    expect(w.find('#mfa-totp-code').exists()).toBe(true)

    await tab(w, 'Recovery').trigger('click')
    expect(
      w.find('label[for="mfa-recovery-code"]').exists(),
      'the recovery field has no associated label'
    ).toBe(true)
    expect(w.find('#mfa-recovery-code').exists()).toBe(true)
  })

  it('lets a user abandon the challenge and start over', async () => {
    const w = await atChallenge()
    await buttonNamed(w, 'Use a different account').trigger('click')
    await flushPromises()

    expect(w.find('#login-password').exists()).toBe(true)
    expect(w.find('[role="tablist"]').exists()).toBe(false)
    expect(mocks.push, 'abandoning a challenge must not sign anyone in').not.toHaveBeenCalled()
  })
})

describe('verifying with an authenticator code', () => {
  it('will not submit fewer than six digits', async () => {
    const w = await atChallenge(['totp'])
    await w.find('#mfa-totp-code').setValue('12345')
    expect(buttonNamed(w, 'Verify').attributes('disabled')).toBeDefined()

    await w.find('#mfa-totp-code').setValue('123456')
    expect(buttonNamed(w, 'Verify').attributes('disabled')).toBeUndefined()
  })

  it('sends the challenge token with the code', async () => {
    // The token is what ties the code to the password step. Sending the code
    // without it -- or with a stale one -- is a 401 the user cannot act on.
    const w = await atChallenge(['totp'])
    mocks.verify.mockResolvedValue(verified())
    await w.find('#mfa-totp-code').setValue(' 123456 ')
    await buttonNamed(w, 'Verify').trigger('click')
    await flushPromises()

    expect(mocks.verify).toHaveBeenCalledWith({
      challenge_token: 'chal-abc',
      method: 'totp',
      code: '123456',
    })
  })

  it('signs the user in and leaves the challenge behind', async () => {
    const w = await atChallenge(['totp'])
    mocks.verify.mockResolvedValue(verified())
    await w.find('#mfa-totp-code').setValue('123456')
    await buttonNamed(w, 'Verify').trigger('click')
    await flushPromises()

    expect(localStorage.getItem('css_token')).toBe('jwt-after-mfa')
    expect(mocks.push).toHaveBeenCalledWith('/')
  })

  it('sends a user who must still enroll to the settings page', async () => {
    const w = await atChallenge(['totp'])
    mocks.verify.mockResolvedValue(verified({ must_enroll_mfa: true }))
    await w.find('#mfa-totp-code').setValue('123456')
    await buttonNamed(w, 'Verify').trigger('click')
    await flushPromises()

    expect(mocks.push).toHaveBeenCalledWith('/profile/mfa')
  })

  it('honors the redirect through the second factor as well', async () => {
    // The query survives the challenge step. Losing it drops the user on the
    // home page after a two-step login, which reads as the deep link failing.
    route.value.query = { redirect: '/admin/roster' }
    const w = await atChallenge(['totp'])
    mocks.verify.mockResolvedValue(verified())
    await w.find('#mfa-totp-code').setValue('123456')
    await buttonNamed(w, 'Verify').trigger('click')
    await flushPromises()

    expect(mocks.push).toHaveBeenCalledWith('/admin/roster')
  })

  it("shows the server's reason for a rejected code and stays on the form", async () => {
    const w = await atChallenge(['totp'])
    mocks.verify.mockResolvedValue({ success: false, error: 'Invalid TOTP code' })
    await w.find('#mfa-totp-code').setValue('000000')
    await buttonNamed(w, 'Verify').trigger('click')
    await flushPromises()

    expect(errorText(w)).toContain('Invalid TOTP code')
    expect(mocks.push).not.toHaveBeenCalled()
    expect(localStorage.getItem('css_token')).toBeNull()
  })

  it('frees the form when the request rejects outright', async () => {
    // Otherwise `mfaBusy` strands and the Verify button is dead for the life of
    // the page -- with the user one code away from being logged in.
    const w = await atChallenge(['totp'])
    mocks.verify.mockRejectedValue(new Error('Network Error'))
    await w.find('#mfa-totp-code').setValue('123456')
    await buttonNamed(w, 'Verify').trigger('click')
    await flushPromises()

    expect(errorText(w)).toContain('Network error')
    expect(buttonNamed(w, 'Verify').attributes('disabled')).toBeUndefined()
    expect(w.find('#mfa-totp-code').attributes('disabled')).toBeUndefined()
  })
})

describe('verifying with a recovery code', () => {
  it('will not submit a blank one', async () => {
    const w = await atChallenge(['recovery'])
    await w.find('#mfa-recovery-code').setValue('   ')
    expect(buttonNamed(w, 'Verify').attributes('disabled')).toBeDefined()
  })

  it('trims what the user typed', async () => {
    const w = await atChallenge(['recovery'])
    mocks.verify.mockResolvedValue(verified())
    await w.find('#mfa-recovery-code').setValue('  ABCD-EFGH-JKLM  ')
    await buttonNamed(w, 'Verify').trigger('click')
    await flushPromises()

    expect(mocks.verify).toHaveBeenCalledWith({
      challenge_token: 'chal-abc',
      method: 'recovery',
      code: 'ABCD-EFGH-JKLM',
    })
  })

  it('reports a spent code rather than pretending it worked', async () => {
    const w = await atChallenge(['recovery'])
    mocks.verify.mockResolvedValue({ success: false, error: 'Invalid recovery code' })
    await w.find('#mfa-recovery-code').setValue('ABCD-EFGH-JKLM')
    await buttonNamed(w, 'Verify').trigger('click')
    await flushPromises()

    expect(errorText(w)).toContain('Invalid recovery code')
    expect(mocks.push).not.toHaveBeenCalled()
  })
})

describe('verifying with a security key', () => {
  const OPTIONS = { publicKey: { challenge: 'abc' } }

  it('runs the ceremony with the options the server sent', async () => {
    // Not options the view invented. The challenge in them is what the server
    // will check the signature against.
    const w = await atChallenge(['webauthn'], OPTIONS)
    mocks.webauthnGet.mockResolvedValue({ id: 'cred-1' })
    mocks.verify.mockResolvedValue(verified())

    await buttonNamed(w, 'Use security key').trigger('click')
    await flushPromises()

    expect(mocks.webauthnGet).toHaveBeenCalledWith(OPTIONS)
    expect(mocks.verify).toHaveBeenCalledWith({
      challenge_token: 'chal-abc',
      method: 'webauthn',
      response: { id: 'cred-1' },
    })
    expect(mocks.push).toHaveBeenCalledWith('/')
  })

  it('reports a ceremony the authenticator refused and frees the button', async () => {
    const w = await atChallenge(['webauthn'], OPTIONS)
    mocks.webauthnGet.mockRejectedValue(
      new Error('The operation either timed out or was not allowed')
    )

    await buttonNamed(w, 'Use security key').trigger('click')
    await flushPromises()

    expect(errorText(w)).toContain('timed out')
    expect(buttonNamed(w, 'Use security key').attributes('disabled')).toBeUndefined()
    expect(mocks.verify, 'a refused ceremony must not be sent to the server').not.toHaveBeenCalled()
  })

  it('does nothing at all when the server sent no options', async () => {
    // `webauthn_options` is null when `start_passkey_authentication` failed
    // server-side. The tab can still be reached, and calling the ceremony with
    // null is a TypeError inside the library rather than a message.
    const w = await atChallenge(['webauthn'], null)

    await buttonNamed(w, 'Use security key').trigger('click')
    await flushPromises()

    expect(mocks.webauthnGet).not.toHaveBeenCalled()
    expect(mocks.verify).not.toHaveBeenCalled()
  })
})

describe('the forgot-password link', () => {
  // The regression test for the dead link.
  //
  // `LoginView.vue` rendered `<a href="#">Forgot password?</a>` from the first
  // release. A member who forgot their password saw the affordance, clicked it,
  // and nothing happened -- and nothing at any tier noticed, because an anchor
  // that goes nowhere is indistinguishable from one that goes somewhere unless
  // a test asks where it goes.

  /** A public config with just enough of the shape to answer this question. */
  function withAuthConfig(auth: PublicConfig['auth']) {
    useConfigStore().config = { auth } as PublicConfig
  }

  it('points at the reset page when recovery is available', async () => {
    withAuthConfig({ password_reset_enabled: true, require_email_verification: false })
    const w = mountLogin()
    await flushPromises()

    const link = w.get('[data-test="forgot-password"]')
    expect(
      link.attributes('href'),
      'the control must navigate to the reset page. An `href="#"` here is the ' +
        'original defect: it looks like an affordance and does nothing.'
    ).toBe('/forgot-password')
  })

  it('is withheld when the deployment cannot send mail', async () => {
    // The server ANDs password_reset_enabled with email.enabled before sending
    // this, so false here means "asking would 403". Offering the link anyway
    // would be the same promise-without-a-product one layer up.
    withAuthConfig({ password_reset_enabled: false, require_email_verification: false })
    const w = mountLogin()
    await flushPromises()

    expect(w.find('[data-test="forgot-password"]').exists()).toBe(false)
  })

  it('is withheld by a server too old to say', async () => {
    // No auth block at all. `undefined` must read as "no", not as "probably":
    // such a server has no reset endpoints either.
    useConfigStore().config = {} as PublicConfig
    const w = mountLogin()
    await flushPromises()

    expect(w.find('[data-test="forgot-password"]').exists()).toBe(false)
  })
})
