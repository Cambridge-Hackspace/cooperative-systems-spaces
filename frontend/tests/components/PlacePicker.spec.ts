// Tier 2: PlacePicker.
//
// Eighty-five lines carrying four separate decisions: whether a null option is
// selectable, how special places are separated out, how the hierarchy is
// flattened into indented options, and the same ''-to-null conversion
// SchedulePicker makes. Each is invisible from the others in a running app.
//
// It also contains a defect its own comment describes as working. The comment
// on `flat` says a special place's children "show up here rooted at the
// special's id; render them at depth 0". They do not show up at all -- see
// `findings/` below. A comment that describes intended behavior beside code
// that does something else is worse than no comment, because it stops the next
// reader looking.

import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'

import PlacePicker from '@/components/PlacePicker.vue'
import type { Place } from '@/types'

function place(
  id: string,
  name: string,
  parent_id: string | null = null,
  is_special = false,
  place_type = 'Room'
): Place {
  return { id, name, parent_id, is_special, place_type } as unknown as Place
}

function picker(places: Place[], props: Record<string, unknown> = {}) {
  return mount(PlacePicker, { props: { modelValue: null, places, ...props } })
}

describe('the null option', () => {
  it('is selectable and labeled by default', () => {
    const first = picker([]).findAll('option')[0]
    expect(first.text()).toBe('— Unassigned —')
    expect(first.attributes('disabled')).toBeUndefined()
  })

  it('takes the label it is given', () => {
    expect(picker([], { nullLabel: '— Outside —' }).findAll('option')[0].text()).toBe('— Outside —')
  })

  it('becomes an unselectable prompt when null is not a legal value', () => {
    // The distinction matters: a disabled placeholder still occupies the first
    // slot so the select shows a prompt, while refusing to be chosen. Removing
    // it entirely would silently select the first real place instead.
    const first = picker([], { hideNull: true }).findAll('option')[0]
    expect(first.text()).toBe('— Choose a place —')
    expect(first.attributes('disabled')).toBeDefined()
  })
})

describe('special places', () => {
  it('are grouped separately and sorted by name', () => {
    const w = picker([
      place('s2', 'Outside', null, true),
      place('s1', 'Common Area', null, true),
      place('r1', 'Workshop'),
    ])
    const group = w.find('optgroup')
    expect(group.exists()).toBe(true)
    expect(group.attributes('label')).toBe('Special')
    expect(group.findAll('option').map((o) => o.text().replace(/\s+/g, ' ').trim())).toEqual([
      '★ Common Area · Room',
      '★ Outside · Room',
    ])
  })

  it('produce no group when there are none', () => {
    expect(
      picker([place('r1', 'Workshop')])
        .find('optgroup')
        .exists()
    ).toBe(false)
  })
})

describe('the hierarchy', () => {
  it('indents by depth and sorts siblings by name', () => {
    const w = picker([
      place('b1', 'Building A'),
      place('r2', 'Workshop', 'b1'),
      place('r1', 'Kitchen', 'b1'),
      place('s1', 'Lathe corner', 'r2'),
    ])
    const texts = w
      .findAll('option')
      .slice(1) // past the null option
      .map((o) => o.text().replace(/\s+/g, ' ').trim())

    expect(texts).toEqual([
      'Building A · Room',
      '— Kitchen · Room',
      '— Workshop · Room',
      '— — Lathe corner · Room',
    ])
  })
})

describe('findings', () => {
  it('drops the children of a special place entirely', () => {
    // A PINNED FINDING, not a passing behavior.
    //
    // `flat` builds `byParent` from the non-special places and then walks with
    // `visit(null, 0)`. A regular place whose parent is a SPECIAL place has
    // `parent_id = <special id>`, which is a key in `byParent` that nothing
    // ever visits -- so it is never emitted.
    //
    // The comment above that code says these children "show up here rooted at
    // the special's id; render them at depth 0 so the picker isn't confusing".
    // They do not show up. A place parented to Outside cannot be chosen at all,
    // and the only symptom is a name missing from a dropdown.
    //
    // If this assertion fails, somebody fixed it -- delete it and assert the
    // child IS offered. See TESTING.md, "Known defects".
    const w = picker([
      place('sp', 'Outside', null, true),
      place('p1', 'Parking Lot A', 'sp'),
      place('r1', 'Workshop'),
    ])
    const texts = w.findAll('option').map((o) => o.text())

    expect(texts.some((t) => t.includes('Workshop'))).toBe(true)
    expect(texts.some((t) => t.includes('Outside'))).toBe(true)
    expect(
      texts.some((t) => t.includes('Parking Lot A')),
      'PINNED FINDING: a place whose parent is a special place is absent from ' +
        'the picker, despite the comment in PlacePicker.vue saying it is ' +
        'rendered at depth 0. It cannot be selected at all.'
    ).toBe(false)
  })
})

describe('findings: markup', () => {
  it('puts a span inside an option, which HTML does not allow', () => {
    // A PINNED FINDING. `<option>` may contain text only -- no elements. The
    // template wraps the place type in `<span class="text-base-content/50">`,
    // so the class styles nothing: browsers keep the text and discard the
    // element. Vue's compiler says so out loud during any test that mounts
    // this component, which is how it was noticed.
    //
    // Harmless today and worth pinning rather than leaving: it is markup that
    // looks like it does something, and the next person to wonder why the type
    // is not greyed out will find this test instead of the compiler warning.
    const w = picker([place('r1', 'Workshop')])
    const option = w.findAll('option')[1]
    expect(
      option.find('span').exists(),
      'PINNED FINDING: a <span> inside an <option> is invalid HTML and the ' +
        'class on it has no effect. If this fails, the markup was fixed -- ' +
        'delete the assertion.'
    ).toBe(true)
  })
})

describe('what the picker emits', () => {
  it('emits null for the empty option rather than an empty string', async () => {
    const w = picker([place('r1', 'Workshop')], { modelValue: 'r1' })
    const select = w.find('select')
    select.element.value = ''
    await select.trigger('change')
    expect(w.emitted('update:modelValue')?.[0]?.[0]).toBeNull()
  })

  it('disables itself when there is nothing to choose', () => {
    expect(picker([]).find('select').attributes('disabled')).toBeDefined()
    expect(
      picker([place('r1', 'W')])
        .find('select')
        .attributes('disabled')
    ).toBeUndefined()
  })
})
