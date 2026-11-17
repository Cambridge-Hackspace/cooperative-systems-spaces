// Tier 2: a member choosing how messages from apps reach them (#226).
//
// The view shows what the server says each choice is, sends exactly the
// change made, and shows a refusal rather than pretending it saved. It
// proves nothing about delivery: the e2e `notify` stage reads the mail sink.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const api = vi.hoisted(() => ({ preferences: vi.fn(), setPreferences: vi.fn() }))
vi.mock('@/utils/api', () => ({ notifyApi: api }))

import NotificationSettingsView from '@/views/NotificationSettingsView.vue'

const stubs = { 'router-link': { props: ['to'], template: '<a :href="to"><slot /></a>' } }
const pop = () => ({
  client: 'c1',
  app: 'POP',
  enabled: true,
  kinds: [
    { kind: 'print.done', label: 'Your print is done', email: true, default: false },
    { kind: 'queue.your_turn', label: 'It is your turn', email: false, default: true },
  ],
})

beforeEach(() => {
  for (const m of Object.values(api)) m.mockReset()
  api.preferences.mockResolvedValue({ success: true, data: [pop()] })
})

const mountIt = async () => {
  const w = mount(NotificationSettingsView, { global: { stubs } })
  await flushPromises()
  return w
}

describe('NotificationSettingsView', () => {
  it('shows each kind with what reaches the member now', async () => {
    const w = await mountIt()
    expect((w.get('[data-test="email-c1-print.done"]').element as HTMLInputElement).checked).toBe(
      true
    )
    expect(
      (w.get('[data-test="email-c1-queue.your_turn"]').element as HTMLInputElement).checked
    ).toBe(false)
  })

  it('sends one kind’s change, and only that', async () => {
    api.setPreferences.mockResolvedValue({ success: true, data: { saved: true } })
    const w = await mountIt()
    await w.get('[data-test="email-c1-queue.your_turn"]').setValue(true)
    await flushPromises()
    expect(api.setPreferences).toHaveBeenCalledWith('c1', { kinds: { 'queue.your_turn': true } })
  })

  it('switches a whole app off, and the kinds follow', async () => {
    api.setPreferences.mockResolvedValue({ success: true, data: { saved: true } })
    const w = await mountIt()
    await w.get('[data-test="enabled-c1"]').setValue(false)
    await flushPromises()
    expect(api.setPreferences).toHaveBeenCalledWith('c1', { enabled: false })
    expect((w.get('[data-test="email-c1-print.done"]').element as HTMLInputElement).disabled).toBe(
      true
    )
  })

  it('shows a refusal rather than pretending it saved', async () => {
    api.setPreferences.mockResolvedValue({ success: false, error: 'nope' })
    const w = await mountIt()
    await w.get('[data-test="email-c1-print.done"]').setValue(false)
    await flushPromises()
    expect(w.get('[data-test="error"]').text()).toContain('nope')
  })

  it('says so when no app sends anything', async () => {
    api.preferences.mockResolvedValue({ success: true, data: [] })
    const w = await mountIt()
    expect(w.find('[data-test="empty"]').exists()).toBe(true)
  })
})
