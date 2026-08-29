// Tier 2: WebhookManagement.
//
// Three tabs over four endpoints. This one is better written than most of its
// neighbours -- it clears `saving` before its early returns, it acknowledges
// its own silent-failure gap in a comment, and it uses `void` deliberately
// rather than by accident -- so several assertions below are about behaviour
// worth keeping rather than defects.
//
// The finding that matters is not in the component's logic. The webhook URL is
// free text, and `api/webhooks.rs:166` validates only that it starts with
// `http://` or `https://`. There is no host allowlist and no private-address
// block. The server then fetches that URL and stores the result:
// `WebhookDelivery.response_body` is returned to the admin UI verbatim.
//
// So an admin can point a webhook at `http://169.254.169.254/...` or at any
// service on the server's own network, fire a test delivery, and read the
// response body back out of the delivery history. That turns "an admin may
// configure notifications" into "an admin may read any URL the server can
// reach". It is admin-gated and webhooks legitimately point at arbitrary
// external URLs, so this is a design worth deciding on deliberately rather
// than an outright bug -- but it should be decided, not inherited.
//
// What this spec does NOT prove: that the server fetches the URL, or what it
// stores. Those are asserted from the type and the handler and belong to Tier
// 6. What is asserted here is that this component sends such a URL unchallenged.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({
  listWebhooks: vi.fn(),
  listAuthHeaders: vi.fn(),
  listEventTypes: vi.fn(),
  listDeliveries: vi.fn(),
  createWebhook: vi.fn(),
  updateWebhook: vi.fn(),
  deleteWebhook: vi.fn(),
  testWebhook: vi.fn(),
  createAuthHeader: vi.fn(),
  updateAuthHeader: vi.fn(),
  deleteAuthHeader: vi.fn(),
}))
vi.mock('@/utils/api', () => ({ webhooksApi: mocks }))

import WebhookManagement from '@/components/WebhookManagement.vue'
import type { Webhook, WebhookAuthHeader, WebhookDelivery, WebhookEventType } from '@/types'

const EVENT_TYPES: WebhookEventType[] = [
  { value: 'user_login', label: 'User Login' },
  { value: 'door_unlocked_card', label: 'Door Unlocked (card)' },
]

function webhook(over: Partial<Webhook> = {}): Webhook {
  return {
    id: 'wh-1',
    name: 'Matrix bridge',
    url: 'https://example.org/hook',
    enabled: true,
    signing_secret: 'sec',
    event_types: ['user_login'],
    auth_header_ids: [],
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...over,
  }
}

function authHeader(over: Partial<WebhookAuthHeader> = {}): WebhookAuthHeader {
  return {
    id: 'ah-1',
    name: 'Matrix token',
    header_name: 'Authorization',
    has_value: true,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...over,
  }
}

function delivery(over: Partial<WebhookDelivery> = {}): WebhookDelivery {
  return {
    id: 'd-1',
    webhook_id: 'wh-1',
    audit_log_id: null,
    event_type: 'user_login',
    attempt: 1,
    success: true,
    status_code: 200,
    response_body: 'ok',
    error: null,
    request_payload: {},
    created_at: '2026-01-14T09:30:00Z',
    ...over,
  }
}

const stubs = { 'router-link': { props: ['to'], template: '<a><slot /></a>' } }

let confirmResult = true

beforeEach(() => {
  for (const m of Object.values(mocks)) m.mockReset()
  mocks.listWebhooks.mockResolvedValue({ success: true, data: [] })
  mocks.listAuthHeaders.mockResolvedValue({ success: true, data: [] })
  mocks.listEventTypes.mockResolvedValue({ success: true, data: EVENT_TYPES })
  mocks.listDeliveries.mockResolvedValue({ success: true, data: [] })
  mocks.createWebhook.mockResolvedValue({ success: true })
  mocks.updateWebhook.mockResolvedValue({ success: true })
  mocks.deleteWebhook.mockResolvedValue({ success: true })
  mocks.testWebhook.mockResolvedValue({ success: true, data: { delivered: true } })
  mocks.createAuthHeader.mockResolvedValue({ success: true })
  mocks.updateAuthHeader.mockResolvedValue({ success: true })
  mocks.deleteAuthHeader.mockResolvedValue({ success: true })
  confirmResult = true
  vi.stubGlobal(
    'confirm',
    vi.fn(() => confirmResult)
  )
  vi.spyOn(console, 'error').mockImplementation(() => {})
})

async function page(over: { webhooks?: Webhook[]; auth?: WebhookAuthHeader[] } = {}) {
  if (over.webhooks) mocks.listWebhooks.mockResolvedValue({ success: true, data: over.webhooks })
  if (over.auth) mocks.listAuthHeaders.mockResolvedValue({ success: true, data: over.auth })
  const w = mount(WebhookManagement, { global: { stubs } })
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof page>>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}

function tabNamed(w: Wrapper, name: string) {
  const t = w.findAll('[role="tab"]').find((x) => x.text().trim().startsWith(name))
  if (!t) throw new Error(`no tab starting ${JSON.stringify(name)}`)
  return t
}

const modalInputs = (w: Wrapper) => w.findAll('.modal-box input[type="text"]')

// The secret is a password field, not a text one -- which is itself worth
// asserting, so it is looked up by that type rather than by position.
const secretInput = (w: Wrapper) => {
  const el = w.find('.modal-box input[type="password"]')
  if (!el.exists()) throw new Error('the credential modal has no password input')
  return el
}

async function openNewWebhook(w: Wrapper) {
  await buttonNamed(w, '+ New Webhook').trigger('click')
  await nextTick()
}

describe('the three tabs', () => {
  it('loads webhooks, credentials and event types on open, and no deliveries', async () => {
    await page()
    expect(mocks.listWebhooks).toHaveBeenCalledTimes(1)
    expect(mocks.listAuthHeaders).toHaveBeenCalledTimes(1)
    expect(mocks.listEventTypes).toHaveBeenCalledTimes(1)
    expect(mocks.listDeliveries).not.toHaveBeenCalled()
  })

  it('fetches deliveries only when that tab is opened', async () => {
    const w = await page()
    await tabNamed(w, 'Deliveries').trigger('click')
    await flushPromises()
    expect(mocks.listDeliveries).toHaveBeenCalledWith({ limit: 100 })
  })

  it('re-fetches deliveries on demand', async () => {
    const w = await page()
    await tabNamed(w, 'Deliveries').trigger('click')
    await flushPromises()
    await buttonNamed(w, 'Refresh').trigger('click')
    await flushPromises()
    expect(mocks.listDeliveries).toHaveBeenCalledTimes(2)
  })

  // Pinned, and the component says so itself in a comment: `loadDeliveries`
  // logs a failure and leaves the tab showing an empty list, which reads as
  // "no deliveries yet". The comment is honest that the log "is not a
  // substitute" for an error surface. This test holds it to that.
  it('shows a failed delivery load as an empty history', async () => {
    mocks.listDeliveries.mockResolvedValue({ success: false, error: 'Forbidden' })
    const w = await page()
    await tabNamed(w, 'Deliveries').trigger('click')
    await flushPromises()

    expect(
      w.text(),
      'the deliveries tab now reports a failed load -- if an error surface was ' +
        'added, this test should assert it instead'
    ).not.toContain('Forbidden')
    expect(w.findAll('tbody tr')).toHaveLength(0)
  })
})

describe('the webhook list', () => {
  it('says there are none', async () => {
    expect((await page()).text()).toContain('No webhooks')
  })

  it('lists each webhook with its name and URL', async () => {
    const w = await page({ webhooks: [webhook()] })
    expect(w.find('tbody tr').text()).toContain('Matrix bridge')
    expect(w.find('tbody tr').text()).toContain('https://example.org/hook')
  })

  it('names the webhook a delivery belongs to', async () => {
    mocks.listDeliveries.mockResolvedValue({ success: true, data: [delivery()] })
    const w = await page({ webhooks: [webhook({ id: 'wh-1', name: 'Matrix bridge' })] })
    await tabNamed(w, 'Deliveries').trigger('click')
    await flushPromises()
    expect(w.find('tbody tr').text()).toContain('Matrix bridge')
  })

  // Recorded: a delivery whose webhook has since been deleted falls back to a
  // truncated id. Deleting a webhook is documented as removing its delivery
  // history, so this should be unreachable -- which makes the fallback a
  // reasonable belt-and-braces rather than a defect, and worth pinning so it
  // stays that shape.
  it('falls back to a truncated id for a delivery with no matching webhook', async () => {
    mocks.listDeliveries.mockResolvedValue({
      success: true,
      data: [delivery({ webhook_id: 'deadbeef-1111-2222-3333-444444444444' })],
    })
    const w = await page({ webhooks: [] })
    await tabNamed(w, 'Deliveries').trigger('click')
    await flushPromises()
    expect(w.find('tbody tr').text()).toContain('deadbeef')
  })
})

describe('creating and editing a webhook', () => {
  it('requires a name and a URL, and says which', async () => {
    const w = await page()
    await openNewWebhook(w)
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('Name and URL are required')
    expect(mocks.createWebhook).not.toHaveBeenCalled()
  })

  it('offers every event type the server advertises, with select-all and clear', async () => {
    const w = await page()
    await openNewWebhook(w)
    expect(w.findAll('.modal-box input[type="checkbox"].checkbox')).toHaveLength(EVENT_TYPES.length)

    await w
      .findAll('.modal-box a')
      .find((a) => a.text() === 'Select all')
      ?.trigger('click')
    await nextTick()
    await modalInputs(w)[0].setValue('Matrix bridge')
    await modalInputs(w)[1].setValue('https://example.org/hook')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect((mocks.createWebhook.mock.calls[0][0] as { event_types: string[] }).event_types).toEqual(
      EVENT_TYPES.map((e) => e.value)
    )
  })

  it('loads an existing webhook into the form and updates rather than creates', async () => {
    const w = await page({ webhooks: [webhook({ id: 'wh-7', name: 'Matrix bridge' })] })
    await buttonNamed(w, 'Edit').trigger('click')
    await nextTick()

    expect((modalInputs(w)[0].element as HTMLInputElement).value).toBe('Matrix bridge')
    await buttonNamed(w, 'Save').trigger('click')
    await flushPromises()

    expect(mocks.updateWebhook.mock.calls[0][0]).toBe('wh-7')
    expect(mocks.createWebhook).not.toHaveBeenCalled()
  })

  it('reloads and reports success', async () => {
    const w = await page()
    await openNewWebhook(w)
    await modalInputs(w)[0].setValue('Matrix bridge')
    await modalInputs(w)[1].setValue('https://example.org/hook')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(w.find('.alert-success').text()).toContain('Webhook created')
    expect(mocks.listWebhooks).toHaveBeenCalledTimes(2)
    expect(w.find('.modal-open').exists()).toBe(false)
  })

  // FINDING, pinned. The URL is free text here, and `api/webhooks.rs:166`
  // checks only the scheme. No allowlist, no private-address block -- and the
  // server stores what comes back: `WebhookDelivery.response_body` is returned
  // to this very UI. Firing a test delivery at a loopback or link-local address
  // and reading the body out of the Deliveries tab is a read primitive against
  // anything the server can reach.
  //
  // Admin-gated, and webhooks legitimately point at arbitrary external URLs, so
  // this is a design to decide on rather than an outright bug. It should be
  // decided.
  it('sends a loopback and a link-local URL unchallenged', async () => {
    for (const url of ['http://127.0.0.1:5432/', 'http://169.254.169.254/latest/meta-data/']) {
      mocks.createWebhook.mockClear()
      const w = await page()
      await openNewWebhook(w)
      await modalInputs(w)[0].setValue('probe')
      await modalInputs(w)[1].setValue(url)
      await buttonNamed(w, 'Create').trigger('click')
      await flushPromises()

      expect(
        (mocks.createWebhook.mock.calls[0][0] as { url: string }).url,
        'the URL is now validated -- if a host check was added, delete this ' +
          'test and assert the rejection instead'
      ).toBe(url)
      expect(w.find('.alert-error').exists()).toBe(false)
    }
  })
})

describe('testing a webhook', () => {
  it('reports a delivered test by name', async () => {
    const w = await page({ webhooks: [webhook({ name: 'Matrix bridge' })] })
    await buttonNamed(w, 'Test').trigger('click')
    await flushPromises()

    expect(mocks.testWebhook).toHaveBeenCalledWith('wh-1')
    expect(w.find('.alert-success').text()).toContain('Test delivered to "Matrix bridge"')
  })

  it('reports a test that reached the server but was not delivered', async () => {
    mocks.testWebhook.mockResolvedValue({
      success: true,
      data: { delivered: false, error: 'connection refused' },
    })
    const w = await page({ webhooks: [webhook()] })
    await buttonNamed(w, 'Test').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('connection refused')
  })

  it('shows a spinner on the row being tested, and clears it', async () => {
    mocks.testWebhook.mockReturnValue(new Promise(() => {}))
    const w = await page({ webhooks: [webhook()] })
    await buttonNamed(w, 'Test').trigger('click')
    await nextTick()
    expect(w.find('tbody .loading').exists()).toBe(true)
  })
})

describe('deleting a webhook', () => {
  it('warns that the delivery history goes too', async () => {
    confirmResult = false
    const w = await page({ webhooks: [webhook({ name: 'Matrix bridge' })] })
    await buttonNamed(w, 'Delete').trigger('click')
    await flushPromises()

    expect(vi.mocked(globalThis.confirm)).toHaveBeenCalledWith(
      'Delete webhook "Matrix bridge"? This also removes its delivery history.'
    )
    expect(mocks.deleteWebhook).not.toHaveBeenCalled()
  })

  it('deletes and reloads', async () => {
    const w = await page({ webhooks: [webhook({ id: 'wh-3' })] })
    await buttonNamed(w, 'Delete').trigger('click')
    await flushPromises()

    expect(mocks.deleteWebhook).toHaveBeenCalledWith('wh-3')
    expect(mocks.listWebhooks).toHaveBeenCalledTimes(2)
  })
})

describe('credentials', () => {
  const openAuthTab = async (w: Wrapper) => {
    await tabNamed(w, 'Auth').trigger('click')
    await nextTick()
  }

  it('requires a value when creating, but not when editing', async () => {
    const w = await page({ auth: [authHeader()] })
    await openAuthTab(w)

    await buttonNamed(w, '+ New Credential').trigger('click')
    await nextTick()
    await modalInputs(w)[0].setValue('Matrix token')
    await modalInputs(w)[1].setValue('Authorization')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('A header value is required')
    expect(mocks.createAuthHeader).not.toHaveBeenCalled()
  })

  // Behaviour worth keeping: editing opens with the secret box empty and omits
  // it from the payload unless retyped, so saving a rename does not blank the
  // credential. The secret is never sent back to the browser either --
  // `WebhookAuthHeader` carries `has_value`, not the value.
  it('keeps the stored secret when an edit does not retype it', async () => {
    const w = await page({ auth: [authHeader({ id: 'ah-9', name: 'Matrix token' })] })
    await openAuthTab(w)
    await buttonNamed(w, 'Edit').trigger('click')
    await nextTick()

    expect((secretInput(w).element as HTMLInputElement).value).toBe('')
    await modalInputs(w)[0].setValue('Matrix token (rotated)')
    await buttonNamed(w, 'Save').trigger('click')
    await flushPromises()

    const payload = mocks.updateAuthHeader.mock.calls[0][1] as Record<string, unknown>
    expect(mocks.updateAuthHeader.mock.calls[0][0]).toBe('ah-9')
    expect(payload).not.toHaveProperty('header_value')
    expect(payload.name).toBe('Matrix token (rotated)')
  })

  it('sends the new secret when an edit does retype it', async () => {
    const w = await page({ auth: [authHeader()] })
    await openAuthTab(w)
    await buttonNamed(w, 'Edit').trigger('click')
    await nextTick()
    await secretInput(w).setValue('Bearer new-token')
    await buttonNamed(w, 'Save').trigger('click')
    await flushPromises()

    expect(mocks.updateAuthHeader.mock.calls[0][1]).toMatchObject({
      header_value: 'Bearer new-token',
    })
  })

  it('suggests the likely cause when a credential cannot be deleted', async () => {
    mocks.deleteAuthHeader.mockResolvedValue({ success: false })
    const w = await page({ auth: [authHeader()] })
    await openAuthTab(w)
    await buttonNamed(w, 'Delete').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('still be attached to a webhook')
  })
})

describe('what a network error does', () => {
  // FINDING, pinned. Ninth component with this shape.
  it('spins forever when the initial load rejects', async () => {
    const escaped: unknown[] = []
    mocks.listWebhooks.mockRejectedValue(new Error('Network Error'))
    const w = mount(WebhookManagement, {
      global: { stubs, config: { errorHandler: (e: unknown) => escaped.push(e) } },
    })
    await flushPromises()

    expect(w.find('.loading-spinner').exists()).toBe(true)
    expect(escaped).toHaveLength(1)
  })

  // FINDING, pinned. Fourth component with this shape: `saveWebhook` sets
  // `saving = true` with no `finally`. It does clear the flag before both of
  // its *early* returns, which is more care than most -- the gap is only the
  // rejecting request.
  it('strands the save button when the save rejects', async () => {
    const escaped: unknown[] = []
    mocks.createWebhook.mockRejectedValue(new Error('Network Error'))
    const w = mount(WebhookManagement, {
      global: { stubs, config: { errorHandler: (e: unknown) => escaped.push(e) } },
    })
    await flushPromises()
    await openNewWebhook(w)
    await modalInputs(w)[0].setValue('Matrix bridge')
    await modalInputs(w)[1].setValue('https://example.org/hook')
    await w.find('.modal-action .btn-primary').trigger('click')
    await flushPromises()

    expect(
      w.find('.modal-action .btn-primary').attributes('disabled'),
      'the save button now recovers -- if a try/finally was added, delete this test'
    ).toBeDefined()
    expect(escaped).toHaveLength(1)
  })
})
