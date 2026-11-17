// Tier 2: registering the apps that sign members in through CS:S (#225).
//
// The two things a reader could get wrong by assuming: that a secret can be
// seen again (it can't -- it is shown once, from the response that made it),
// and that removing an app is one click (it is two, because everything it was
// issued stops working).

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const api = vi.hoisted(() => ({
  list: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  remove: vi.fn(),
  rotateSecret: vi.fn(),
  rotateKeys: vi.fn(),
}))
vi.mock('@/utils/api', () => ({ oauthAdminApi: api }))

import OAuthAppsManagement from '@/components/OAuthAppsManagement.vue'

const stubs = { 'router-link': { props: ['to'], template: '<a :href="to"><slot /></a>' } }
const app = (over: Record<string, unknown> = {}) => ({
  id: 'a1',
  client_id: 'css-pop',
  name: 'POP',
  confidential: true,
  redirect_uris: ['https://pop.example.org/cb'],
  post_logout_redirect_uris: [],
  allowed_scopes: ['openid', 'training:read'],
  skip_consent: true,
  disabled: false,
  created_by: null,
  created_at: '2026-11-15T00:00:00Z',
  updated_at: '2026-11-15T00:00:00Z',
  ...over,
})

beforeEach(() => {
  for (const m of Object.values(api)) m.mockReset()
  api.list.mockResolvedValue({ success: true, data: [app()] })
})

const mountIt = async () => {
  const w = mount(OAuthAppsManagement, { global: { stubs } })
  await flushPromises()
  return w
}

describe('OAuthAppsManagement', () => {
  it('lists the apps with their client ids', async () => {
    const w = await mountIt()
    expect(w.get('[data-test="app-css-pop"]').text()).toContain('css-pop')
  })

  it('registers an app and shows its secret once', async () => {
    api.list.mockResolvedValue({ success: true, data: [] })
    api.create.mockResolvedValue({
      success: true,
      data: {
        client: app({ id: 'a2', client_id: 'css-new', name: 'New' }),
        client_secret: 'S3CRET',
      },
    })
    const w = await mountIt()
    await w.get('#app-name').setValue('New')
    await w.get('#app-redirects').setValue('https://new.example/cb\n\n')
    await w.get('[data-test="scope-training:read"]').setValue(true)
    await w.get('[data-test="create-form"]').trigger('submit')
    await flushPromises()
    expect(api.create).toHaveBeenCalledWith({
      name: 'New',
      redirect_uris: ['https://new.example/cb'],
      post_logout_redirect_uris: [],
      allowed_scopes: ['openid', 'training:read'],
      confidential: true,
      skip_consent: false,
    })
    expect(w.get('[data-test="secret-value"]').text()).toBe('S3CRET')
    expect(w.find('[data-test="app-css-new"]').exists()).toBe(true)
  })

  it('removes an app only after a second confirmation', async () => {
    api.remove.mockResolvedValue({ success: true, data: { deleted: true } })
    const w = await mountIt()
    await w.get('[data-test="delete-css-pop"]').trigger('click')
    expect(api.remove).not.toHaveBeenCalled()
    await w.get('[data-test="confirm-delete-css-pop"]').trigger('click')
    await flushPromises()
    expect(api.remove).toHaveBeenCalledWith('a1')
    expect(w.find('[data-test="app-css-pop"]').exists()).toBe(false)
  })

  it('shows a new secret when one is made', async () => {
    api.rotateSecret.mockResolvedValue({
      success: true,
      data: { client: app(), client_secret: 'NEW' },
    })
    const w = await mountIt()
    await w.get('[data-test="rotate-css-pop"]').trigger('click')
    await flushPromises()
    expect(w.get('[data-test="secret-value"]').text()).toBe('NEW')
  })

  it('shows the server refusing a change', async () => {
    api.update.mockResolvedValue({ success: false, error: 'oauth.clients.manage needed' })
    const w = await mountIt()
    await w.get('[data-test="toggle-css-pop"]').trigger('click')
    await flushPromises()
    expect(w.get('[data-test="error"]').text()).toContain('oauth.clients.manage needed')
  })
})
