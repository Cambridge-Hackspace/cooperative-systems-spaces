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
import { DOMWrapper, flushPromises, mount } from '@vue/test-utils'

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
const resources = vi.hoisted(() => ({
  list: vi.fn(),
  update: vi.fn(),
  sessions: vi.fn(),
  stop: vi.fn(),
}))

const oauthAdmin = vi.hoisted(() => ({ list: vi.fn() }))

vi.mock('@/utils/api', () => ({
  toolModulesApi: toolModules,
  toolsApi: tools,
  doorsApi: doors,
  resourcesApi: resources,
  oauthAdminApi: oauthAdmin,
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
    resources.sessions.mockResolvedValue({ success: true, data: [] })
    resources.list.mockResolvedValue({ success: true, data: [] })
    oauthAdmin.list.mockResolvedValue({ success: true, data: [] })
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

  it('shows whether a tool identifies its member, and offers the switch on tools only (#228)', async () => {
    const snap = snapshot()
    snap.data.tools.push(doorEntry())
    toolModules.state.mockResolvedValue(snap)
    resources.list.mockResolvedValue({
      success: true,
      data: [
        { id: 'tool-1', kind: 'tool', session_profile: 'until_stopped', identify_member: true },
        { id: 'door-1', kind: 'door', session_profile: 'momentary', identify_member: false },
      ],
    })
    const w = mount(ResourceWiring)
    await flushPromises()
    const toggle = w.get('[data-testid="identify-tool-1"]')
    expect((toggle.element as HTMLInputElement).checked).toBe(true)
    // A door can't name its member, so it isn't offered the switch at all.
    expect(w.find('[data-testid="identify-door-1"]').exists()).toBe(false)
  })

  it('sends a change of identifying the member, and shows what the server says (#228)', async () => {
    resources.update.mockResolvedValue({
      success: true,
      data: { id: 'tool-1', kind: 'tool', session_profile: 'until_stopped', identify_member: true },
    })
    const w = mount(ResourceWiring)
    await flushPromises()
    const toggle = w.get('[data-testid="identify-tool-1"]')
    expect((toggle.element as HTMLInputElement).checked).toBe(false)
    await toggle.setValue(true)
    await flushPromises()
    expect(resources.update).toHaveBeenCalledWith('tool-1', { identify_member: true })
    expect((w.get('[data-testid="identify-tool-1"]').element as HTMLInputElement).checked).toBe(
      true
    )
  })

  it('shows the refusal when the server declines identifying the member (#228)', async () => {
    resources.update.mockResolvedValue({
      success: false,
      error: 'identify_member is available for tools only',
    })
    const w = mount(ResourceWiring)
    await flushPromises()
    await w.get('[data-testid="identify-tool-1"]').setValue(true)
    await flushPromises()
    expect(w.find('[role="alert"]').exists()).toBe(true)
    expect(w.text()).toContain('identify_member is available for tools only')
    // Refused, so the switch goes back to what the server holds.
    expect((w.get('[data-testid="identify-tool-1"]').element as HTMLInputElement).checked).toBe(
      false
    )
  })

  // ── #249: requiring signed messages on the local broker ─────────────────

  it('shows whether a resource requires signed messages, on doors as on tools (#249)', async () => {
    const snap = snapshot()
    snap.data.tools.push(doorEntry())
    toolModules.state.mockResolvedValue(snap)
    resources.list.mockResolvedValue({
      success: true,
      data: [
        { id: 'tool-1', kind: 'tool', session_profile: 'until_stopped', identify_member: false },
        {
          id: 'door-1',
          kind: 'door',
          session_profile: 'momentary',
          identify_member: false,
          require_signed: true,
        },
      ],
    })
    const w = mount(ResourceWiring)
    await flushPromises()
    // A door's strike is what a forged reply opens, so it is offered too.
    const door = w.get('[data-testid="require-signed-door-1"]')
    expect((door.element as HTMLInputElement).checked).toBe(true)
    // Absent from the server means off.
    const tool = w.get('[data-testid="require-signed-tool-1"]')
    expect((tool.element as HTMLInputElement).checked).toBe(false)
  })

  it('sends a change of requiring signed messages, and shows what the server says (#249)', async () => {
    resources.update.mockResolvedValue({
      success: true,
      data: {
        id: 'tool-1',
        kind: 'tool',
        session_profile: 'until_stopped',
        identify_member: false,
        require_signed: true,
      },
    })
    const w = mount(ResourceWiring)
    await flushPromises()
    const toggle = w.get('[data-testid="require-signed-tool-1"]')
    expect((toggle.element as HTMLInputElement).checked).toBe(false)
    await toggle.setValue(true)
    await flushPromises()
    expect(resources.update).toHaveBeenCalledWith('tool-1', { require_signed: true })
    expect(
      (w.get('[data-testid="require-signed-tool-1"]').element as HTMLInputElement).checked
    ).toBe(true)
  })

  it('shows the refusal when the server declines requiring signed messages (#249)', async () => {
    resources.update.mockResolvedValue({ success: false, error: 'Resource not found' })
    const w = mount(ResourceWiring)
    await flushPromises()
    await w.get('[data-testid="require-signed-tool-1"]').setValue(true)
    await flushPromises()
    expect(w.find('[role="alert"]').exists()).toBe(true)
    expect(w.text()).toContain('Resource not found')
    expect(
      (w.get('[data-testid="require-signed-tool-1"]').element as HTMLInputElement).checked
    ).toBe(false)
  })

  // ── #241: a tap that signs the member in to an app ──────────────────────
  //
  // Which apps are offered is the server's rule (oidc::card::app_for),
  // duplicated deliberately here: the view must offer the same set, so the
  // spec states the rule itself rather than reusing the component's.

  function app(id: string, over: Record<string, unknown> = {}) {
    return {
      id,
      client_id: `css_${id}`,
      name: `App ${id}`,
      confidential: true,
      skip_consent: true,
      disabled: false,
      redirect_uris: [],
      post_logout_redirect_uris: [],
      allowed_scopes: ['openid'],
      created_by: null,
      created_at: '2026-11-19T00:00:00Z',
      updated_at: '2026-11-19T00:00:00Z',
      ...over,
    }
  }

  function identifying(over: Record<string, unknown> = {}) {
    return {
      id: 'tool-1',
      kind: 'tool',
      session_profile: 'until_stopped',
      identify_member: true,
      sign_in_app: null,
      second_factor: 'off',
      ...over,
    }
  }

  /** The values a select offers, in order. */
  function options(select: Omit<DOMWrapper<Element>, 'exists'>): string[] {
    return select.findAll('option').map((o) => (o.element as HTMLOptionElement).value)
  }

  it('offers only apps a card may sign in to, and only while identifying (#241)', async () => {
    oauthAdmin.list.mockResolvedValue({
      success: true,
      data: [
        app('own'),
        app('third-party', { skip_consent: false }),
        app('public', { confidential: false }),
        app('off', { disabled: true }),
      ],
    })
    resources.list.mockResolvedValue({ success: true, data: [identifying()] })
    const w = mount(ResourceWiring)
    await flushPromises()
    expect(options(w.get('[data-testid="sign-in-app-tool-1"]'))).toEqual(['', 'own'])
    // No app yet: nothing to ask a second factor for.
    expect(w.find('[data-testid="second-factor-tool-1"]').exists()).toBe(false)

    resources.list.mockResolvedValue({
      success: true,
      data: [identifying({ identify_member: false })],
    })
    const off = mount(ResourceWiring)
    await flushPromises()
    expect(off.find('[data-testid="sign-in-app-tool-1"]').exists()).toBe(false)
  })

  it('keeps showing an app that is no longer eligible while a tool still uses it (#241)', async () => {
    oauthAdmin.list.mockResolvedValue({
      success: true,
      data: [app('own'), app('was', { disabled: true })],
    })
    resources.list.mockResolvedValue({ success: true, data: [identifying({ sign_in_app: 'was' })] })
    const w = mount(ResourceWiring)
    await flushPromises()
    const select = w.get('[data-testid="sign-in-app-tool-1"]')
    expect((select.element as HTMLSelectElement).value).toBe('was')
    expect(select.text()).toContain('no longer eligible')
  })

  it('never shows a tool that signs in as signing in to nothing without the app list (#241)', async () => {
    oauthAdmin.list.mockResolvedValue({ success: false, error: 'Forbidden' })
    resources.list.mockResolvedValue({ success: true, data: [identifying({ sign_in_app: 'abc' })] })
    const w = mount(ResourceWiring)
    await flushPromises()
    expect((w.get('[data-testid="sign-in-app-tool-1"]').element as HTMLSelectElement).value).toBe(
      'abc'
    )
  })

  it('sends the app and the second factor, and shows what the server says (#241)', async () => {
    oauthAdmin.list.mockResolvedValue({ success: true, data: [app('own')] })
    resources.list.mockResolvedValue({ success: true, data: [identifying()] })
    resources.update.mockResolvedValueOnce({
      success: true,
      data: identifying({ sign_in_app: 'own' }),
    })
    const w = mount(ResourceWiring)
    await flushPromises()
    await w.get('[data-testid="sign-in-app-tool-1"]').setValue('own')
    await flushPromises()
    expect(resources.update).toHaveBeenLastCalledWith('tool-1', { sign_in_app: 'own' })

    resources.update.mockResolvedValueOnce({
      success: true,
      data: identifying({ sign_in_app: 'own', second_factor: 'everyone' }),
    })
    const factor = w.get('[data-testid="second-factor-tool-1"]')
    expect((factor.element as HTMLSelectElement).value).toBe('off')
    await factor.setValue('everyone')
    await flushPromises()
    expect(resources.update).toHaveBeenLastCalledWith('tool-1', { second_factor: 'everyone' })
    expect((w.get('[data-testid="second-factor-tool-1"]').element as HTMLSelectElement).value).toBe(
      'everyone'
    )
  })

  it('turns the sign-in off with identifying, in one request (#241)', async () => {
    oauthAdmin.list.mockResolvedValue({ success: true, data: [app('own')] })
    resources.list.mockResolvedValue({ success: true, data: [identifying({ sign_in_app: 'own' })] })
    resources.update.mockResolvedValue({
      success: true,
      data: identifying({ identify_member: false, sign_in_app: null }),
    })
    const w = mount(ResourceWiring)
    await flushPromises()
    await w.get('[data-testid="identify-tool-1"]').setValue(false)
    await flushPromises()
    expect(resources.update).toHaveBeenCalledTimes(1)
    expect(resources.update).toHaveBeenCalledWith('tool-1', {
      identify_member: false,
      sign_in_app: '',
    })
    expect(w.find('[data-testid="sign-in-app-tool-1"]').exists()).toBe(false)
  })

  it('shows the refusal when the server declines a sign-in app (#241)', async () => {
    oauthAdmin.list.mockResolvedValue({ success: true, data: [app('own')] })
    resources.list.mockResolvedValue({ success: true, data: [identifying()] })
    resources.update.mockResolvedValue({
      success: false,
      error:
        "sign_in_app must be an app that is the space's own, has a secret, and is not disabled",
    })
    const w = mount(ResourceWiring)
    await flushPromises()
    await w.get('[data-testid="sign-in-app-tool-1"]').setValue('own')
    await flushPromises()
    expect(w.find('[role="alert"]').exists()).toBe(true)
    expect(w.text()).toContain("sign_in_app must be an app that is the space's own")
    // Refused, so the select goes back to what the server holds: nothing.
    expect((w.get('[data-testid="sign-in-app-tool-1"]').element as HTMLSelectElement).value).toBe(
      ''
    )
  })

  it('shows the refusal when the server declines a profile change', async () => {
    resources.update.mockResolvedValue({ success: false, error: 'session_profile must be one of' })
    const w = mount(ResourceWiring)
    await flushPromises()
    await w.get('[data-testid="profile-tool-1"]').setValue('momentary')
    await flushPromises()
    expect(w.find('[role="alert"]').exists()).toBe(true)
    expect(w.text()).toContain('session_profile must be one of')
    expect((w.get('[data-testid="profile-tool-1"]').element as HTMLSelectElement).value).toBe(
      'until_stopped'
    )
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

  it('shows an open session with who holds it, and ends it from the platform (#190)', async () => {
    resources.sessions.mockResolvedValue({
      success: true,
      data: [
        {
          resource_id: 'tool-1',
          kind: 'tool',
          user_id: 'u-1',
          username: 'ada',
          full_name: 'Ada Lovelace',
          opened_at: '2026-10-28T10:00:00Z',
          device_id: null,
        },
      ],
    })
    resources.stop.mockResolvedValue({
      success: true,
      data: { resource_id: 'tool-1', kind: 'tool', ended: true },
    })
    const w = mount(ResourceWiring)
    await flushPromises()
    const session = w.find('[data-testid="session-tool-1"]')
    expect(session.exists(), 'the open session must be shown on its resource').toBe(true)
    expect(session.text()).toContain('Ada Lovelace')
    await w.find('[data-testid="end-session-tool-1"]').trigger('click')
    await flushPromises()
    expect(resources.stop).toHaveBeenCalledWith('tool-1')
    expect(
      w.find('[data-testid="session-tool-1"]').exists(),
      'once ended, the session must no longer be shown'
    ).toBe(false)
  })

  it('shows no session on a resource the platform lists none for (#190)', async () => {
    const w = mount(ResourceWiring)
    await flushPromises()
    expect(w.find('[data-testid="session-tool-1"]').exists()).toBe(false)
  })
})
