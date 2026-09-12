// Tier 2: component conformance for RolesAdminView — the RBAC management screen.
//
// The screen is the only place a role, the permission matrix, or the inheritance
// graph is edited from the UI, so the assertions here are about what it offers
// and what it sends:
//
//   - a system role cannot be deleted (no delete control on its row), because
//     the server refuses it and offering the button would only produce a 403;
//   - a matrix tick reflects a *direct* grant and toggling it sends the whole
//     new set (the API replaces, it does not patch);
//   - a refused inheritance change (the server's cycle guard) is surfaced rather
//     than swallowed, since that is the one edit a careful admin can still get
//     wrong.
//
// WHAT THIS DOES NOT PROVE: that the server actually enforces any of it — that
// is the contract/roles tier's job. Here the claim is only about the UI.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createTestingPinia } from '@pinia/testing'
import { flushPromises, mount } from '@vue/test-utils'

import RolesAdminView from '@/views/RolesAdminView.vue'
import { useAuthStore } from '@/stores/auth'
import { type RbacConfig } from '@/types'

type Envelope = Promise<{ success: boolean; error?: string; data?: unknown }>

const mocks = vi.hoisted(() => ({
  config: vi.fn(),
  createRole: vi.fn(),
  updateRole: vi.fn(),
  removeRole: vi.fn(),
  setPermissions: vi.fn(),
  setInheritance: vi.fn(),
}))

vi.mock('@/utils/api', () => ({
  rbacApi: {
    config: mocks.config,
    createRole: mocks.createRole,
    updateRole: mocks.updateRole,
    removeRole: mocks.removeRole,
    setPermissions: mocks.setPermissions,
    setInheritance: mocks.setInheritance,
  },
  apiClient: {},
}))

const config = mocks.config as unknown as ReturnType<typeof vi.fn<() => Envelope>>
const removeRole = mocks.removeRole as unknown as ReturnType<typeof vi.fn<(id: string) => Envelope>>
const setPermissions = mocks.setPermissions as unknown as ReturnType<
  typeof vi.fn<(id: string, perms: string[]) => Envelope>
>
const setInheritance = mocks.setInheritance as unknown as ReturnType<
  typeof vi.fn<(id: string, inh: string[]) => Envelope>
>

const stubs = { RouterLink: { template: '<a><slot /></a>' } }

const SAMPLE: RbacConfig = {
  roles: [
    {
      id: 'r-member',
      name: 'member',
      description: 'A member',
      is_system: true,
      level: 2,
      inherits: [],
      permissions: ['member.access'],
    },
    {
      id: 'r-custom',
      name: 'shopkeeper',
      description: 'Runs the shop',
      is_system: false,
      level: 2,
      inherits: ['r-member'],
      permissions: [],
    },
  ],
  permissions: [
    { key: 'member.access', description: 'Access member routes' },
    { key: 'tools.manage', description: 'Manage tools' },
  ],
}

async function mountView() {
  const pinia = createTestingPinia({ createSpy: vi.fn, stubActions: true })
  const auth = useAuthStore(pinia)
  ;(auth as { isAdmin: boolean }).isAdmin = true
  config.mockResolvedValue({ success: true, data: structuredClone(SAMPLE) })
  const wrapper = mount(RolesAdminView, { global: { plugins: [pinia], stubs } })
  await flushPromises()
  return { wrapper, auth }
}

beforeEach(() => {
  for (const m of [config, removeRole, setPermissions, setInheritance]) m.mockReset()
})

describe('RolesAdminView', () => {
  it('lists every role and flags the system ones', async () => {
    const { wrapper } = await mountView()
    // The first table is the roles list (the second is the permission matrix).
    const rows = wrapper.findAll('table')[0].findAll('tbody tr')
    expect(rows).toHaveLength(2)
    // The system role is badged; the custom one is not.
    expect(rows[0].text()).toContain('member')
    expect(rows[0].text()).toContain('system')
    expect(rows[1].text()).toContain('shopkeeper')
    expect(rows[1].text()).not.toContain('system')
  })

  it('offers no delete control on a system role', async () => {
    const { wrapper } = await mountView()
    const rows = wrapper.findAll('table')[0].findAll('tbody tr')
    const del = (r: (typeof rows)[number]) =>
      r.findAll('button').filter((b) => b.text() === 'Delete')
    expect(del(rows[0]), 'system role must not be deletable').toHaveLength(0)
    expect(del(rows[1]), 'a custom role is deletable').toHaveLength(1)
  })

  it('reflects direct grants in the matrix and replaces the whole set on toggle', async () => {
    setPermissions.mockResolvedValue({ success: true })
    const { wrapper } = await mountView()

    // Matrix: one checkbox per (permission, role). member.access is granted to
    // the member role (checked), tools.manage to nobody (unchecked).
    const memberAccess = wrapper.get('input[aria-label="member.access for member"]')
    expect((memberAccess.element as HTMLInputElement).checked).toBe(true)
    const toolsForCustom = wrapper.get('input[aria-label="tools.manage for shopkeeper"]')
    expect((toolsForCustom.element as HTMLInputElement).checked).toBe(false)

    // Granting tools.manage to shopkeeper sends the full new set (replace, not patch).
    await toolsForCustom.setValue(true)
    await flushPromises()
    expect(setPermissions).toHaveBeenCalledWith('r-custom', ['tools.manage'])
  })

  it('surfaces a refused inheritance change (the cycle guard) instead of swallowing it', async () => {
    setInheritance.mockResolvedValue({
      success: false,
      error: 'this inheritance change would create a cycle',
    })
    const { wrapper } = await mountView()

    // Open the inheritance editor for the custom role (second row).
    const inhBtn = wrapper
      .findAll('table')[0]
      .findAll('tbody tr')[1]
      .findAll('button')
      .find((b) => b.text() === 'Inheritance')
    await inhBtn.trigger('click')
    await wrapper.get('.card-actions button.btn-primary').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('would create a cycle')
  })

  it('refuses to render for a non-admin', async () => {
    const pinia = createTestingPinia({ createSpy: vi.fn, stubActions: true })
    const auth = useAuthStore(pinia)
    ;(auth as { isAdmin: boolean }).isAdmin = false
    const wrapper = mount(RolesAdminView, { global: { plugins: [pinia], stubs } })
    await flushPromises()
    expect(wrapper.text()).toContain('administrator privileges')
    expect(config).not.toHaveBeenCalled()
  })
})
