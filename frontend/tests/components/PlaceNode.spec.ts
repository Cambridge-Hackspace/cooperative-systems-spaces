// Tier 2: PlaceNode.
//
// A recursive component with one piece of arithmetic in it, and the arithmetic
// is where the risk lives. `canAddChild` decides whether a place may contain
// another by finding its type's index in the configured vocabulary and asking
// whether a deeper one exists:
//
//     i >= 0 && i < config.types.length - 1
//
// Both bounds matter and neither is obvious. Off by one at the top and the
// deepest type offers to contain something the configuration has no name for;
// off by one at the bottom and a type that is not configured at all is treated
// as containable. The two ends are asserted separately because a single
// off-by-one fixes one and breaks the other.
//
// The recursion is asserted too. Events from a grandchild have to arrive at the
// top intact -- each level re-emits by hand, so a level that forgets to forward
// one breaks a button that works perfectly at every other depth.

import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'

import PlaceNode from '@/components/PlaceNode.vue'
import type { Place, PlaceConfig } from '@/types'

const CONFIG: PlaceConfig = { enabled: true, types: ['Building', 'Room', 'Spot'] }

// Every field of `Place`, not a cast. `as unknown as Place` would let the type
// drift out from under this fixture silently -- a component reading a field the
// fixture never sets sees `undefined`, and the test still compiles.
function place(id: string, name: string, place_type: string, is_special = false): Place {
  return {
    id,
    name,
    place_type,
    is_special,
    parent_id: null,
    description: null,
    external_id: null,
    created_at: '2026-01-15T12:00:00Z',
    updated_at: '2026-01-15T12:00:00Z',
  }
}

/** `childrenMap` is keyed by parent id, which is how the component walks down. */
function tree(pairs: Array<[string | null, Place[]]>) {
  return new Map<string | null, Place[]>(pairs)
}

describe('what a node shows', () => {
  it('names the place and its type', () => {
    const w = mount(PlaceNode, {
      props: { place: place('p1', 'Workshop', 'Room'), childrenMap: tree([]), config: CONFIG },
    })
    expect(w.text()).toContain('Workshop')
    expect(w.text()).toContain('Room')
  })

  it('marks a special place, and marks nothing on an ordinary one', () => {
    const special = mount(PlaceNode, {
      props: {
        place: place('p1', 'Outside', 'Outside', true),
        childrenMap: tree([]),
        config: CONFIG,
      },
    })
    expect(special.text()).toContain('special')
    expect(special.find('.badge-warning').exists()).toBe(true)

    const ordinary = mount(PlaceNode, {
      props: { place: place('p2', 'Workshop', 'Room'), childrenMap: tree([]), config: CONFIG },
    })
    expect(ordinary.text()).not.toContain('special')
    expect(ordinary.find('.badge-ghost').exists()).toBe(true)
  })
})

describe('whether a place may contain another', () => {
  const cases: Array<[string, string, PlaceConfig | null, boolean]> = [
    ['a type with a deeper one below it', 'Building', CONFIG, true],
    ['a type in the middle', 'Room', CONFIG, true],
    ['the deepest configured type', 'Spot', CONFIG, false],
    ['a type the configuration does not know', 'Nowhere', CONFIG, false],
    ['no configuration at all', 'Room', null, false],
  ]

  it.each(cases)('%s -> %s', (_why, type, config, expected) => {
    const w = mount(PlaceNode, {
      props: { place: place('p1', 'X', type), childrenMap: tree([]), config },
    })
    const addChild = w.findAll('button').filter((b) => b.text().includes('Child'))
    expect(addChild.length > 0, `expected canAddChild to be ${expected}`).toBe(expected)
  })
})

describe('recursion', () => {
  it('renders descendants at every depth', () => {
    const w = mount(PlaceNode, {
      props: {
        place: place('p1', 'Building A', 'Building'),
        childrenMap: tree([
          ['p1', [place('p2', 'Workshop', 'Room')]],
          ['p2', [place('p3', 'Lathe corner', 'Spot')]],
        ]),
        config: CONFIG,
      },
    })
    expect(w.text()).toContain('Building A')
    expect(w.text()).toContain('Workshop')
    expect(
      w.text(),
      'a grandchild was not rendered, so the recursion stops at one level'
    ).toContain('Lathe corner')
  })

  it('forwards a grandchild’s events to the top', async () => {
    const w = mount(PlaceNode, {
      props: {
        place: place('p1', 'Building A', 'Building'),
        childrenMap: tree([
          ['p1', [place('p2', 'Workshop', 'Room')]],
          ['p2', [place('p3', 'Lathe corner', 'Spot')]],
        ]),
        config: CONFIG,
      },
    })

    // The deepest Delete button belongs to the grandchild.
    const deletes = w.findAll('button').filter((b) => b.text() === 'Delete')
    expect(deletes.length).toBe(3)
    await deletes[deletes.length - 1].trigger('click')

    const emitted = w.emitted('delete')
    expect(
      emitted,
      'each level re-emits by hand, so an event raised two levels down and not ' +
        'forwarded leaves a button that works at depth one and does nothing deeper'
    ).toBeTruthy()
    expect((emitted?.[0]?.[0] as Place).id).toBe('p3')
  })
})
