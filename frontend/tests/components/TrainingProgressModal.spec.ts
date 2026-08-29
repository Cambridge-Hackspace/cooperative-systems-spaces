// Tier 2: TrainingProgressModal.
//
// Six hundred and ninety-four lines, and none of them can run against data.
// `loadProgress` fetches nothing:
//
//     // In a real implementation, you'd have an endpoint to get all user
//     // progress for a training step. For now, we'll simulate this
//     error.value = 'Training progress viewing is not fully implemented yet. ...'
//
// `allProgress` is therefore always empty, and everything built on it --
// `filteredProgress`, `paginatedProgress`, `totalPages`, the four stat
// counters, the status filter, the search box, the pagination, and the whole
// progress-row template with its badges, dates and scores -- is unreachable.
//
// So this spec is short, and that is the finding rather than a gap in it. What
// can be asserted is the shell: what an operator is shown, in what order, and
// that the machinery below is genuinely dead. `formatStatus`,
// `getStatusBadgeClass` and `isExpired` cannot be exercised from outside at
// all, because reaching them requires a populated list; they would be Tier 1
// material the day they are extracted or the endpoint exists.
//
// What this spec does NOT prove: anything about training progress. There is no
// endpoint to prove it against.

import { describe, expect, it } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'

import TrainingProgressModal from '@/components/TrainingProgressModal.vue'
import { AssessmentType, TrainingStatus, type TrainingStep } from '@/types/training'

function step(over: Partial<TrainingStep> = {}): TrainingStep {
  return {
    id: 'step-1',
    tool_id: 'tool-1',
    step_number: 1,
    step_name: 'Lathe safety',
    description: 'Guards, speeds, and what not to wear.',
    assessment_type: AssessmentType.Practical,
    is_active: true,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...over,
  }
}

async function modal(s: TrainingStep | null = step()) {
  const w = mount(TrainingProgressModal, { props: { step: s } })
  await flushPromises()
  return w
}

describe('what an operator is actually shown', () => {
  // FINDING, pinned. The empty state renders *above* the not-implemented
  // notice, so the first thing on screen is a statement of fact about the
  // training step -- "No users have started this training step yet." -- and
  // the disclaimer that nothing was loaded is underneath it.
  //
  // A step that fifty people have attempted looks identical to one nobody has
  // touched, and the sentence that would explain why is below the fold of the
  // claim it contradicts.
  it('claims nobody has started the step, then explains underneath that it did not look', async () => {
    const w = await modal()

    expect(w.find('.empty-state').text()).toContain('No users have started this training step yet.')
    expect(w.find('.error-message').text()).toContain('not fully implemented yet')

    // Order matters here: the claim comes first.
    const html = w.html()
    expect(
      html.indexOf('No users have started'),
      'the notice now precedes the empty state -- if the order was changed, ' +
        'this assertion should follow it'
    ).toBeLessThan(html.indexOf('not fully implemented'))
  })

  it('reports every statistic as zero', async () => {
    const w = await modal()
    const numbers = w.findAll('.stat-number').map((n) => n.text())
    expect(numbers.length).toBeGreaterThan(0)
    expect(
      numbers.every((n) => n === '0'),
      `stats read ${numbers.join(', ')}`
    ).toBe(true)
  })

  it('offers filters that can never match anything', async () => {
    const w = await modal()
    expect(w.findAll('select option').length).toBeGreaterThan(1)
    expect(w.find('.search-input').exists()).toBe(true)
    expect(w.findAll('.progress-list').length).toBe(0)
  })

  it('shows no pagination, because there is never a second page', async () => {
    expect((await modal()).find('.pagination').exists()).toBe(false)
  })

  it('never renders a progress row', async () => {
    // The whole row template -- badges, dates, assessment scores, instructor,
    // notes -- hangs off `paginatedProgress`, which is derived from a list
    // nothing populates.
    const w = await modal()
    expect(
      w.find('.progress-list').exists(),
      'a progress row now renders -- if the endpoint arrived, this spec needs ' +
        'rewriting around it rather than extending'
    ).toBe(false)
  })

  // `loadProgress` also opens with `if (!props.step) return`, and that guard is
  // unreachable: both of its callers -- the watcher and `onMounted` -- check
  // the same condition before calling. Three guards for one question, two of
  // which are the ones that fire. An equivalent mutant, recorded rather than
  // chased.
  it('does nothing at all without a step', async () => {
    const w = await modal(null)
    expect(w.find('.error-message').exists()).toBe(false)
    expect(w.find('.empty-state').exists()).toBe(true)
  })

  it('names the step it was opened for', async () => {
    const w = await modal(step({ step_name: 'Lathe safety' }))
    expect(w.find('.modal-header').text()).toContain('Lathe safety')
  })
})

describe('the status filter it offers', () => {
  // Pinned as a list rather than compared to the enum, because the filter
  // offers exactly the five `TrainingStatus` values and the day the endpoint
  // exists this is the assertion that would catch a sixth being added without
  // a way to filter for it -- the same shape as the audit-log filter gap.
  it('offers every status the enum defines', async () => {
    const w = await modal()
    const offered = w
      .findAll('select option')
      .map((o) => o.attributes('value'))
      .filter((v) => v !== '')

    expect([...offered].sort()).toEqual([...Object.values(TrainingStatus)].sort())
  })
})

describe('reloading', () => {
  // Recorded, NOT asserted, and the distinction matters: `watch(...,
  // { immediate: true })` and `onMounted` both call `loadProgress`, so it runs
  // twice per open -- the same duplicate PrerequisitesModal has, where it
  // costs two HTTP requests. Here it costs nothing and, more to the point,
  // cannot be observed from outside: the loader's only effect is to set a
  // constant string, so both calls are indistinguishable from one. There is
  // nothing to spy on and no assertion that would fail if the duplicate were
  // removed. Writing one anyway would be a test that passes either way.
  it('re-runs when the step changes, and does not clear the notice first', async () => {
    const w = await modal()
    expect(w.find('.error-message').exists()).toBe(true)

    await w.setProps({ step: step({ id: 'step-2' }) })
    await nextTick()
    expect(w.find('.error-message').exists()).toBe(true)
  })
})

describe('closing', () => {
  it('closes on the overlay and the header button', async () => {
    const w = await modal()
    await w.find('.modal-overlay').trigger('click')
    await w.find('.close-btn').trigger('click')
    expect(w.emitted('close')).toHaveLength(2)
  })

  it('does not close when the content is clicked', async () => {
    const w = await modal()
    await w.find('.modal-content').trigger('click')
    expect(w.emitted('close')).toBeUndefined()
  })
})
