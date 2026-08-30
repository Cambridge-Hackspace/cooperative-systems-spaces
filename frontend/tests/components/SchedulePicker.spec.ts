// Tier 2: SchedulePicker.
//
// Thirty-four lines, and all of the risk is in one conversion. The select's
// empty option means "always", and the API wants `null` for that -- not `''`.
// A schedule id is a UUID column with a foreign key, so emitting the empty
// string instead would be rejected by the database rather than by the form,
// and the user would be told the server broke about a dropdown they left alone.
//
// The conversion runs both ways and both directions are asserted: `null` in has
// to select the empty option, and the empty option out has to emit `null`.
// Testing only one leaves a component that displays correctly and saves wrongly,
// or the reverse.

import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'

import SchedulePicker from '@/components/SchedulePicker.vue'
import type { Schedule } from '@/types'

const SCHEDULES = [
  { id: 'sched-1', name: 'Weekdays' },
  { id: 'sched-2', name: 'Weekends' },
] as unknown as Schedule[]

function picker(modelValue: string | null, disabled = false) {
  return mount(SchedulePicker, { props: { modelValue, schedules: SCHEDULES, disabled } })
}

describe('what the picker shows', () => {
  it('offers every schedule plus an explicit "always"', () => {
    const options = picker(null).findAll('option')
    // Length asserted, not just presence: an extra option is as wrong as a
    // missing one and only the count catches a duplicate.
    expect(options).toHaveLength(SCHEDULES.length + 1)
    expect(options[0].text()).toBe('— Always —')
    expect(options[0].attributes('value')).toBe('')
    expect(options.slice(1).map((o) => o.text())).toEqual(['Weekdays', 'Weekends'])
  })

  it('selects the empty option when the value is null', () => {
    // `modelValue ?? ''`. Without the fallback the select would have no matching
    // option and the browser would show the first one anyway -- looking correct
    // by accident, and diverging the moment a schedule is added above it.
    expect(picker(null).find('select').element.value).toBe('')
  })

  it('selects the schedule it was given', () => {
    expect(picker('sched-2').find('select').element.value).toBe('sched-2')
  })

  it('is disabled only when told to be', () => {
    expect(picker(null).find('select').attributes('disabled')).toBeUndefined()
    expect(picker(null, true).find('select').attributes('disabled')).toBeDefined()
  })
})

describe('what the picker emits', () => {
  it('emits null for "always", not an empty string', async () => {
    const w = picker('sched-1')
    const select = w.find('select')
    select.element.value = ''
    await select.trigger('change')

    const emitted = w.emitted('update:modelValue')
    expect(emitted).toHaveLength(1)
    expect(
      emitted?.[0]?.[0],
      "the empty option means 'always' and the column is a nullable foreign " +
        "key -- emitting '' sends an empty string where a UUID or null is " +
        'expected, and the failure surfaces from the database rather than the form'
    ).toBeNull()
  })

  it('emits the id when a schedule is chosen', async () => {
    const w = picker(null)
    const select = w.find('select')
    select.element.value = 'sched-2'
    await select.trigger('change')
    expect(w.emitted('update:modelValue')?.[0]?.[0]).toBe('sched-2')
  })
})
