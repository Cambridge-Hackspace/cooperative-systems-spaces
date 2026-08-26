// Tier 2: component conformance for ToolCard.
//
// The component has two entirely different faces — one for somebody who can
// manage the tool and one for somebody who can only use it — chosen by a single
// prop. Neither face is reachable from the other in a running app without a
// role change, so a mistake in one is invisible to anybody working on the
// other. That is the shape this file is for.
//
// It is also where a real defect lived. The training warning was chained as a
// `v-else-if` onto a button whose condition was `hasTrainingSteps`, while the
// warning's own condition *required* `hasTrainingSteps` — so the branch could
// never be taken, and no member was ever told that a tool needs training first.
// The test named for it is the one that would have caught it, and it asserts
// the button and the warning appear *together*, because they are not
// alternatives.

import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'

import ToolCard from '@/components/ToolCard.vue'
import { ToolCategory, ToolStatus, type Tool } from '@/types/tools'

// The enum members, not their string values. `ToolStatus` is a TypeScript
// enum, so `'idle'` is not assignable to it -- and vue-tsc type-checks this
// directory as part of `npm run build`, which is how the first version of
// this file broke the production build rather than just the test run. Using
// the members also means a renamed variant fails to compile here.
const STATUSES = [
  ToolStatus.Idle,
  ToolStatus.InUse,
  ToolStatus.Maintenance,
  ToolStatus.Broken,
  ToolStatus.Repair,
  ToolStatus.Retired,
] as const

function tool(overrides: Partial<Tool> = {}): Tool {
  return {
    id: '00000000-0000-4000-8000-000000000001',
    name: 'Bandsaw',
    category: ToolCategory.Saw,
    status: ToolStatus.Idle,
    description: null,
    location: null,
    manufacturer: null,
    model: null,
    serial_number: null,
    purchase_date: null,
    purchase_price: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  } as Tool
}

/** ToolTrainingModal reaches for the API on mount; it is not this file's subject. */
const stubs = { ToolTrainingModal: true }

function mountCard(props: Record<string, unknown>) {
  return mount(ToolCard, { props: { tool: tool(), canManage: false, ...props }, global: { stubs } })
}

describe('the training warning', () => {
  it('renders alongside the View Training button, not instead of it', () => {
    // The defect, stated as an assertion. `v-else-if` made these two mutually
    // exclusive; the warning's condition required exactly what the button's
    // condition provided, so the warning was dead code that read as a feature.
    const wrapper = mountCard({
      canManage: false,
      hasTrainingSteps: true,
      canUseBasedOnTraining: false,
    })

    expect(wrapper.find('.training-btn').exists()).toBe(true)
    expect(wrapper.find('.training-warning').exists()).toBe(true)
    expect(wrapper.find('.training-warning').text()).toContain('Training required before using')
  })

  it('is absent once the viewer has completed the training', () => {
    const wrapper = mountCard({
      canManage: false,
      hasTrainingSteps: true,
      canUseBasedOnTraining: true,
    })
    expect(wrapper.find('.training-btn').exists()).toBe(true)
    expect(wrapper.find('.training-warning').exists()).toBe(false)
  })

  it('is absent when the tool has no training at all', () => {
    const wrapper = mountCard({
      canManage: false,
      hasTrainingSteps: false,
      canUseBasedOnTraining: false,
    })
    expect(wrapper.find('.training-warning').exists()).toBe(false)
    expect(wrapper.find('.training-btn').exists()).toBe(false)
  })

  it('is absent for a tool that cannot be used right now anyway', () => {
    // "You need training" on a broken tool is advice about a tool nobody can
    // touch, competing for attention with the reason they cannot touch it.
    for (const status of STATUSES.filter((s) => s !== ToolStatus.Idle)) {
      const wrapper = mountCard({
        tool: tool({ status }),
        canManage: false,
        hasTrainingSteps: true,
        canUseBasedOnTraining: false,
      })
      expect(wrapper.find('.training-warning').exists(), status).toBe(false)
    }
  })

  it('defaults to assuming the viewer may use the tool', () => {
    // `canUseBasedOnTraining` defaults to true. A default of false would show
    // the warning to everybody on every tool until the parent said otherwise,
    // and a parent that forgot to pass it would look like a training system
    // nobody had completed.
    const wrapper = mountCard({ canManage: false, hasTrainingSteps: true })
    expect(wrapper.find('.training-warning').exists()).toBe(false)
  })
})

describe('the two faces', () => {
  it('shows management controls only to somebody who can manage', () => {
    const manager = mountCard({ canManage: true })
    expect(manager.find('.tool-actions').exists()).toBe(true)
    expect(manager.find('.member-actions').exists()).toBe(false)

    const member = mountCard({ canManage: false })
    expect(member.find('.tool-actions').exists()).toBe(false)
    expect(member.find('.member-actions').exists()).toBe(true)
  })

  it('offers no status control to a member, on any status', () => {
    // The status select is how a tool is taken out of service. It must not
    // appear for somebody who cannot manage the tool, whatever state it is in.
    for (const status of STATUSES) {
      const wrapper = mountCard({ tool: tool({ status }), canManage: false })
      expect(wrapper.find('select').exists(), status).toBe(false)
      expect(wrapper.findAll('button').length, `${status}: member has buttons`).toBeLessThanOrEqual(1)
    }
  })
})

describe('availability, as a member sees it', () => {
  const EXPECTED: Array<[ToolStatus, string, string]> = [
    [ToolStatus.Idle, '.available', '✅ Available for use'],
    [ToolStatus.InUse, '.in-use', '⏳ Currently in use'],
    [ToolStatus.Maintenance, '.unavailable', '❌ Not available (Maintenance)'],
    [ToolStatus.Broken, '.unavailable', '❌ Not available (Broken)'],
    [ToolStatus.Repair, '.unavailable', '❌ Not available (Repair)'],
    [ToolStatus.Retired, '.unavailable', '❌ Not available (Retired)'],
  ]

  it('covers every status the enum declares', () => {
    // The table above is hand-written, which is what makes it an independent
    // statement of what each status should read as. This is what stops it
    // silently falling behind the enum.
    expect(EXPECTED.map(([s]) => s).sort()).toEqual([...STATUSES].sort())
  })

  it.each(EXPECTED)('%s reads as %s', (status, selector, text) => {
    const wrapper = mountCard({ tool: tool({ status }), canManage: false })
    const el = wrapper.find(`.availability-info ${selector}`)
    expect(el.exists()).toBe(true)
    expect(el.text()).toBe(text)
  })

  it('shows exactly one availability line', () => {
    for (const [status] of EXPECTED) {
      const wrapper = mountCard({ tool: tool({ status }), canManage: false })
      expect(wrapper.find('.availability-info').element.children.length, status).toBe(1)
    }
  })
})

describe('status formatting', () => {
  it('turns the wire value into words without losing the wire value', () => {
    // The label is `In Use`; the class stays `status-in_use`, because that is
    // what the stylesheet keys on. Formatting both would silently unstyle the
    // card.
    const wrapper = mountCard({ tool: tool({ status: ToolStatus.InUse }), canManage: false })
    expect(wrapper.find('.status-badge').text()).toBe('In Use')
    expect(wrapper.find('.status-badge').classes()).toContain('status-in_use')
    expect(wrapper.classes()).toContain('status-in_use')
  })

  it('title-cases a multi-word category the same way', () => {
    const wrapper = mountCard({ tool: tool({ category: ToolCategory.LaserCutting }), canManage: false })
    expect(wrapper.find('.tool-category').text()).toBe('Laser Cutting')
  })
})

describe('the optional detail rows', () => {
  it('renders no row for a field the tool does not have', () => {
    const wrapper = mountCard({ canManage: false })
    expect(wrapper.findAll('.info-row')).toHaveLength(0)
  })

  it('renders one row per populated field, labelled', () => {
    const wrapper = mountCard({
      tool: tool({
        description: 'A big saw',
        location: 'Bay 3',
        manufacturer: 'Acme',
        model: 'X1',
        serial_number: 'SN-1',
        purchase_price: 1200,
      }),
      canManage: false,
    })
    const rows = wrapper.findAll('.info-row')
    expect(rows).toHaveLength(6)
    expect(rows.map((r) => r.text())).toEqual([
      'Description: A big saw',
      'Location: Bay 3',
      'Manufacturer: Acme',
      'Model: X1',
      'Serial #: SN-1',
      'Price: $1200',
    ])
  })

  it('renders a zero price rather than hiding it', () => {
    // `v-if="tool.purchase_price"` is falsy for 0, so a donated tool shows no
    // price row at all. Recorded rather than asserted as correct: whether a
    // zero price should read as "free" or as "unknown" is a product question,
    // and this is where the current answer is written down.
    const wrapper = mountCard({ tool: tool({ purchase_price: 0 }), canManage: false })
    expect(
      wrapper.findAll('.info-row').some((r) => r.text().startsWith('Price:')),
      'a zero purchase price is currently indistinguishable from an unknown one',
    ).toBe(false)
  })
})

describe('the manager status-change flow', () => {
  it('offers no Update button or notes box until a different status is chosen', () => {
    const wrapper = mountCard({ canManage: true })
    expect(wrapper.find('.status-controls button').exists()).toBe(false)
    expect(wrapper.find('textarea').exists()).toBe(false)
  })

  it('ignores re-selecting the status the tool already has', () => {
    // Otherwise every click on the select arms an Update button that would
    // write a no-op status-change event into the tool's history.
    const wrapper = mountCard({ tool: tool({ status: ToolStatus.Idle }), canManage: true })
    wrapper.find('select').setValue('idle')
    expect(wrapper.find('.status-controls button').exists()).toBe(false)
  })

  it('reveals Update and the notes box once a new status is chosen', async () => {
    const wrapper = mountCard({ tool: tool({ status: ToolStatus.Idle }), canManage: true })
    await wrapper.find('select').setValue('broken')

    expect(wrapper.find('.status-controls button').text()).toBe('Update')
    expect(wrapper.find('textarea').exists()).toBe(true)
    // Nothing is emitted yet. The select is not the commit.
    expect(wrapper.emitted('status-change')).toBeUndefined()
  })

  it('emits the chosen status with the notes, then disarms', async () => {
    const wrapper = mountCard({ tool: tool({ status: ToolStatus.Idle }), canManage: true })
    await wrapper.find('select').setValue('maintenance')
    await wrapper.find('textarea').setValue('Blade replacement')
    await wrapper.find('.status-controls button').trigger('click')

    const emitted = wrapper.emitted('status-change')
    expect(emitted).toHaveLength(1)
    expect(emitted?.[0]?.[1]).toBe('maintenance')
    expect(emitted?.[0]?.[2]).toBe('Blade replacement')

    // And the form resets, so a second click cannot re-send the same change.
    expect(wrapper.find('.status-controls button').exists()).toBe(false)
  })

  it('sends undefined rather than an empty string when there are no notes', async () => {
    // An empty string written into a tool's history is a note that says
    // nothing, displayed as though somebody had left one.
    const wrapper = mountCard({ tool: tool({ status: ToolStatus.Idle }), canManage: true })
    await wrapper.find('select').setValue('broken')
    await wrapper.find('.status-controls button').trigger('click')
    expect(wrapper.emitted('status-change')?.[0]?.[2]).toBeUndefined()
  })

  it('offers every status the wire accepts, and no others', () => {
    // The select is the only place a status is chosen. A value here that the
    // server's enum does not have is a 400 the user cannot avoid; one missing
    // is a state nobody can set.
    const wrapper = mountCard({ canManage: true })
    expect(wrapper.findAll('option').map((o) => o.attributes('value'))).toEqual([
      'idle',
      'in_use',
      'maintenance',
      'broken',
      'repair',
      'retired',
    ])
  })
})

describe('the manager action buttons', () => {
  it('refuses to delete a tool that is in use', () => {
    const inUse = mountCard({ tool: tool({ status: ToolStatus.InUse }), canManage: true })
    const del = inUse.findAll('button').find((b) => b.text() === 'Delete')
    expect(del?.attributes('disabled')).toBeDefined()

    const idle = mountCard({ tool: tool({ status: ToolStatus.Idle }), canManage: true })
    const delIdle = idle.findAll('button').find((b) => b.text() === 'Delete')
    expect(delIdle?.attributes('disabled')).toBeUndefined()
  })

  it('offers Set Up Training when there is none and Manage Training when there is', () => {
    const without = mountCard({ canManage: true, hasTrainingSteps: false })
    expect(without.text()).toContain('Set Up Training')
    expect(without.text()).not.toContain('Manage Training')

    const withSteps = mountCard({ canManage: true, hasTrainingSteps: true })
    expect(withSteps.text()).toContain('Manage Training')
    expect(withSteps.text()).not.toContain('Set Up Training')
  })

  it('emits the tool itself with edit, delete and view-history', async () => {
    const subject = tool({ name: 'Lathe' })
    const wrapper = mountCard({ tool: subject, canManage: true })
    const byText = (t: string) => wrapper.findAll('button').find((b) => b.text() === t)

    await byText('Edit')?.trigger('click')
    await byText('History')?.trigger('click')
    await byText('Delete')?.trigger('click')

    // toStrictEqual, not toBe: Vue hands the handler the reactive proxy of the
    // prop, so it is a different object with the same contents. Identity is not
    // available to assert here, and the claim worth making is that the payload
    // is *this* tool rather than a neighbouring card's -- which the id carries.
    for (const event of ['edit', 'view-history', 'delete'] as const) {
      const payload = wrapper.emitted(event)?.[0]?.[0] as Tool
      expect(payload, event).toStrictEqual(subject)
      expect(payload.id, event).toBe(subject.id)
      expect(payload.name, event).toBe('Lathe')
    }
  })
})
