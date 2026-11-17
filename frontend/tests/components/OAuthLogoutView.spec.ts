// Tier 2: the page that finishes an app's request to sign the member out (#225).
//
// The page passes the app's parameters to the server untouched and goes where
// the server says. It must never follow `post_logout_redirect_uri` itself:
// only the server knows whether that address is registered to the app, and
// following it unchecked would make CS:S an open redirect.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'

const mocks = vi.hoisted(() => ({ endSession: vi.fn(), leaveFor: vi.fn() }))
vi.mock('@/utils/api', () => ({ oauthApi: { endSession: mocks.endSession }, apiClient: {} }))
vi.mock('@/utils/navigate', () => ({ leaveFor: mocks.leaveFor }))
const route = { query: {} as Record<string, string> }
vi.mock('vue-router', () => ({ useRoute: () => route }))

import OAuthLogoutView from '@/views/OAuthLogoutView.vue'

beforeEach(() => {
  setActivePinia(createPinia())
  for (const m of Object.values(mocks)) m.mockReset()
  route.query = {
    id_token_hint: 'hint',
    post_logout_redirect_uri: 'https://evil.example/',
    state: 's',
  }
})

describe('OAuthLogoutView', () => {
  it('hands the parameters to the server and goes where the server says, not where the app asked', async () => {
    mocks.endSession.mockResolvedValue({ success: true, data: { redirect: '/' } })
    mount(OAuthLogoutView)
    await flushPromises()
    expect(mocks.endSession).toHaveBeenCalledWith({
      id_token_hint: 'hint',
      post_logout_redirect_uri: 'https://evil.example/',
      state: 's',
    })
    expect(mocks.leaveFor).toHaveBeenCalledWith('/')
  })

  it('stays and says so when signing out fails', async () => {
    mocks.endSession.mockResolvedValue({ success: false, error: 'boom' })
    const w = mount(OAuthLogoutView)
    await flushPromises()
    expect(w.get('[data-test="error"]').text()).toContain('boom')
    expect(mocks.leaveFor).not.toHaveBeenCalled()
  })
})
