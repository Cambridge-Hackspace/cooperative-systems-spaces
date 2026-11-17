// Tier 2: the page that finishes a sign-in another app asked CS:S for (#225).
//
// What is under test is where the browser is sent and when: a trusted app or
// a remembered consent is answered without a question; anything else asks,
// and sends back exactly what the server says -- a code or access_denied. The
// page never builds a redirect itself; the server's redirect is the only one.
//
// What this does NOT prove: that the code is valid, that PKCE is checked, or
// that the redirect is registered. Those are the server's, and the e2e `oidc`
// stage drives them against a real server.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const mocks = vi.hoisted(() => ({ request: vi.fn(), decide: vi.fn(), leaveFor: vi.fn() }))
vi.mock('@/utils/api', () => ({ oauthApi: { request: mocks.request, decide: mocks.decide } }))
vi.mock('@/utils/navigate', () => ({ leaveFor: mocks.leaveFor }))
const route = { params: { id: 'req-1' } as Record<string, string> }
vi.mock('vue-router', () => ({ useRoute: () => route }))

import OAuthConsentView from '@/views/OAuthConsentView.vue'

const asking = {
  success: true,
  data: {
    app: 'POP',
    returns_to: 'pop.example.org',
    scopes: [
      { scope: 'openid', description: 'Confirm who you are' },
      { scope: 'training:read', description: 'Which tools you are trained on' },
    ],
    automatic: false,
  },
}

beforeEach(() => {
  for (const m of Object.values(mocks)) m.mockReset()
})

describe('OAuthConsentView', () => {
  it('asks, naming the app, what it will see, and where it goes back to', async () => {
    mocks.request.mockResolvedValue(asking)
    const w = mount(OAuthConsentView)
    await flushPromises()
    expect(mocks.request).toHaveBeenCalledWith('req-1')
    expect(w.get('[data-test="app"]').text()).toBe('POP')
    expect(w.get('[data-test="scopes"]').text()).toContain('Which tools you are trained on')
    expect(w.get('[data-test="returns-to"]').text()).toContain('pop.example.org')
    expect(mocks.decide).not.toHaveBeenCalled()
    expect(mocks.leaveFor).not.toHaveBeenCalled()
  })

  it('sends the member back with whatever the server answers to Allow', async () => {
    mocks.request.mockResolvedValue(asking)
    mocks.decide.mockResolvedValue({
      success: true,
      data: { redirect: 'https://pop.example.org/cb?code=c&state=s' },
    })
    const w = mount(OAuthConsentView)
    await flushPromises()
    await w.get('[data-test="allow"]').trigger('click')
    await flushPromises()
    expect(mocks.decide).toHaveBeenCalledWith('req-1', true)
    expect(mocks.leaveFor).toHaveBeenCalledWith('https://pop.example.org/cb?code=c&state=s')
  })

  it('tells the server No, and sends the member back with that', async () => {
    mocks.request.mockResolvedValue(asking)
    mocks.decide.mockResolvedValue({
      success: true,
      data: { redirect: 'https://pop.example.org/cb?error=access_denied' },
    })
    const w = mount(OAuthConsentView)
    await flushPromises()
    await w.get('[data-test="deny"]').trigger('click')
    await flushPromises()
    expect(mocks.decide).toHaveBeenCalledWith('req-1', false)
    expect(mocks.leaveFor).toHaveBeenCalledWith('https://pop.example.org/cb?error=access_denied')
  })

  it('asks nothing when there is nothing to ask', async () => {
    mocks.request.mockResolvedValue({ ...asking, data: { ...asking.data, automatic: true } })
    mocks.decide.mockResolvedValue({
      success: true,
      data: { redirect: 'https://pop.example.org/cb?code=c' },
    })
    const w = mount(OAuthConsentView)
    await flushPromises()
    expect(mocks.decide).toHaveBeenCalledWith('req-1', true)
    expect(mocks.leaveFor).toHaveBeenCalledWith('https://pop.example.org/cb?code=c')
    expect(w.find('[data-test="allow"]').exists()).toBe(false)
  })

  it('says so when the request has expired, and sends the member nowhere', async () => {
    mocks.request.mockResolvedValue({
      success: false,
      error: 'This sign-in request has expired. Start again from the app.',
    })
    const w = mount(OAuthConsentView)
    await flushPromises()
    expect(w.get('[data-test="error"]').text()).toContain('expired')
    expect(mocks.leaveFor).not.toHaveBeenCalled()
  })
})
