// Tier 2: component conformance for ProfileField.
//
// This component is the whole profile UI. Eight field types, each with its own
// element, its own class string and its own value handling, chosen by a
// `v-if`/`v-else-if` chain that ends in a `v-else` textarea — which means every
// mistake in the chain has the same symptom: the field renders as a textarea
// and nobody notices, because a textarea looks like a perfectly reasonable
// thing for a form to contain.
//
// So the assertions are exact. The element's tag, its `type` attribute and its
// full class string, per type, because "an input rendered" is satisfied by the
// wrong input and "it has the input class" is satisfied by six of the eight.
//
// THE TIER BOUNDARY. Nothing here asserts anything that needs real CSS. jsdom
// runs no cascade, so `getComputedStyle` returns the declarations on the
// element and not the result of the stylesheet — an assertion about what
// `input-error` *looks* like would pass against a stylesheet that does not
// define it. Class names are structure; colours and layout are Tier 5's.
//
// WHAT THIS DOES NOT PROVE. That the daisyUI classes used here exist. A typo
// in `input-bordred` is asserted faithfully by these tests and renders as an
// unstyled box in a browser. That belongs to the live-browser tier.

import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'

import ProfileField from '@/components/ProfileField.vue'
import { ProfileFieldType, type ProfileField as Field } from '@/types'

function field(overrides: Partial<Field> = {}): Field {
  return {
    key: 'test_field',
    label: 'Test Field',
    field_type: ProfileFieldType.Text,
    required: false,
    ...overrides,
  }
}

function mountField(f: Field, modelValue: unknown = '', props: Record<string, unknown> = {}) {
  return mount(ProfileField, { props: { field: f, modelValue, ...props } })
}

describe('the control rendered for each field type', () => {
  // The table is written out rather than derived from the component, and that
  // is the point: a table read out of the template would agree with the
  // template however the template changed.
  const CONTROLS: Array<[ProfileFieldType, string, string | null, string]> = [
    [ProfileFieldType.Text, 'input', 'text', 'input input-bordered w-full'],
    [ProfileFieldType.Email, 'input', 'email', 'input input-bordered w-full'],
    [ProfileFieldType.Phone, 'input', 'tel', 'input input-bordered w-full'],
    [ProfileFieldType.Number, 'input', 'number', 'input input-bordered w-full'],
    [ProfileFieldType.Date, 'input', 'date', 'input input-bordered w-full'],
    [ProfileFieldType.Boolean, 'input[type="checkbox"]', 'checkbox', 'checkbox'],
    [ProfileFieldType.Select, 'select', null, 'select select-bordered w-full'],
  ]

  it.each(CONTROLS)('%s renders a %s', (type, selector, inputType, classes) => {
    const f =
      type === ProfileFieldType.Select
        ? field({ field_type: { Select: { options: ['a', 'b'] } } })
        : field({ field_type: type })

    const wrapper = mountField(f, type === ProfileFieldType.Boolean ? false : '')
    const el = wrapper.find(selector)

    expect(el.exists()).toBe(true)
    if (inputType !== null && selector === 'input') {
      expect(el.attributes('type')).toBe(inputType)
    }
    expect(el.attributes('class')).toBe(classes)
  })

  it('falls through to a textarea only for a type it does not know', () => {
    // The v-else arm. Every mistake in the chain above lands here, so this is
    // the assertion that keeps "it rendered a textarea" from being an
    // acceptable answer for seven of the eight types.
    const wrapper = mountField(field({ field_type: 'Markdown' as ProfileFieldType }))
    expect(wrapper.find('textarea').exists()).toBe(true)
    expect(wrapper.find('textarea').attributes('class')).toBe(
      'textarea textarea-bordered w-full h-24',
    )

    // And a known type never reaches it.
    for (const type of [
      ProfileFieldType.Text,
      ProfileFieldType.Email,
      ProfileFieldType.Phone,
      ProfileFieldType.Number,
      ProfileFieldType.Date,
      ProfileFieldType.Boolean,
    ]) {
      const w = mountField(field({ field_type: type }), type === ProfileFieldType.Boolean ? false : '')
      expect(w.find('textarea').exists(), `${type} rendered a textarea`).toBe(false)
    }
  })

  it('renders exactly one control, never two', () => {
    // A v-if chain that became a series of independent v-ifs renders several
    // controls at once, all bound to the same model, and the last one wins
    // silently.
    for (const type of Object.values(ProfileFieldType)) {
      const f =
        type === ProfileFieldType.Select
          ? field({ field_type: { Select: { options: ['a'] } } })
          : field({ field_type: type })
      const w = mountField(f, type === ProfileFieldType.Boolean ? false : '')
      const controls = w.findAll('input, select, textarea')
      // TextArray is a chip editor: a wrapper div with one text input inside,
      // plus one remove button per chip. With no chips that is one input.
      expect(controls.length, `${type} rendered ${controls.length} controls`).toBe(1)
    }
  })
})

describe('the label', () => {
  it('marks a required field with an asterisk and nothing else does', () => {
    const optional = mountField(field({ required: false }))
    expect(optional.find('.text-error').exists()).toBe(false)

    const required = mountField(field({ required: true }))
    const marker = required.find('.text-error')
    expect(marker.exists()).toBe(true)
    expect(marker.text()).toBe('*')
  })

  it('shows help text as a tooltip carrying the exact text', () => {
    const wrapper = mountField(field({ help_text: 'Your RFID card number' }))
    const tip = wrapper.find('.tooltip')
    expect(tip.exists()).toBe(true)
    expect(tip.attributes('data-tip')).toBe('Your RFID card number')
    // One icon, not a stack of them.
    expect(wrapper.findAll('svg')).toHaveLength(1)
  })

  it('renders no tooltip at all when there is no help text', () => {
    const wrapper = mountField(field())
    expect(wrapper.find('.tooltip').exists()).toBe(false)
    expect(wrapper.findAll('svg')).toHaveLength(0)
  })
})

describe('error presentation', () => {
  it('adds the type-appropriate error class and renders the message verbatim', () => {
    // Three different error classes for three different control kinds, which is
    // three chances to attach the wrong one — and an input carrying
    // `select-error` is styled by nothing at all.
    const cases: Array<[ProfileFieldType | { Select: { options: string[] } }, string, string]> = [
      [ProfileFieldType.Text, 'input', 'input input-bordered w-full input-error'],
      [ProfileFieldType.Boolean, 'input[type="checkbox"]', 'checkbox checkbox-error'],
      [{ Select: { options: ['a'] } }, 'select', 'select select-bordered w-full select-error'],
    ]

    for (const [type, selector, expected] of cases) {
      const wrapper = mountField(field({ field_type: type as Field['field_type'] }), '', {
        errorMessage: 'That will not do',
      })
      expect(wrapper.find(selector).attributes('class')).toBe(expected)
      expect(wrapper.find('.text-error').text()).toBe('That will not do')
    }
  })

  it('shows no error element when there is no message', () => {
    const wrapper = mountField(field())
    expect(wrapper.find('.label-text-alt.text-error').exists()).toBe(false)
  })
})

describe('value handling', () => {
  it('emits null for a cleared date and never an empty string', () => {
    // An empty string is not a date. Sent to the server it is a parse error on
    // a column that would have accepted NULL perfectly happily.
    const wrapper = mountField(field({ field_type: ProfileFieldType.Date }), '2026-01-01')
    const input = wrapper.find('input[type="date"]')
    ;(input.element as HTMLInputElement).value = ''
    input.trigger('input')

    const emitted = wrapper.emitted('update:modelValue')
    expect(emitted).toBeTruthy()
    expect(emitted?.at(-1)?.[0]).toBeNull()
  })

  it('emits null for a cleared number and a Number for anything else', () => {
    const wrapper = mountField(field({ field_type: ProfileFieldType.Number }), 5)
    const input = wrapper.find('input[type="number"]')

    ;(input.element as HTMLInputElement).value = ''
    input.trigger('input')
    expect(wrapper.emitted('update:modelValue')?.at(-1)?.[0]).toBeNull()

    ;(input.element as HTMLInputElement).value = '0'
    input.trigger('input')
    // Zero, not null, not ''. A falsy number that becomes null is a value the
    // user typed and the form silently discarded.
    expect(wrapper.emitted('update:modelValue')?.at(-1)?.[0]).toBe(0)
  })

  it('shows a zero rather than an empty box', () => {
    const wrapper = mountField(field({ field_type: ProfileFieldType.Number }), 0)
    expect((wrapper.find('input[type="number"]').element as HTMLInputElement).value).toBe('0')
  })

  it('renders a false boolean as unchecked rather than absent', () => {
    const wrapper = mountField(field({ field_type: ProfileFieldType.Boolean }), false)
    const box = wrapper.find('input[type="checkbox"]').element as HTMLInputElement
    expect(box.checked).toBe(false)
  })

  it('coerces a non-boolean model value for a boolean field', () => {
    const wrapper = mountField(field({ field_type: ProfileFieldType.Boolean }), 'yes')
    expect((wrapper.find('input[type="checkbox"]').element as HTMLInputElement).checked).toBe(true)
  })
})

describe('the select field', () => {
  it('renders a placeholder option plus one per configured option, in order', () => {
    const wrapper = mountField(field({ field_type: { Select: { options: ['red', 'green', 'blue'] } } }))
    const options = wrapper.findAll('option')

    expect(options).toHaveLength(4)
    expect(options[0].attributes('value')).toBe('')
    expect(options[0].text()).toBe('Select Test Field')
    expect(options.slice(1).map((o) => o.attributes('value'))).toEqual(['red', 'green', 'blue'])
  })

  it('renders only the placeholder when the option list is empty', () => {
    const wrapper = mountField(field({ field_type: { Select: { options: [] } } }))
    expect(wrapper.findAll('option')).toHaveLength(1)
  })
})

describe('the TextArray chip editor', () => {
  it('renders one badge per value, in order, with a remove control each', () => {
    const wrapper = mountField(field({ field_type: ProfileFieldType.TextArray }), ['A1', 'B2', 'C3'])
    const badges = wrapper.findAll('.badge')

    expect(badges).toHaveLength(3)
    expect(badges.map((b) => b.text().replace('×', '').trim())).toEqual(['A1', 'B2', 'C3'])
    expect(wrapper.findAll('button[aria-label="Remove"]')).toHaveLength(3)
  })

  it('ignores non-string entries rather than rendering them', () => {
    // Deployed profile data holds both shapes for this field, and a number that
    // renders as a chip is a chip that cannot be removed by value.
    const wrapper = mountField(field({ field_type: ProfileFieldType.TextArray }), ['A1', 42, null, 'B2'])
    expect(wrapper.findAll('.badge')).toHaveLength(2)
  })

  it('treats a scalar model value as no chips rather than one', () => {
    const wrapper = mountField(field({ field_type: ProfileFieldType.TextArray }), 'A1')
    expect(wrapper.findAll('.badge')).toHaveLength(0)
  })

  it('emits the whole new array on add, and refuses a duplicate', async () => {
    const wrapper = mountField(field({ field_type: ProfileFieldType.TextArray }), ['A1'])
    const input = wrapper.find('input[type="text"]')

    await input.setValue('B2')
    await input.trigger('keydown.enter')
    expect(wrapper.emitted('update:modelValue')?.at(-1)?.[0]).toEqual(['A1', 'B2'])

    // The component is uncontrolled in the test — the prop does not change — so
    // a duplicate of the *original* value is what proves the guard.
    await input.setValue('A1')
    await input.trigger('keydown.enter')
    expect(wrapper.emitted('update:modelValue')).toHaveLength(1)
  })

  it('does not add an empty or whitespace-only chip', async () => {
    const wrapper = mountField(field({ field_type: ProfileFieldType.TextArray }), [])
    const input = wrapper.find('input[type="text"]')

    for (const value of ['', '   ', '\t']) {
      await input.setValue(value)
      await input.trigger('keydown.enter')
    }
    expect(wrapper.emitted('update:modelValue')).toBeUndefined()
  })

  it('trims the value it commits', async () => {
    const wrapper = mountField(field({ field_type: ProfileFieldType.TextArray }), [])
    const input = wrapper.find('input[type="text"]')
    await input.setValue('  A1  ')
    await input.trigger('keydown.enter')
    expect(wrapper.emitted('update:modelValue')?.at(-1)?.[0]).toEqual(['A1'])
  })

  it('removes the chip at the index clicked, not the one matching by value', async () => {
    // Duplicates cannot be added, but they can arrive from the server, and
    // removing by value would take the wrong one.
    const wrapper = mountField(field({ field_type: ProfileFieldType.TextArray }), ['A1', 'B2', 'A1'])
    await wrapper.findAll('button[aria-label="Remove"]')[2].trigger('click')
    expect(wrapper.emitted('update:modelValue')?.at(-1)?.[0]).toEqual(['A1', 'B2'])
  })

  it('commits a pending chip on blur rather than discarding it', async () => {
    // Typing a value and clicking Save without pressing Enter is the common
    // way to use this control, and losing it looks exactly like the save
    // having failed.
    const wrapper = mountField(field({ field_type: ProfileFieldType.TextArray }), [])
    const input = wrapper.find('input[type="text"]')
    await input.setValue('A1')
    await input.trigger('blur')
    expect(wrapper.emitted('update:modelValue')?.at(-1)?.[0]).toEqual(['A1'])
    expect(wrapper.emitted('blur')).toHaveLength(1)
  })

  it('shows the placeholder only while there are no chips', () => {
    const empty = mountField(field({ field_type: ProfileFieldType.TextArray, help_text: 'cards' }), [])
    expect(empty.find('input[type="text"]').attributes('placeholder')).toBe('cards')

    const filled = mountField(field({ field_type: ProfileFieldType.TextArray, help_text: 'cards' }), ['A1'])
    expect(filled.find('input[type="text"]').attributes('placeholder')).toBe('')
  })
})

describe('the disabled state', () => {
  it('disables every control it renders, including the chip remove buttons', () => {
    // A remove button that stays live on a disabled field lets somebody delete
    // a value they are not allowed to edit, and the deletion looks like theirs.
    for (const type of Object.values(ProfileFieldType)) {
      const f =
        type === ProfileFieldType.Select
          ? field({ field_type: { Select: { options: ['a'] } } })
          : field({ field_type: type })
      const model = type === ProfileFieldType.TextArray ? ['A1'] : ''
      const wrapper = mountField(f, model, { disabled: true })

      for (const el of wrapper.findAll('input, select, textarea, button')) {
        expect(
          el.attributes('disabled'),
          `${type}: <${el.element.tagName.toLowerCase()}> is still enabled`,
        ).toBeDefined()
      }
    }
  })
})

describe('the blur event', () => {
  it('is emitted by every control kind', () => {
    // The parent validates on blur. A control that never emits it is a field
    // that never validates until submit, which is a different UI with the same
    // appearance.
    const cases: Array<[Field['field_type'], string, string]> = [
      [ProfileFieldType.Text, 'input[type="text"]', 'blur'],
      [ProfileFieldType.Email, 'input[type="email"]', 'blur'],
      [ProfileFieldType.Phone, 'input[type="tel"]', 'blur'],
      [ProfileFieldType.Number, 'input[type="number"]', 'blur'],
      [ProfileFieldType.Date, 'input[type="date"]', 'blur'],
      [ProfileFieldType.Boolean, 'input[type="checkbox"]', 'change'],
      [{ Select: { options: ['a'] } }, 'select', 'blur'],
    ]

    for (const [type, selector, event] of cases) {
      const wrapper = mountField(field({ field_type: type }), '')
      wrapper.find(selector).trigger(event)
      expect(wrapper.emitted('blur'), `${JSON.stringify(type)} did not emit blur`).toBeTruthy()
    }
  })
})
