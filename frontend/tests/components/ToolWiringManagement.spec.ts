// Tier 2: ToolWiringManagement (#83).
//
// The per-tool wiring view: which devices fill a tool's reader/power/sensor
// roles, the interlocks that gate it, and whether the modules that switch it can
// reach a safe state unaided.
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

vi.mock('@/utils/api', () => ({
  toolModulesApi: toolModules,
  toolsApi: tools,
}))

import ToolWiringManagement from '@/components/ToolWiringManagement.vue'

function snapshot(overrides: Record<string, unknown> = {}) {
  return {
    success: true,
    data: {
      as_of: '2026-09-15T12:00:00Z',
      tools: [
        {
          tool_id: 'tool-1',
          external_id: 'ext-1',
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

describe('ToolWiringManagement', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    toolModules.state.mockResolvedValue(snapshot())
    tools.getTools.mockResolvedValue({
      success: true,
      data: [{ id: 'tool-1', name: 'Big Laser' }],
    })
  })

  it('names the tool from the catalog rather than showing a bare uuid', async () => {
    const w = mount(ToolWiringManagement)
    await flushPromises()
    expect(w.text()).toContain('Big Laser')
    expect(w.text()).not.toContain('tool-1')
  })

  it('renders the bound modules and their disconnect policy', async () => {
    const w = mount(ToolWiringManagement)
    await flushPromises()
    expect(w.text()).toContain('main plug')
    expect(w.text()).toContain('power')
    expect(w.text()).toContain('fail_off')
  })

  it('renders each interlock with the tier it is actually enforced at', async () => {
    // The tier is the difference between a trip that survives the coordinator
    // dying and one that does not, so it has to be on screen next to the rule.
    const w = mount(ToolWiringManagement)
    await flushPromises()
    expect(w.get('[data-testid="enforcement-i1"]').text()).toBe('edge')
    expect(w.text()).toContain('door_open')
  })

  it('says so when the power cannot fail safe', async () => {
    toolModules.state.mockResolvedValue(snapshot({ power_fails_safe: false }))
    const w = mount(ToolWiringManagement)
    await flushPromises()
    expect(w.get('[data-testid="fail-safe-tool-1"]').text()).toContain('cannot fail safe')
    // And explains what that means rather than leaving a red badge to interpret.
    expect(w.text()).toContain('mitigation')
  })

  it('says so when it can', async () => {
    // The counterpart, so the assertion above is not passing on a string that is
    // always present.
    const w = mount(ToolWiringManagement)
    await flushPromises()
    const badge = w.get('[data-testid="fail-safe-tool-1"]')
    expect(badge.text()).toContain('Power fails safe')
    expect(badge.text()).not.toContain('cannot')
    expect(w.text()).not.toContain('mitigation')
  })

  it('reports nothing wired rather than rendering an empty frame', async () => {
    toolModules.state.mockResolvedValue({ success: true, data: { as_of: 'x', tools: [] } })
    const w = mount(ToolWiringManagement)
    await flushPromises()
    expect(w.text()).toContain('No tool has modules bound yet')
  })

  it('surfaces a failed load instead of showing an empty list', async () => {
    // An error that renders as "nothing is wired" is worse than an error: it
    // reads as a fact about the space.
    toolModules.state.mockRejectedValue(new Error('server exploded'))
    const w = mount(ToolWiringManagement)
    await flushPromises()
    expect(w.find('[role="alert"]').exists()).toBe(true)
    expect(w.text()).toContain('server exploded')
    expect(w.text()).not.toContain('No tool has modules bound yet')
  })
})
