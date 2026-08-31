// Tier 2: MfaSettings -- the enrollment surface.
//
// Six actions -- begin TOTP, confirm TOTP, disable TOTP, add a security key,
// remove one, regenerate recovery codes -- against one `busy` flag and one
// `flash` string.
//
// The `busy` flag was this file's headline finding: five of the six handlers
// set it true, awaited, and cleared it on the next line, so a rejection
// stranded the flag and every primary button on the page stayed disabled for
// the life of it. `addWebauthn` alone did the same work inside try/finally,
// which is what made it an oversight rather than a design. **That was fixed in
// ca54bea** and the assertions at the bottom of this file now pin the fix
// rather than the defect -- see the note there.
//
// WHAT THIS FILE DOES NOT COVER, and where it is covered instead. This is
// enrollment only: setting a factor up while already signed in. The half that
// matters more -- whether a second factor actually gates the JWT -- is not here
// and cannot be, because this component never sees a login.
//
//   * `tests/unit/auth-store-login.spec.ts` -- the store's challenge branch.
//   * `tests/components/LoginView.spec.ts`  -- the challenge form.
//   * `tests/e2e/mfa-login.spec.ts`         -- a real browser, real reload.
//   * `e2e/drivers/mfa.mjs`                 -- a real HMAC, real database.
//
// What none of THOSE prove: that a WebAuthn ceremony works. The browser API is
// mocked out here and in the fake-API tier alike, so what is asserted is that
// the component calls it with the options the server sent and does the right
// thing with each outcome. The ceremonies themselves are completed in
// `tests/live/passkey.spec.ts`, which attaches Chromium's virtual authenticator
// over CDP and runs them against the real stack.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({
  status: vi.fn(),
  listWebauthn: vi.fn(),
  totpSetup: vi.fn(),
  totpConfirm: vi.fn(),
  totpDisable: vi.fn(),
  webauthnRegisterBegin: vi.fn(),
  webauthnRegisterFinish: vi.fn(),
  webauthnRemove: vi.fn(),
  regenerateRecoveryCodes: vi.fn(),
  toDataURL: vi.fn(),
  webauthnCreate: vi.fn(),
}))

vi.mock('@/utils/api', () => ({ mfaApi: mocks }))
vi.mock('qrcode', () => ({ default: { toDataURL: mocks.toDataURL } }))
vi.mock('@github/webauthn-json', () => ({ create: mocks.webauthnCreate }))

import MfaSettings from '@/components/MfaSettings.vue'
import type { MfaStatus, MfaWebauthnCredential } from '@/types'

function status(over: Partial<MfaStatus> = {}): MfaStatus {
  return {
    enabled: true,
    totp_enrolled: false,
    webauthn_count: 0,
    recovery_codes_remaining: 0,
    must_enroll: false,
    ...over,
  }
}

const CRED: MfaWebauthnCredential = {
  id: 'c1',
  label: 'Yubikey 5C',
  created_at: '2025-11-02T09:30:00Z',
  last_used_at: null,
}

// `router-link` resolves through the router in the real app. Stubbed as an
// anchor rather than left unresolved, because `tests/setup.ts` turns the
// resulting Vue warning into a failure -- correctly.
const stubs = { 'router-link': { props: ['to'], template: '<a><slot /></a>' } }

let confirmResult = true

beforeEach(() => {
  for (const m of Object.values(mocks)) m.mockReset()
  mocks.status.mockResolvedValue({ success: true, data: status() })
  mocks.listWebauthn.mockResolvedValue({ success: true, data: [] })
  mocks.toDataURL.mockResolvedValue('data:image/png;base64,QR')
  confirmResult = true
  // jsdom's `window.confirm` is a stub that logs "Not implemented" and returns
  // undefined, which would silently take the *cancel* branch of every guarded
  // action and make those tests assert nothing.
  vi.stubGlobal(
    'confirm',
    vi.fn(() => confirmResult)
  )
})

async function settings(over: Partial<MfaStatus> = {}) {
  mocks.status.mockResolvedValue({ success: true, data: status(over) })
  const w = mount(MfaSettings, { global: { stubs } })
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof settings>>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labeled ${JSON.stringify(label)}`)
  return b
}

const flashText = (w: Wrapper) => w.find('.alert-success, .alert-error').text()

describe('before the status arrives', () => {
  it('shows a spinner and none of the sections', () => {
    mocks.status.mockReturnValue(new Promise(() => {}))
    mocks.listWebauthn.mockReturnValue(new Promise(() => {}))
    const w = mount(MfaSettings, { global: { stubs } })

    expect(w.find('.loading-spinner').exists()).toBe(true)
    expect(w.text()).not.toContain('Authenticator app')
  })

  // FIXED. `loadAll` wrote `status` only on success and the whole page is
  // gated on `v-if="!status"`, so a refused status left it a spinner forever
  // -- on the page whose job is letting somebody secure their account.
  it('reports a refused status instead of spinning forever', async () => {
    mocks.status.mockResolvedValue({ success: false, error: 'Forbidden' })
    const w = mount(MfaSettings, { global: { stubs } })
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('Forbidden')
  })

  it('reports a rejected status rather than letting it escape the app', async () => {
    const escaped: unknown[] = []
    mocks.status.mockRejectedValue(new Error('Network Error'))
    const w = mount(MfaSettings, {
      global: { stubs, config: { errorHandler: (e: unknown) => escaped.push(e) } },
    })
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('Network Error')
    expect(escaped, 'the rejection is handled now, not routed to the app').toHaveLength(0)
  })
})

describe('what the server configuration allows', () => {
  it('warns when MFA is switched off server-side and disables every enrolling action', async () => {
    const w = await settings({ enabled: false })

    expect(w.find('.alert-warning').text()).toContain('MFA is disabled in server configuration')
    expect(buttonNamed(w, 'Set up authenticator').attributes('disabled')).toBeDefined()
    expect(buttonNamed(w, 'Add security key').attributes('disabled')).toBeDefined()
    expect(buttonNamed(w, 'Generate recovery codes').attributes('disabled')).toBeDefined()
  })

  it('says nothing about configuration when MFA is on', async () => {
    expect((await settings()).find('.alert-warning').exists()).toBe(false)
  })

  it('tells a user whose role requires MFA that it does', async () => {
    expect((await settings({ must_enroll: true })).find('.alert-info').text()).toContain(
      'Your role requires MFA'
    )
  })

  it('names the recovery-code button for what it will do', async () => {
    expect((await settings({ recovery_codes_remaining: 0 })).text()).toContain(
      'Generate recovery codes'
    )
    expect((await settings({ recovery_codes_remaining: 3 })).text()).toContain(
      'Regenerate recovery codes'
    )
  })
})

describe('the fields a person types into', () => {
  it('associates both inputs with their labels', async () => {
    // The same defect as the two on the login form, in the same feature: a
    // `<label>` with no `for` whose input is a sibling is associated with
    // nothing, so a screen reader announces an unlabeled text field and
    // clicking the label focuses nothing.
    //
    // This is also what lets the live passkey tier address the key-label field
    // by `getByLabel`, which is the same thing assistive technology does -- so
    // the selector there is the accessibility assertion rather than a separate
    // one somebody has to remember to write.
    // Both fields have to be on screen at once, and the confirm field only
    // exists once a setup is in progress -- so the setup call is arranged
    // rather than left at its reset default, which resolves undefined and
    // renders nothing.
    mocks.totpSetup.mockResolvedValue({
      success: true,
      data: { secret_base32: 'JBSWY3DPEHPK3PXP', otpauth_uri: 'otpauth://totp/css:me' },
    })
    const w = await settings({ totp_enrolled: false })
    await buttonNamed(w, 'Set up authenticator').trigger('click')
    await flushPromises()

    for (const id of ['mfa-totp-confirm', 'mfa-key-label']) {
      const label = w.find(`label[for="${id}"]`)
      expect(label.exists(), `no label is associated with #${id}`).toBe(true)
      expect(label.text().trim().length).toBeGreaterThan(0)
      expect(w.find(`#${id}`).exists(), `#${id} does not exist`).toBe(true)
    }
  })
})

describe('setting up an authenticator', () => {
  const SETUP = { secret_base32: 'JBSWY3DPEHPK3PXP', otpauth_uri: 'otpauth://totp/css:me' }

  it('shows the secret and a QR rendered from the otpauth URI', async () => {
    mocks.totpSetup.mockResolvedValue({ success: true, data: SETUP })
    const w = await settings()
    await buttonNamed(w, 'Set up authenticator').trigger('click')
    await flushPromises()

    expect(mocks.toDataURL).toHaveBeenCalledWith('otpauth://totp/css:me', {
      margin: 1,
      width: 220,
    })
    expect(w.find('img').attributes('src')).toBe('data:image/png;base64,QR')
    expect(w.text()).toContain('JBSWY3DPEHPK3PXP')
  })

  it('reports a QR rendering failure rather than showing a broken image', async () => {
    mocks.totpSetup.mockResolvedValue({ success: true, data: SETUP })
    mocks.toDataURL.mockRejectedValue(new Error('canvas unavailable'))
    const w = await settings()
    await buttonNamed(w, 'Set up authenticator').trigger('click')
    await flushPromises()

    expect(flashText(w)).toContain('Failed to render QR: canvas unavailable')
    expect(w.find('img').exists()).toBe(false)
    // The secret is still there, so the manual-entry path survives a QR
    // failure. That is the point of showing both.
    expect(w.text()).toContain('JBSWY3DPEHPK3PXP')
  })

  it('will not submit a code shorter than six characters', async () => {
    mocks.totpSetup.mockResolvedValue({ success: true, data: SETUP })
    const w = await settings()
    await buttonNamed(w, 'Set up authenticator').trigger('click')
    await flushPromises()

    expect(buttonNamed(w, 'Confirm').attributes('disabled')).toBeDefined()
    await w.find('input[inputmode="numeric"]').setValue('12345')
    expect(buttonNamed(w, 'Confirm').attributes('disabled')).toBeDefined()
    await w.find('input[inputmode="numeric"]').setValue('123456')
    expect(buttonNamed(w, 'Confirm').attributes('disabled')).toBeUndefined()
  })

  it('shows the recovery codes exactly once, on a successful confirmation', async () => {
    mocks.totpSetup.mockResolvedValue({ success: true, data: SETUP })
    mocks.totpConfirm.mockResolvedValue({
      success: true,
      data: { recovery_codes: ['aaaa-1111', 'bbbb-2222'] },
    })
    const w = await settings()
    await buttonNamed(w, 'Set up authenticator').trigger('click')
    await flushPromises()
    await w.find('input[inputmode="numeric"]').setValue(' 123456 ')
    await buttonNamed(w, 'Confirm').trigger('click')
    await flushPromises()

    // Trimmed on the way out, so a pasted code with surrounding whitespace is
    // not rejected by the server for a reason the user cannot see.
    expect(mocks.totpConfirm).toHaveBeenCalledWith('123456')
    expect(w.text()).toContain('aaaa-1111')
    expect(w.text()).toContain('They will not be shown again')
    expect(flashText(w)).toContain('TOTP enabled')
    // The status is re-read, so the badge and the code count reflect the change.
    expect(mocks.status).toHaveBeenCalledTimes(2)
  })

  it("shows the server's reason when the code is rejected, and stays in setup", async () => {
    mocks.totpSetup.mockResolvedValue({ success: true, data: SETUP })
    mocks.totpConfirm.mockResolvedValue({ success: false, error: 'Code already used' })
    const w = await settings()
    await buttonNamed(w, 'Set up authenticator').trigger('click')
    await flushPromises()
    await w.find('input[inputmode="numeric"]').setValue('123456')
    await buttonNamed(w, 'Confirm').trigger('click')
    await flushPromises()

    expect(flashText(w)).toContain('Code already used')
    expect(w.text()).toContain('JBSWY3DPEHPK3PXP')
  })

  it('abandons the setup on Cancel', async () => {
    mocks.totpSetup.mockResolvedValue({ success: true, data: SETUP })
    const w = await settings()
    await buttonNamed(w, 'Set up authenticator').trigger('click')
    await flushPromises()
    await buttonNamed(w, 'Cancel').trigger('click')
    await flushPromises()

    expect(w.text()).not.toContain('JBSWY3DPEHPK3PXP')
    expect(w.find('img').exists()).toBe(false)
  })

  it('offers to disable, not to set up, once TOTP is enrolled', async () => {
    const w = await settings({ totp_enrolled: true })
    expect(w.find('.badge-success').text()).toBe('Enrolled')
    expect(w.findAll('button').map((b) => b.text().trim())).not.toContain('Set up authenticator')
  })

  it('asks before disabling, and does nothing if the answer is no', async () => {
    confirmResult = false
    const w = await settings({ totp_enrolled: true })
    await buttonNamed(w, 'Disable').trigger('click')
    await flushPromises()

    expect(mocks.totpDisable).not.toHaveBeenCalled()
  })
})

describe('security keys', () => {
  it('lists each credential, and marks one that has never been used', async () => {
    mocks.listWebauthn.mockResolvedValue({
      success: true,
      data: [CRED, { ...CRED, id: 'c2', label: 'Touch ID', last_used_at: '2026-01-10T08:00:00Z' }],
    })
    const w = await settings()
    const rows = w.findAll('tbody tr')

    expect(rows).toHaveLength(2)
    expect(rows[0].text()).toContain('Yubikey 5C')
    // Computed the same way the component does, so the assertion survives a
    // machine in another timezone while still failing if the wrong field is
    // rendered.
    expect(rows[0].findAll('td')[1].text()).toBe(new Date(CRED.created_at).toLocaleString())
    expect(rows[0].findAll('td')[2].text()).toBe('—')
    expect(rows[1].findAll('td')[2].text()).toBe(new Date('2026-01-10T08:00:00Z').toLocaleString())
  })

  it('shows no table at all when there are no keys', async () => {
    expect((await settings()).find('tbody').exists()).toBe(false)
  })

  it('requires a label before a key can be added', async () => {
    const w = await settings()
    expect(buttonNamed(w, 'Add security key').attributes('disabled')).toBeDefined()
    await w.find('input[type="text"]').setValue('   ')
    expect(buttonNamed(w, 'Add security key').attributes('disabled')).toBeDefined()
    await w.find('input[type="text"]').setValue('Yubikey 5C')
    expect(buttonNamed(w, 'Add security key').attributes('disabled')).toBeUndefined()
  })

  it('runs the registration ceremony and hands the challenge token back', async () => {
    mocks.webauthnRegisterBegin.mockResolvedValue({
      success: true,
      data: { challenge_token: 'tok-1', options: { challenge: 'abc' } },
    })
    mocks.webauthnCreate.mockResolvedValue({ id: 'new-cred' })
    mocks.webauthnRegisterFinish.mockResolvedValue({ success: true })

    const w = await settings()
    await w.find('input[type="text"]').setValue('  Yubikey 5C  ')
    await buttonNamed(w, 'Add security key').trigger('click')
    await flushPromises()

    expect(mocks.webauthnRegisterBegin).toHaveBeenCalledWith('Yubikey 5C')
    expect(mocks.webauthnCreate).toHaveBeenCalledWith({ challenge: 'abc' })
    expect(mocks.webauthnRegisterFinish).toHaveBeenCalledWith('tok-1', { id: 'new-cred' })
    expect(flashText(w)).toContain('Security key added')
    expect((w.find('input[type="text"]').element as HTMLInputElement).value).toBe('')
  })

  it('reports a ceremony the authenticator refused, and frees the page again', async () => {
    mocks.webauthnRegisterBegin.mockResolvedValue({
      success: true,
      data: { challenge_token: 'tok-1', options: {} },
    })
    mocks.webauthnCreate.mockRejectedValue(new Error('The operation was not allowed'))

    const w = await settings()
    await w.find('input[type="text"]').setValue('Yubikey 5C')
    await buttonNamed(w, 'Add security key').trigger('click')
    await flushPromises()

    expect(flashText(w)).toContain('The operation was not allowed')
    // `addWebauthn` is the one handler with a `finally`, so the page is usable
    // afterwards. The three without one are the finding below.
    expect(buttonNamed(w, 'Add security key').attributes('disabled')).toBeUndefined()
    expect(buttonNamed(w, 'Set up authenticator').attributes('disabled')).toBeUndefined()
  })

  it('asks before removing a key, and does nothing if the answer is no', async () => {
    mocks.listWebauthn.mockResolvedValue({ success: true, data: [CRED] })
    confirmResult = false
    const w = await settings()
    await buttonNamed(w, 'Remove').trigger('click')
    await flushPromises()

    expect(mocks.webauthnRemove).not.toHaveBeenCalled()
  })

  it('removes a key and re-reads the list', async () => {
    mocks.listWebauthn.mockResolvedValue({ success: true, data: [CRED] })
    mocks.webauthnRemove.mockResolvedValue({ success: true })
    const w = await settings()
    await buttonNamed(w, 'Remove').trigger('click')
    await flushPromises()

    expect(mocks.webauthnRemove).toHaveBeenCalledWith('c1')
    expect(mocks.listWebauthn).toHaveBeenCalledTimes(2)
  })
})

describe('recovery codes', () => {
  it('asks before invalidating the existing set', async () => {
    confirmResult = false
    const w = await settings({ recovery_codes_remaining: 5 })
    await buttonNamed(w, 'Regenerate recovery codes').trigger('click')
    await flushPromises()

    expect(mocks.regenerateRecoveryCodes).not.toHaveBeenCalled()
  })

  it('shows a fresh set with the warning that it will not be shown again', async () => {
    mocks.regenerateRecoveryCodes.mockResolvedValue({
      success: true,
      data: { recovery_codes: ['zzzz-9999'] },
    })
    const w = await settings({ recovery_codes_remaining: 5 })
    await buttonNamed(w, 'Regenerate recovery codes').trigger('click')
    await flushPromises()

    expect(w.text()).toContain('zzzz-9999')
    expect(w.find('.alert-warning').text()).toContain('Save these somewhere safe')
  })
})

describe('the flash message', () => {
  it('clears itself after five seconds', async () => {
    mocks.totpDisable.mockResolvedValue({ success: true })
    const w = await settings({ totp_enrolled: true })
    await buttonNamed(w, 'Disable').trigger('click')
    await flushPromises()
    expect(flashText(w)).toContain('TOTP disabled')

    vi.advanceTimersByTime(5000)
    await nextTick()
    expect(w.find('.alert-success').exists()).toBe(false)
  })

  it('can be dismissed by hand', async () => {
    mocks.totpDisable.mockResolvedValue({ success: true })
    const w = await settings({ totp_enrolled: true })
    await buttonNamed(w, 'Disable').trigger('click')
    await flushPromises()

    await buttonNamed(w, '✕').trigger('click')
    expect(w.find('.alert-success').exists()).toBe(false)
  })
})

describe('what a network error does to the page', () => {
  // These two were the "locking" cases: before ca54bea, a rejection in either
  // handler stranded `busy` and disabled every primary button on the page. They
  // now assert the recovery, like the Confirm case below -- the name is kept
  // only because it says what the arrangement is for.
  const locking: [string, () => void][] = [
    ['Set up authenticator', () => mocks.totpSetup.mockRejectedValue(new Error('Network Error'))],
    [
      'Regenerate recovery codes',
      () => mocks.regenerateRecoveryCodes.mockRejectedValue(new Error('Network Error')),
    ],
  ]

  // FIXED. `beginTotp`, `confirmTotp` and `regenRecovery` each set `busy = true`
  // and cleared it on the next line, so a rejection in any of them stranded the
  // flag and every primary button on the page stayed disabled for the life of
  // it -- with nothing on screen to say why. `addWebauthn` already did the same
  // work inside try/finally, which is what made it an oversight rather than a
  // design.
  it('frees the page and reports the failure when Confirm rejects', async () => {
    mocks.totpSetup.mockResolvedValue({
      success: true,
      data: { secret_base32: 'JBSWY3DPEHPK3PXP', otpauth_uri: 'otpauth://totp/css:me' },
    })
    mocks.totpConfirm.mockRejectedValue(new Error('Network Error'))
    const w = mount(MfaSettings, { global: { stubs } })
    await flushPromises()

    await buttonNamed(w, 'Set up authenticator').trigger('click')
    await flushPromises()
    await w.find('input[inputmode="numeric"]').setValue('123456')
    await buttonNamed(w, 'Confirm').trigger('click')
    await flushPromises()

    expect(buttonNamed(w, 'Confirm').attributes('disabled')).toBeUndefined()
    expect(flashText(w)).toContain('Network Error')
  })

  for (const [label, arrange] of locking) {
    it(`frees the page and reports the failure when ${label} rejects`, async () => {
      arrange()
      mocks.status.mockResolvedValue({
        success: true,
        data: status({ recovery_codes_remaining: 5 }),
      })
      const w = mount(MfaSettings, { global: { stubs } })
      await flushPromises()

      await buttonNamed(w, label).trigger('click')
      await flushPromises()

      expect(buttonNamed(w, 'Set up authenticator').attributes('disabled')).toBeUndefined()
      expect(buttonNamed(w, 'Add security key').attributes('disabled')).toBeDefined() // no label typed
      expect(buttonNamed(w, 'Regenerate recovery codes').attributes('disabled')).toBeUndefined()
      expect(flashText(w)).toContain('Network Error')
    })
  }
})
