// Tier 2: ResourceWiring (#83, #182 -- was the tool wiring view).
//
// The per-resource wiring view: which devices fill a resource's reader / power /
// strike / sensor roles, the interlocks that gate it, and whether the modules
// that switch it can reach a safe state unaided. #182: doors appear beside
// tools, each entry naming its kind, and a strike binding shows its pulse.
//
// What is under test here is that the view reports what the server says --
// especially the two things a reader could otherwise get wrong by assuming:
// the fail-safe verdict, and which tier each interlock is actually enforced at.
// A tool switched by a plug that cannot de-energize itself must not look the
// same as one that can.
//
// What this spec does NOT prove: that any interlock fires, that a lease lapses,
// or that a tier is achievable. Those are the edge crate's unit tests and the
// e2e `toolmodules` stage, against real logic and a real database.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const toolModules = vi.hoisted(() => ({
  listModules: vi.fn(),
  createModule: vi.fn(),
  removeModule: vi.fn(),
  state: vi.fn(),
  listInterlocks: vi.fn(),
  createInterlock: vi.fn(),
  removeInterlock: vi.fn(),
}))
const tools = vi.hoisted(() => ({ getTools: vi.fn() }))
const doors = vi.hoisted(() => ({ list: vi.fn() }))
const resources = vi.hoisted(() => ({ update: vi.fn() }))

vi.mock('@/utils/api', () => ({
  toolModulesApi: toolModules,
  toolsApi: tools,
  doorsApi: doors,
  resourcesApi: resources,
}))

import ResourceWiring from '@/components/ResourceWiring.vue'

function snapshot(overrides: Record<string, unknown> = {}) {
  return {
    success: true,
    data: {
      as_of: '2026-09-15T12:00:00Z',
      tools: [
        {
          tool_id: 'tool-1',
          external_id: 'ext-1',
          kind: 'tool',
          modules: [
            {
              id: 'm1',
              device_id: 'dev-1',
              role: 'power',
              name: 'main plug',
              params: {},
              on_disconnect: 'fail_off',
            },
          ],
          interlocks: [
            {
              id: 'i1',
              kind: 'trip',
              condition: 'door_open',
              source_module_id: null,
              debounce_ms: 0,
              latch: true,
              reset: 're_auth',
              enforcement: 'edge',
            },
          ],
          power_fails_safe: true,
          ...overrides,
        },
      ],
    },
  }
}

/** A door with a coordinator and a strike, as the snapshot carries it since #182. */
function doorEntry() {
  return {
    tool_id: 'door-1',
    external_id: null,
    kind: 'door',
    session_profile: 'momentary',
    modules: [
      {
        id: 'e1',
        device_id: 'dev-9',
        role: 'edge',
        name: 'front coordinator',
        params: {},
        on_disconnect: 'fail_off',
      },
      {
        id: 's1',
        device_id: 'dev-9',
        role: 'strike',
        name: 'front latch',
        params: {},
        on_disconnect: 'ignore',
        pulse_ms: 4200,
        hold_open_access: false,
      },
    ],
    interlocks: [],
    power_fails_safe: false,
  }
}

describe('ResourceWiring', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    toolModules.state.mockResolvedValue(snapshot())
    tools.getTools.mockResolvedValue({
      success: true,
      data: [{ id: 'tool-1', name: 'Big Laser' }],
    })
    doors.list.mockResolvedValue({
      success: true,
      data: [{ id: 'door-1', name: 'Front door' }],
    })
  })

  it('names a door from the door list and labels its kind (#182)', async () => {
    const snap = snapshot()
    snap.data.tools.push(doorEntry())
    toolModules.state.mockResolvedValue(snap)
    const w = mount(ResourceWiring)
    await flushPromises()
    expect(w.text()).toContain('Front door')
    expect(w.text()).not.toContain('door-1')
    expect(w.get('[data-testid="kind-door-1"]').text()).toBe('door')
    expect(w.get('[data-testid="kind-tool-1"]').text()).toBe('tool')
  })

  it("shows a strike binding's pulse and whether it is held for open access", async () => {
    const snap = snapshot()
    snap.data.tools.push(doorEntry())
    toolModules.state.mockResolvedValue(snap)
    const w = mount(ResourceWiring)
    await flushPromises()
    const cell = w.get('[data-testid="pulse-s1"]').text()
    expect(cell).toContain('4.2 s')
    expect(cell).toContain('not held')
    // The coordinator row has no pulse to show.
    expect(w.get('[data-testid="pulse-e1"]').text()).toBe('—')
  })

  it('offers the session profile per resource and sends a change to the server (#182)', async () => {
    const snap = snapshot()
    snap.data.tools.push(doorEntry())
    toolModules.state.mockResolvedValue(snap)
    resources.update.mockResolvedValue({
      success: true,
      data: { id: 'door-1', kind: 'door', session_profile: 'until_stopped' },
    })
    const w = mount(ResourceWiring)
    await flushPromises()
    const select = w.get('[data-testid="profile-door-1"]')
    expect((select.element as HTMLSelectElement).value).toBe('momentary')
    // A server that predates the field sends '' and the kind's default shows.
    expect((w.get('[data-testid="profile-tool-1"]').element as HTMLSelectElement).value).toBe(
      'until_stopped'
    )
    await select.setValue('until_stopped')
    await flushPromises()
    expect(resources.update).toHaveBeenCalledWith('door-1', { session_profile: 'until_stopped' })
    expect((select.element as HTMLSelectElement).value).toBe('until_stopped')
  })

  it('shows the refusal when the server declines a profile change', async () => {
    resources.update.mockResolvedValue({ success: false, error: 'session_profile must be one of' })
    const w = mount(ResourceWiring)
    await flushPromises()
    await w.get('[data-testid="profile-tool-1"]').setValue('momentary')
    await flushPromises()
    expect(w.find('[role="alert"]').exists()).toBe(true)
    expect(w.text()).toContain('session_profile must be one of')
  })

  it('does not warn about power on a resource that has no power binding', async () => {
    // "Power cannot fail safe" on a door with a strike and a coordinator would be
    // a warning about a plug it does not have.
    const snap = snapshot()
    snap.data.tools.push(doorEntry())
    toolModules.state.mockResolvedValue(snap)
    const w = mount(ResourceWiring)
    await flushPromises()
    expect(w.find('[data-testid="fail-safe-door-1"]').exists()).toBe(false)
    expect(w.find('[data-testid="fail-safe-tool-1"]').exists()).toBe(true)
  })

  it('names the tool from the catalog rather than showing a bare uuid', async () => {
    const w = mount(ResourceWiring)
    await flushPromises()
    expect(w.text()).toContain('Big Laser')
    expect(w.text()).not.toContain('tool-1')
  })

  it('renders the bound modules and their disconnect policy', async () => {
    const w = mount(ResourceWiring)
    await flushPromises()
    expect(w.text()).toContain('main plug')
    expect(w.text()).toContain('power')
    expect(w.text()).toContain('fail_off')
  })

  it('renders each interlock with the tier it is actually enforced at', async () => {
    // The tier is the difference between a trip that survives the coordinator
    // dying and one that does not, so it has to be on screen next to the rule.
    const w = mount(ResourceWiring)
    await flushPromises()
    expect(w.get('[data-testid="enforcement-i1"]').text()).toBe('edge')
    expect(w.text()).toContain('door_open')
  })

  it('says so when the power cannot fail safe', async () => {
    toolModules.state.mockResolvedValue(snapshot({ power_fails_safe: false }))
    const w = mount(ResourceWiring)
    await flushPromises()
    expect(w.get('[data-testid="fail-safe-tool-1"]').text()).toContain('cannot fail safe')
    // And explains what that means rather than leaving a red badge to interpret.
    expect(w.text()).toContain('mitigation')
  })

  it('says so when it can', async () => {
    // The counterpart, so the assertion above is not passing on a string that is
    // always present.
    const w = mount(ResourceWiring)
    await flushPromises()
    const badge = w.get('[data-testid="fail-safe-tool-1"]')
    expect(badge.text()).toContain('Power fails safe')
    expect(badge.text()).not.toContain('cannot')
    expect(w.text()).not.toContain('mitigation')
  })

  it('reports nothing wired rather than rendering an empty frame', async () => {
    toolModules.state.mockResolvedValue({ success: true, data: { as_of: 'x', tools: [] } })
    const w = mount(ResourceWiring)
    await flushPromises()
    expect(w.text()).toContain('No resource has a device bound yet')
  })

  it('surfaces a failed load instead of showing an empty list', async () => {
    // An error that renders as "nothing is wired" is worse than an error: it
    // reads as a fact about the space.
    toolModules.state.mockRejectedValue(new Error('server exploded'))
    const w = mount(ResourceWiring)
    await flushPromises()
    expect(w.find('[role="alert"]').exists()).toBe(true)
    expect(w.text()).toContain('server exploded')
    expect(w.text()).not.toContain('No resource has a device bound yet')
  })
})
