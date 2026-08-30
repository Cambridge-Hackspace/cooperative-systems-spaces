// Tier 2: HomeLinkManagement.
//
// A CRUD table plus a modal. Two things make it worth reading carefully.
//
// First, it gets the expiry right where EditTrainerModal gets it wrong: it
// converts the `datetime-local` value through the *local* getters, and it sends
// an explicit `null` to clear. Those are asserted here as behavior worth
// keeping, not just as coverage.
//
// Second, the URL is free text end to end. This component does not check it,
// and `api/home_links.rs:239` checks only that it is non-empty -- while
// `HomeView.vue:85` renders it as `:href="l.url"` on the *public* home page.
// Vue does not sanitise an href binding. That is the finding below.
//
// What this spec does NOT prove: that HomeView renders anything. It is a view,
// not a component in this tier's inventory, and the rendering half of that
// finding is recorded rather than asserted here.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const mocks = vi.hoisted(() => ({
  list: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  remove: vi.fn(),
}))
vi.mock('@/utils/api', () => ({ homeLinksApi: mocks }))

import HomeLinkManagement from '@/components/HomeLinkManagement.vue'
import type { HomeLink, HomeLinkAudience } from '@/types'

// Every audience the type admits. Both lookup tables in the component are bare
// object indexes with no fallback, so a value outside this set renders an empty
// badge with `class="badge undefined"`. Keeping the list derived from one place
// means adding an audience breaks here rather than in production.
const AUDIENCES: HomeLinkAudience[] = ['everyone', 'anonymous', 'logged_in', 'member', 'staff']

function link(over: Partial<HomeLink> = {}): HomeLink {
  return {
    id: 'l1',
    label: 'Wiki',
    url: 'https://wiki.example.org',
    description: null,
    icon: null,
    audience: 'everyone',
    sort_order: 10,
    enabled: true,
    created_at: '2025-06-01T00:00:00Z',
    updated_at: '2025-06-01T00:00:00Z',
    expires_at: null,
    ...over,
  }
}

const stubs = { 'router-link': { props: ['to'], template: '<a><slot /></a>' } }

let confirmResult = true

beforeEach(() => {
  for (const m of Object.values(mocks)) m.mockReset()
  mocks.list.mockResolvedValue({ success: true, data: [] })
  mocks.create.mockResolvedValue({ success: true })
  mocks.update.mockResolvedValue({ success: true })
  mocks.remove.mockResolvedValue({ success: true })
  confirmResult = true
  vi.stubGlobal(
    'confirm',
    vi.fn(() => confirmResult)
  )
})

async function page(links: HomeLink[] = []) {
  mocks.list.mockResolvedValue({ success: true, data: links })
  const w = mount(HomeLinkManagement, { global: { stubs } })
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof page>>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labeled ${JSON.stringify(label)}`)
  return b
}

const body = () => mocks.create.mock.calls[0][0] as Record<string, unknown>
const patched = () => mocks.update.mock.calls[0][1] as Record<string, unknown>

async function openForm(w: Wrapper) {
  await buttonNamed(w, '+ New link').trigger('click')
  await nextTick()
}

async function fill(w: Wrapper, label: string, url: string) {
  const inputs = w.findAll('.modal-box input[type="text"]')
  await inputs[0].setValue(label)
  await inputs[1].setValue(url)
}

describe('the list', () => {
  it('spins, then says there is nothing yet', async () => {
    const w = await page([])
    expect(w.text()).toContain('No homepage links yet')
    expect(w.find('table').exists()).toBe(false)
  })

  it('renders one row per link with its order, label and URL', async () => {
    const w = await page([link(), link({ id: 'l2', label: 'Calendar', sort_order: 20 })])
    const rows = w.findAll('tbody tr')

    expect(rows).toHaveLength(2)
    expect(rows[0].text()).toContain('Wiki')
    expect(rows[0].text()).toContain('https://wiki.example.org')
    expect(rows[0].findAll('td')[0].text()).toBe('10')
  })

  it('shows the icon and the description when they are set', async () => {
    const w = await page([link({ icon: '📚', description: 'Build notes' })])
    expect(w.find('tbody tr').text()).toContain('📚')
    expect(w.find('tbody tr').text()).toContain('Build notes')
  })

  it('has a label and a badge class for every audience the type admits', async () => {
    // Exhaustive, because both lookups are bare object indexes with no
    // fallback: an unmapped audience yields `undefined` for the text and
    // `class="badge undefined"` for the styling, and neither fails loudly.
    const w = await page(AUDIENCES.map((a, i) => link({ id: `l${i}`, audience: a })))

    for (const row of w.findAll('tbody tr')) {
      const badge = row.findAll('.badge')[0]
      expect(badge.text(), 'an audience with no label').not.toBe('')
      expect(badge.classes().join(' ')).not.toContain('undefined')
    }
  })

  it('distinguishes an enabled link from a disabled one', async () => {
    const w = await page([link(), link({ id: 'l2', enabled: false })])
    expect(w.findAll('tbody tr')[0].text()).toContain('Enabled')
    expect(w.findAll('tbody tr')[1].text()).toContain('Disabled')
  })
})

describe('how an expiry is shown', () => {
  // The clock is frozen at 2026-01-15T12:00:00Z and the zone is
  // America/Chicago; see tests/unit/suite-environment.spec.ts.
  it('shows a dash when there is none', async () => {
    expect((await page([link({ expires_at: null })])).find('tbody tr').text()).toContain('—')
  })

  it('marks one already in the past as expired', async () => {
    const w = await page([link({ expires_at: '2026-01-14T12:00:00Z' })])
    expect(w.find('tbody tr .badge-error').text()).toBe('Expired')
  })

  it('counts down in minutes, hours and days', async () => {
    const w = await page([
      link({ id: 'a', expires_at: '2026-01-15T12:30:00Z' }),
      link({ id: 'b', expires_at: '2026-01-15T20:00:00Z' }),
      link({ id: 'c', expires_at: '2026-01-19T12:00:00Z' }),
    ])
    const rows = w.findAll('tbody tr')
    expect(rows[0].text()).toContain('in 30m')
    expect(rows[1].text()).toContain('in 8h')
    expect(rows[2].text()).toContain('in 4d')
  })

  it('carries the absolute time as a tooltip on the relative one', async () => {
    const w = await page([link({ expires_at: '2026-01-19T12:00:00Z' })])
    expect(w.find('tbody tr [title]').attributes('title')).toBe(
      new Date('2026-01-19T12:00:00Z').toLocaleString()
    )
  })
})

describe('creating a link', () => {
  it('will not save without both a label and a URL', async () => {
    const w = await page()
    await openForm(w)
    expect(buttonNamed(w, 'Create').attributes('disabled')).toBeDefined()

    await fill(w, '  ', '  ')
    expect(buttonNamed(w, 'Create').attributes('disabled')).toBeDefined()

    await fill(w, 'Wiki', 'https://wiki.example.org')
    expect(buttonNamed(w, 'Create').attributes('disabled')).toBeUndefined()
  })

  it('trims what it sends and nulls what is blank', async () => {
    const w = await page()
    await openForm(w)
    await fill(w, '  Wiki  ', '  https://wiki.example.org  ')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(body().label).toBe('Wiki')
    expect(body().url).toBe('https://wiki.example.org')
    expect(body().description).toBeNull()
    expect(body().icon).toBeNull()
    expect(body().expires_at).toBeNull()
    expect(body().audience).toBe('everyone')
  })

  it('suggests the next sort order rather than colliding with an existing one', async () => {
    const w = await page([link({ sort_order: 10 }), link({ id: 'l2', sort_order: 40 })])
    await openForm(w)
    expect((w.find('.modal-box input[type="number"]').element as HTMLInputElement).value).toBe('50')
  })

  it('starts at zero when there is nothing to follow', async () => {
    const w = await page()
    await openForm(w)
    expect((w.find('.modal-box input[type="number"]').element as HTMLInputElement).value).toBe('0')
  })

  it('reloads the list and reports success', async () => {
    const w = await page()
    await openForm(w)
    await fill(w, 'Wiki', 'https://wiki.example.org')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(w.find('.alert-success').text()).toContain('Link created')
    expect(mocks.list).toHaveBeenCalledTimes(2)
    expect(w.find('.modal-open').exists()).toBe(false)
  })

  it("reports the server's reason and keeps the form open on a refusal", async () => {
    mocks.create.mockResolvedValue({ success: false, error: 'That label is taken' })
    const w = await page()
    await openForm(w)
    await fill(w, 'Wiki', 'https://wiki.example.org')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('That label is taken')
    expect(w.find('.modal-open').exists()).toBe(true)
  })

  // FIXED. The URL is rendered as `:href` on the public home page and Vue does
  // not sanitise an href binding, so a `javascript:` URL saved here became a
  // live script handler for every signed-out visitor. Neither this form nor
  // `api/home_links.rs:239` -- which checks only that the string is non-empty
  // -- stopped one. There is an allowlist now; see lib/urls.ts for why it is an
  // allowlist rather than a denylist.
  it('refuses a javascript: URL, and says what is permitted', async () => {
    const w = await page()
    await openForm(w)
    await fill(w, 'Free money', 'javascript:fetch("/api/v1/users").then(r=>r.json())')
    await buttonNamed(w, 'Create').trigger('click')
    await flushPromises()

    expect(mocks.create).not.toHaveBeenCalled()
    expect(w.find('.alert-error').text()).toContain('http:')
    expect(w.find('.modal-open').exists()).toBe(true)
  })

  it('still accepts an ordinary destination and a path on this site', async () => {
    for (const url of ['https://wiki.example.org', '/tools']) {
      mocks.create.mockClear()
      const w = await page()
      await openForm(w)
      await fill(w, 'Wiki', url)
      await buttonNamed(w, 'Create').trigger('click')
      await flushPromises()

      expect((mocks.create.mock.calls[0][0] as { url: string }).url, url).toBe(url)
    }
  })
})

describe('editing a link', () => {
  it('loads the row into the form, including the expiry in local time', async () => {
    // 2026-02-01T15:30Z is 09:30 on the 1st in America/Chicago, and the field
    // is a `datetime-local` -- so the local rendering is the correct one. This
    // is the conversion EditTrainerModal gets wrong by reaching for
    // `toISOString()`.
    const w = await page([link({ label: 'Wiki', expires_at: '2026-02-01T15:30:00Z' })])
    await buttonNamed(w, 'Edit').trigger('click')
    await nextTick()

    const inputs = w.findAll('.modal-box input[type="text"]')
    expect((inputs[0].element as HTMLInputElement).value).toBe('Wiki')
    expect((w.find('input[type="datetime-local"]').element as HTMLInputElement).value).toBe(
      '2026-02-01T09:30'
    )
    expect(w.text()).toContain('Edit homepage link')
  })

  it('round-trips a local expiry back to UTC on save', async () => {
    const w = await page([link()])
    await buttonNamed(w, 'Edit').trigger('click')
    await nextTick()
    await w.find('input[type="datetime-local"]').setValue('2026-02-01T09:30')
    await buttonNamed(w, 'Save').trigger('click')
    await flushPromises()

    expect(patched().expires_at).toBe('2026-02-01T15:30:00.000Z')
  })

  // Worth asserting as behavior to keep: this component reaches the "clear it"
  // state that EditTrainerModal cannot, by sending an explicit null.
  it('sends an explicit null to clear an expiry, rather than omitting the field', async () => {
    const w = await page([link({ expires_at: '2026-02-01T15:30:00Z' })])
    await buttonNamed(w, 'Edit').trigger('click')
    await nextTick()
    await buttonNamed(w, 'Clear').trigger('click')
    await buttonNamed(w, 'Save').trigger('click')
    await flushPromises()

    expect(patched().expires_at).toBeNull()
    expect(JSON.parse(JSON.stringify(patched()))).toHaveProperty('expires_at', null)
  })

  it('addresses the row it was opened on', async () => {
    const w = await page([link({ id: 'l1' }), link({ id: 'l2', label: 'Calendar' })])
    await w.findAll('tbody tr')[1].findAll('button')[0].trigger('click')
    await nextTick()
    await buttonNamed(w, 'Save').trigger('click')
    await flushPromises()

    expect(mocks.update.mock.calls[0][0]).toBe('l2')
  })

  it('abandons the edit on Cancel without calling the server', async () => {
    const w = await page([link()])
    await buttonNamed(w, 'Edit').trigger('click')
    await nextTick()
    await buttonNamed(w, 'Cancel').trigger('click')
    await nextTick()

    expect(w.find('.modal-open').exists()).toBe(false)
    expect(mocks.update).not.toHaveBeenCalled()
  })
})

describe('deleting a link', () => {
  it('names the link in the confirmation, and does nothing if declined', async () => {
    confirmResult = false
    const w = await page([link({ label: 'Wiki' })])
    await buttonNamed(w, 'Delete').trigger('click')
    await flushPromises()

    expect(vi.mocked(globalThis.confirm)).toHaveBeenCalledWith('Delete "Wiki"?')
    expect(mocks.remove).not.toHaveBeenCalled()
  })

  it('removes it and reloads', async () => {
    const w = await page([link({ id: 'l1' })])
    await buttonNamed(w, 'Delete').trigger('click')
    await flushPromises()

    expect(mocks.remove).toHaveBeenCalledWith('l1')
    expect(w.find('.alert-success').text()).toContain('Link deleted')
    expect(mocks.list).toHaveBeenCalledTimes(2)
  })

  it('reports a refused delete', async () => {
    mocks.remove.mockResolvedValue({ success: false, error: 'Still referenced' })
    const w = await page([link()])
    await buttonNamed(w, 'Delete').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('Still referenced')
  })
})

describe('what a network error does', () => {
  // FIXED. `load()` had no try/catch and cleared `loading` only after the
  // await, so a rejected list spun forever and the rejection escaped to an
  // `app.config.errorHandler` that `src/main.ts` never sets.
  it('reports a rejected list and stops spinning', async () => {
    mocks.list.mockRejectedValue(new Error('Network Error'))
    const w = mount(HomeLinkManagement, { global: { stubs } })
    await flushPromises()

    expect(w.find('.loading-spinner').exists()).toBe(false)
    expect(w.find('.alert-error').text()).toContain('Network Error')
  })

  it('reports a refused list rather than showing an empty one', async () => {
    mocks.list.mockResolvedValue({ success: false, error: 'Forbidden' })
    const w = mount(HomeLinkManagement, { global: { stubs } })
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('Forbidden')
  })

  // FIXED. `save()` set `saving = true` with no `finally`, so a rejection left
  // the button disabled and the modal open with no way to retry.
  it('frees the save button and reports the failure when the save rejects', async () => {
    mocks.create.mockRejectedValue(new Error('Network Error'))
    const w = mount(HomeLinkManagement, { global: { stubs } })
    await flushPromises()
    await openForm(w)
    await fill(w, 'Wiki', 'https://wiki.example.org')
    await w.find('.modal-action .btn-primary').trigger('click')
    await flushPromises()

    expect(w.find('.modal-action .btn-primary').attributes('disabled')).toBeUndefined()
    expect(w.find('.modal-open').exists()).toBe(true)
    expect(w.find('.alert-error').text()).toContain('Network Error')
  })
})

describe('the flash message', () => {
  it('clears itself after five seconds', async () => {
    const w = await page([link()])
    await buttonNamed(w, 'Delete').trigger('click')
    await flushPromises()
    expect(w.find('.alert-success').exists()).toBe(true)

    vi.advanceTimersByTime(5000)
    await nextTick()
    expect(w.find('.alert-success').exists()).toBe(false)
  })
})
