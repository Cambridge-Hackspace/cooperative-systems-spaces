// Tier 2: FacilityGraph.
//
// Cytoscape is mocked, deliberately and not for speed. The component's own
// logic is a translation: places and doors in, a flat `ElementDefinition[]`
// out, plus a selection strip driven by the tap handlers it registers. Asking
// jsdom to lay out a real graph in a 0x0 container would test cytoscape, at
// the cost of not being able to assert the one thing this component decides.
//
// So the fake records what it was constructed with and hands back the handlers,
// and every assertion below is about the element list and the strip.
//
// What this spec does NOT prove: that cytoscape renders any of it, that the
// four layouts look different, or that a graph is legible. Tier 10 drives this
// view in a real browser; nothing here substitutes for that.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import type { Door, Place } from '@/types'

type ElementData = Record<string, unknown>
interface Element {
  data: ElementData
  classes?: string
}
interface TapEvent {
  target: unknown
}
type TapHandler = (evt: TapEvent) => void

// The fake core. `elements` and `layout` are recorded from the constructor
// options; `layoutRuns` accumulates every layout re-run so the select can be
// asserted without reading cytoscape's internals.
interface GraphRecorder {
  constructed: { elements: Element[]; layout: { name: string } }[]
  layoutRuns: { name: string }[]
  destroys: number
  handlers: { node: TapHandler | null; edge: TapHandler | null; background: TapHandler | null }
  core: unknown
}

const graph = vi.hoisted<GraphRecorder>(() => ({
  constructed: [],
  layoutRuns: [],
  destroys: 0,
  handlers: { node: null, edge: null, background: null },
  core: null,
}))

const cytoscapeMock = vi.hoisted(
  () =>
    function cytoscapeFake(opts: { elements: Element[]; layout: { name: string } }) {
      graph.constructed.push({ elements: opts.elements, layout: opts.layout })
      const core = {
        on(event: string, a: string | TapHandler, b?: TapHandler) {
          if (typeof a === 'function') graph.handlers.background = a
          else if (a === 'node') graph.handlers.node = b ?? null
          else if (a === 'edge') graph.handlers.edge = b ?? null
        },
        destroy() {
          graph.destroys++
        },
        layout(o: { name: string }) {
          return {
            run() {
              graph.layoutRuns.push(o)
            },
          }
        },
        // Resolves against the elements the component actually handed over, so
        // the edge handler's `${a} ↔ ${b}` subtitle is built from real labels
        // rather than from something this fake invented.
        getElementById(id: string) {
          const found = opts.elements.find((e) => e.data.id === id)
          return { data: (key: string) => found?.data[key] }
        },
      }
      graph.core = core
      return core
    }
)

const mocks = vi.hoisted(() => ({ listPlaces: vi.fn(), listDoors: vi.fn() }))

vi.mock('cytoscape', () => ({ default: cytoscapeMock }))
vi.mock('@/utils/api', () => ({
  placesApi: { list: mocks.listPlaces },
  doorsApi: { list: mocks.listDoors },
}))

import FacilityGraph from '@/components/FacilityGraph.vue'

function place(id: string, name: string, over: Partial<Place> = {}): Place {
  return {
    id,
    name,
    parent_id: null,
    place_type: 'Room',
    description: null,
    external_id: null,
    is_special: false,
    created_at: '2026-01-15T12:00:00Z',
    updated_at: '2026-01-15T12:00:00Z',
    ...over,
  }
}

function door(id: string, name: string, over: Partial<Door> = {}): Door {
  return {
    id,
    name,
    location: null,
    description: null,
    edge_device_id: null,
    unlock_duration_ms: 3000,
    enabled: true,
    created_at: '2026-01-15T12:00:00Z',
    updated_at: '2026-01-15T12:00:00Z',
    place_id_from: null,
    place_id_to: null,
    ...over,
  }
}

beforeEach(() => {
  graph.constructed.length = 0
  graph.layoutRuns.length = 0
  graph.destroys = 0
  graph.handlers = { node: null, edge: null, background: null }
  graph.core = null
  mocks.listPlaces.mockReset()
  mocks.listDoors.mockReset()
  mocks.listPlaces.mockResolvedValue({ success: true, data: [] })
  mocks.listDoors.mockResolvedValue({ success: true, data: [] })
})

async function graphWith(places: Place[], doors: Door[]) {
  mocks.listPlaces.mockResolvedValue({ success: true, data: places })
  mocks.listDoors.mockResolvedValue({ success: true, data: doors })
  const w = mount(FacilityGraph)
  await flushPromises()
  return w
}

const built = () => {
  const last = graph.constructed.at(-1)
  if (!last) throw new Error('cytoscape was never constructed')
  return last
}

const nodes = () => built().elements.filter((e) => !('source' in e.data))
const edges = () => built().elements.filter((e) => 'source' in e.data)

describe('when there is nothing to draw', () => {
  it('says so and does not build a graph at all', async () => {
    const w = await graphWith([], [])
    expect(w.text()).toContain('Nothing to graph yet')
    expect(graph.constructed).toHaveLength(0)
  })

  it('shows a spinner while the two requests are in flight', async () => {
    mocks.listPlaces.mockReturnValue(new Promise(() => {}))
    mocks.listDoors.mockReturnValue(new Promise(() => {}))
    const w = mount(FacilityGraph)
    // `loading` is set inside `load()`, which `onMounted` calls after the first
    // render, so the spinner is one tick away rather than present immediately.
    await nextTick()
    expect(w.find('.loading-spinner').exists()).toBe(true)
    expect(w.text()).not.toContain('Nothing to graph yet')
  })
})

describe('places become nodes', () => {
  it('gives every place one node carrying its name and type', async () => {
    await graphWith([place('p1', 'Workshop', { place_type: 'Room' })], [])
    expect(nodes()).toHaveLength(1)
    expect(nodes()[0].data).toEqual({
      id: 'p1',
      label: 'Workshop',
      kind: 'place',
      placeType: 'Room',
    })
    expect(nodes()[0].classes).toBe('place')
  })

  it('marks a special place so the stylesheet can colour it differently', async () => {
    await graphWith([place('p1', 'Outside', { is_special: true })], [])
    expect(nodes()[0].classes).toBe('place special')
  })

  it('draws a hierarchy edge from parent to child, and none for a root', async () => {
    await graphWith([place('p1', 'Building'), place('p2', 'Room', { parent_id: 'p1' })], [])
    expect(edges()).toHaveLength(1)
    expect(edges()[0].data).toEqual({ source: 'p1', target: 'p2', kind: 'hierarchy' })
    expect(edges()[0].classes).toBe('hierarchy')
  })
})

describe('doors become edges', () => {
  it('connects the two sides and records whether the door is enabled', async () => {
    await graphWith(
      [place('p1', 'Hall'), place('p2', 'Shop')],
      [door('d1', 'Shop door', { place_id_from: 'p1', place_id_to: 'p2' })]
    )
    const doorEdge = edges().find((e) => e.data.kind === 'door')
    expect(doorEdge?.data).toEqual({
      id: 'door-d1',
      source: 'p1',
      target: 'p2',
      label: 'Shop door',
      kind: 'door',
      enabled: true,
    })
    expect(doorEdge?.classes).toBe('door door-enabled')
  })

  it('classes a disabled door differently', async () => {
    await graphWith(
      [place('p1', 'Hall'), place('p2', 'Shop')],
      [door('d1', 'Shop door', { place_id_from: 'p1', place_id_to: 'p2', enabled: false })]
    )
    expect(edges().find((e) => e.data.kind === 'door')?.classes).toBe('door door-disabled')
  })

  it('gives a half-assigned door a placeholder to terminate on', async () => {
    await graphWith([place('p1', 'Hall')], [door('d1', 'Back door', { place_id_from: 'p1' })])
    const placeholder = nodes().find((n) => n.data.kind === 'unset')
    expect(placeholder?.data.label).toBe('(unset)')
    expect(edges().find((e) => e.data.kind === 'door')?.data.target).toBe(placeholder?.data.id)
  })

  it('gives a door with an unassigned FROM side a placeholder too', async () => {
    // Both sides, because the two are separate branches over separate
    // variables. A test that only ever leaves `place_id_to` null passes
    // whatever the `from` branch does.
    await graphWith([place('p1', 'Hall')], [door('d1', 'Back door', { place_id_to: 'p1' })])
    const placeholder = nodes().find((n) => n.data.kind === 'unset')
    expect(placeholder?.data.label).toBe('(unset)')
    expect(edges().find((e) => e.data.kind === 'door')?.data.source).toBe(placeholder?.data.id)
  })

  it('gives each unassigned side its own placeholder rather than sharing one', async () => {
    // Four loose ends across four doors, two on each side, so a shared
    // placeholder on either branch collapses the count. The counter makes them
    // distinct, and the graph shows four loose ends rather than one or two hubs
    // that falsely imply the doors meet somewhere.
    await graphWith(
      [place('p1', 'Hall')],
      [
        door('d1', 'Back door', { place_id_from: 'p1' }),
        door('d2', 'Side door', { place_id_from: 'p1' }),
        door('d3', 'Front door', { place_id_to: 'p1' }),
        door('d4', 'Fire door', { place_id_to: 'p1' }),
      ]
    )
    const ids = nodes()
      .filter((n) => n.data.kind === 'unset')
      .map((n) => n.data.id)
    expect(ids).toHaveLength(4)
    expect(new Set(ids).size).toBe(4)
  })

  // FINDING, pinned. A door with neither side assigned is `continue`d past, so
  // it is absent from the graph with nothing to say it was skipped. The blurb
  // above the graph promises that doors with "an unassigned side land on a
  // small (unset) placeholder" -- a door with two unassigned sides lands
  // nowhere, and an operator using this view to find unconfigured doors will
  // not see the least configured ones.
  it('drops a door with neither side assigned, silently', async () => {
    const w = await graphWith([place('p1', 'Hall')], [door('d1', 'Orphan door')])
    expect(
      edges().filter((e) => e.data.kind === 'door'),
      'a fully unassigned door now appears -- if that was fixed, delete this ' +
        'test; the guard is `if (!d.place_id_from && !d.place_id_to) continue`'
    ).toHaveLength(0)
    expect(w.text()).not.toContain('Orphan door')
  })
})

describe('the layout select', () => {
  it('starts on the force layout', async () => {
    await graphWith([place('p1', 'Hall')], [])
    expect(built().layout.name).toBe('cose')
  })

  it('re-runs the layout named by each option', async () => {
    // Exhaustive over the options the template offers, so an option added to
    // the select without a case in `layoutOptions` fails here rather than
    // silently falling through to `cose`.
    const w = await graphWith([place('p1', 'Hall')], [])
    const offered = w.findAll('select option').map((o) => o.attributes('value'))
    expect(offered).toEqual(['cose', 'breadthfirst', 'concentric', 'grid'])

    // Backwards, because `watch` only fires on a change and the select starts
    // on the first option -- walking forwards would silently assert nothing for
    // `cose`.
    for (const name of [...offered].reverse()) {
      await w.find('select').setValue(name)
      expect(graph.layoutRuns.at(-1)?.name).toBe(name)
    }
  })

  it('rebuilds the whole graph when Reload is pressed', async () => {
    const w = await graphWith([place('p1', 'Hall')], [])
    expect(graph.constructed).toHaveLength(1)

    await w
      .findAll('button')
      .find((b) => b.text().trim() === 'Reload')
      ?.trigger('click')
    await flushPromises()

    expect(mocks.listPlaces).toHaveBeenCalledTimes(2)
    expect(graph.constructed).toHaveLength(2)
    // The previous instance is torn down first; leaking it would leak its
    // listeners and its canvas.
    expect(graph.destroys).toBe(1)
  })

  it('destroys the graph when the component goes away', async () => {
    const w = await graphWith([place('p1', 'Hall')], [])
    w.unmount()
    expect(graph.destroys).toBe(1)
  })
})

describe('the selection strip', () => {
  const tapNode = (data: ElementData, classes: string[] = []) =>
    graph.handlers.node?.({
      target: { data: (k: string) => data[k], hasClass: (c: string) => classes.includes(c) },
    })

  const tapEdge = (data: ElementData) =>
    graph.handlers.edge?.({ target: { data: (k: string) => data[k] } })

  it('shows nothing until something is tapped', async () => {
    const w = await graphWith([place('p1', 'Hall')], [])
    expect(w.find('.card').exists()).toBe(false)
  })

  it('names an ordinary place and its type', async () => {
    const w = await graphWith([place('p1', 'Hall', { place_type: 'Corridor' })], [])
    tapNode({ kind: 'place', label: 'Hall', placeType: 'Corridor' })
    await flushPromises()

    expect(w.find('.card-body').text()).toContain('Hall')
    expect(w.find('.card-body').text()).toContain('Corridor')
    expect(w.find('.card-body .badge').text()).toBe('place')
    expect(w.find('.card-body .badge').classes()).toContain('badge-info')
  })

  it('marks a special place as special', async () => {
    const w = await graphWith([place('p1', 'Outside', { is_special: true })], [])
    tapNode({ kind: 'place', label: 'Outside', placeType: 'Zone' }, ['special'])
    await flushPromises()

    expect(w.find('.card-body .badge').text()).toBe('special')
    expect(w.find('.card-body .badge').classes()).toContain('badge-warning')
  })

  it('explains what a placeholder node is', async () => {
    const w = await graphWith([place('p1', 'Hall')], [door('d1', 'Back', { place_id_from: 'p1' })])
    tapNode({ kind: 'unset', label: '(unset)' })
    await flushPromises()

    expect(w.find('.card-body').text()).toContain('door side not assigned to any place')
    expect(w.find('.card-body .badge').text()).toBe('placeholder')
  })

  it('reads a door edge back as both its ends and its state', async () => {
    const w = await graphWith(
      [place('p1', 'Hall'), place('p2', 'Shop')],
      [door('d1', 'Shop door', { place_id_from: 'p1', place_id_to: 'p2' })]
    )
    tapEdge({ kind: 'door', label: 'Shop door', source: 'p1', target: 'p2', enabled: true })
    await flushPromises()

    expect(w.find('.card-body').text()).toContain('Shop door')
    expect(w.find('.card-body').text()).toContain('Hall ↔ Shop · enabled')
    expect(w.find('.card-body .badge').classes()).toContain('badge-success')
  })

  it('says so when the door it read back is disabled', async () => {
    const w = await graphWith(
      [place('p1', 'Hall'), place('p2', 'Shop')],
      [door('d1', 'Shop door', { place_id_from: 'p1', place_id_to: 'p2', enabled: false })]
    )
    tapEdge({ kind: 'door', label: 'Shop door', source: 'p1', target: 'p2', enabled: false })
    await flushPromises()

    expect(w.find('.card-body').text()).toContain('Hall ↔ Shop · disabled')
    expect(w.find('.card-body .badge').classes()).toContain('badge-neutral')
  })

  it('reads a hierarchy edge back as a parent relationship', async () => {
    const w = await graphWith(
      [place('p1', 'Building'), place('p2', 'Room', { parent_id: 'p1' })],
      []
    )
    tapEdge({ kind: 'hierarchy', source: 'p1', target: 'p2' })
    await flushPromises()

    expect(w.find('.card-body').text()).toContain('Building → Room')
    expect(w.find('.card-body .badge').text()).toBe('parent')
  })

  it('clears the strip when the background is tapped', async () => {
    const w = await graphWith([place('p1', 'Hall')], [])
    tapNode({ kind: 'place', label: 'Hall', placeType: 'Room' })
    await flushPromises()
    expect(w.find('.card').exists()).toBe(true)

    graph.handlers.background?.({ target: graph.core })
    await flushPromises()
    expect(w.find('.card').exists()).toBe(false)
  })
})

describe('what happens when the server does not cooperate', () => {
  // FINDING, pinned. `if (p.success && p.data)` and nothing else. A response
  // that reports failure leaves the previous data in place and says nothing,
  // so pressing Reload against a server that has started refusing shows a
  // graph that is quietly out of date.
  it('keeps the stale graph and says nothing when a reload is refused', async () => {
    const w = await graphWith([place('p1', 'Hall')], [])
    expect(nodes()[0].data.label).toBe('Hall')

    mocks.listPlaces.mockResolvedValue({ success: false, error: 'Forbidden' })
    mocks.listDoors.mockResolvedValue({ success: false, error: 'Forbidden' })
    await w
      .findAll('button')
      .find((b) => b.text().trim() === 'Reload')
      ?.trigger('click')
    await flushPromises()

    // The graph really is rebuilt -- asserting only on `built()` would read the
    // *first* construction and pass just as well if the refusal had emptied the
    // view and skipped the rebuild entirely.
    expect(graph.constructed).toHaveLength(2)
    expect(nodes()[0].data.label).toBe('Hall')
    expect(
      w.text(),
      'the component now reports a refused load -- if that was fixed, this ' +
        'test should assert the message instead of its absence'
    ).not.toContain('Forbidden')
  })

  // FINDING, pinned, and the worst of the three. `load()` has no try/catch and
  // `loading.value = false` sits after the await, so a rejected request strands
  // the spinner forever: no error state, no retry prompt, nothing but a
  // spinner that never stops.
  //
  // The rejection escapes the component entirely. `onMounted(load)` hands Vue
  // an async function, so Vue routes the rejection to `app.config.errorHandler`
  // -- and `src/main.ts` sets none, which means in production it lands in the
  // browser console and nowhere else. That is also why `no-floating-promises`
  // never saw it: `onMounted(load)` passes a reference rather than calling it.
  //
  // The handler is installed on this mount rather than in `tests/setup.ts`,
  // per the note there: a warning that is genuinely expected is declared by the
  // test that provokes it. Without one, Vue's own "Unhandled error during
  // execution of mounted hook" warning fails this test -- which is the harness
  // working, not a reason to widen it.
  it('spins forever when a request rejects, and the rejection leaves the app', async () => {
    const escaped: unknown[] = []
    mocks.listPlaces.mockRejectedValue(new Error('Network Error'))

    const w = mount(FacilityGraph, {
      global: { config: { errorHandler: (err: unknown) => escaped.push(err) } },
    })
    await flushPromises()

    expect(
      w.find('.loading-spinner').exists(),
      'the spinner now clears after a rejected load -- if a failure state ' +
        'was added, this test should assert it; `load()` has no try/catch ' +
        'and sets `loading = false` only after the await'
    ).toBe(true)
    expect(w.text()).not.toContain('Nothing to graph yet')
    expect(graph.constructed).toHaveLength(0)
    expect(
      escaped,
      'the rejection is now handled inside the component -- if a try/catch was ' +
        'added, delete this expectation and assert what the user is shown'
    ).toHaveLength(1)
  })
})
