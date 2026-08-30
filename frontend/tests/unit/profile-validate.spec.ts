import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useProfileStore } from '@/stores/profile'
import { ProfileFieldType, type ProfileField } from '@/types'

function field(over: Partial<ProfileField> & Pick<ProfileField, 'key'>): ProfileField {
  return {
    label: over.key,
    field_type: ProfileFieldType.Text,
    required: false,
    ...over,
  }
}

/** Seed the store's config directly; `validateProfile` is pure given it. */
function validateWith(fields: ProfileField[], data: Record<string, unknown>) {
  const store = useProfileStore()
  store.profileConfig = { profiles_enabled: true, profile_fields: fields }
  return store.validateProfile(data)
}

beforeEach(() => setActivePinia(createPinia()))

describe('with no config loaded', () => {
  it('accepts anything, because there is nothing to validate against', () => {
    const store = useProfileStore()
    store.profileConfig = null
    expect(store.validateProfile({ whatever: 'x' })).toEqual({ valid: true, errors: [] })
  })
})

describe('required fields', () => {
  it('rejects a missing value', () => {
    const r = validateWith([field({ key: 'phone', label: 'Phone', required: true })], {})
    expect(r.valid).toBe(false)
    expect(r.errors).toEqual(['Phone is required'])
  })

  it('rejects whitespace-only text', () => {
    const r = validateWith([field({ key: 'phone', label: 'Phone', required: true })], {
      phone: '   ',
    })
    expect(r.valid).toBe(false)
  })

  // These two are the defect.
  //
  // `required` means "the value must be provided", which is how the server
  // reads it: `ProfileValidator::validate_profile` only errors when the key is
  // *absent* from the object (server/src/profile.rs), and is perfectly happy
  // with `false` or `0`.
  //
  // The frontend used `!value`, so a required Boolean could not be saved as
  // `false` and a required Number could not be saved as `0` — the form
  // reported "is required" for a value the user had explicitly supplied and
  // the server would have accepted. A membership checkbox that must be
  // answered "no", or a required count that is legitimately zero, were both
  // unsaveable.
  it('accepts an explicit false for a required boolean', () => {
    const r = validateWith(
      [
        field({
          key: 'waiver',
          label: 'Waiver',
          required: true,
          field_type: ProfileFieldType.Boolean,
        }),
      ],
      { waiver: false }
    )
    expect(r.errors).toEqual([])
    expect(r.valid).toBe(true)
  })

  it('accepts an explicit zero for a required number', () => {
    const r = validateWith(
      [
        field({
          key: 'hours',
          label: 'Hours',
          required: true,
          field_type: ProfileFieldType.Number,
        }),
      ],
      { hours: 0 }
    )
    expect(r.errors).toEqual([])
    expect(r.valid).toBe(true)
  })
})

describe('optional fields', () => {
  it('skip validation when absent', () => {
    expect(
      validateWith([field({ key: 'email', field_type: ProfileFieldType.Email })], {}).valid
    ).toBe(true)
  })
})

describe('per-type rules', () => {
  it('Email must contain an @', () => {
    const bad = validateWith(
      [field({ key: 'email', label: 'Email', field_type: ProfileFieldType.Email })],
      { email: 'nope' }
    )
    expect(bad.errors).toEqual(['Email must be a valid email address'])
    expect(
      validateWith([field({ key: 'email', field_type: ProfileFieldType.Email })], {
        email: 'a@b.co',
      }).valid
    ).toBe(true)
  })

  it('Phone must be at least seven characters', () => {
    expect(
      validateWith([field({ key: 'p', label: 'P', field_type: ProfileFieldType.Phone })], {
        p: '12345',
      }).valid
    ).toBe(false)
    expect(
      validateWith([field({ key: 'p', field_type: ProfileFieldType.Phone })], { p: '5551234' })
        .valid
    ).toBe(true)
  })

  it('Date must be YYYY-MM-DD shaped', () => {
    expect(
      validateWith([field({ key: 'd', label: 'D', field_type: ProfileFieldType.Date })], {
        d: '15/01/2026',
      }).valid
    ).toBe(false)
    expect(
      validateWith([field({ key: 'd', field_type: ProfileFieldType.Date })], { d: '2026-01-15' })
        .valid
    ).toBe(true)
  })

  // Named for what it does NOT prove: the regex checks shape, not validity.
  // `2026-13-45` is not a date and this accepts it, exactly as the server's
  // own Date check does. Recorded so the limit is visible rather than assumed
  // away.
  it('does not check that a well-shaped date is a real one', () => {
    expect(
      validateWith([field({ key: 'd', field_type: ProfileFieldType.Date })], { d: '2026-13-45' })
        .valid
    ).toBe(true)
  })

  it('Select must be one of its options, and lists them when it is not', () => {
    const select = field({
      key: 'tier',
      label: 'Tier',
      field_type: { Select: { options: ['gold', 'silver'] } },
    })
    const bad = validateWith([select], { tier: 'bronze' })
    expect(bad.valid).toBe(false)
    expect(bad.errors[0]).toContain('gold, silver')
    expect(validateWith([select], { tier: 'gold' }).valid).toBe(true)
  })

  it('Boolean must actually be a boolean', () => {
    expect(
      validateWith([field({ key: 'b', label: 'B', field_type: ProfileFieldType.Boolean })], {
        b: 'yes',
      }).valid
    ).toBe(false)
  })
})

describe('the field_type fallback', () => {
  // `ProfileField.vue` narrows a non-string `field_type` to `Select` when it
  // carries a `Select` key and to `Text` otherwise. `validateProfile` narrowed
  // *everything* non-string to `Select`, so the two disagreed about anything
  // that was neither — and since the `Select` branch then re-checks the shape
  // and finds it wrong, such a field silently received no validation at all
  // under a label claiming it had been validated as a Select.
  it('treats an unrecognized object field_type as Text, matching the component', () => {
    const weird = field({ key: 'x', field_type: { Mystery: {} } as never })
    // Text imposes no constraint, so anything is accepted — but it is accepted
    // *as Text*, which is what the renderer will show.
    expect(validateWith([weird], { x: 'anything' }).valid).toBe(true)
    expect(validateWith([weird], { x: 12345 }).valid).toBe(true)
  })
})

describe('multiple errors', () => {
  it('reports every failing field, not just the first', () => {
    const r = validateWith(
      [
        field({ key: 'email', label: 'Email', field_type: ProfileFieldType.Email }),
        field({ key: 'phone', label: 'Phone', field_type: ProfileFieldType.Phone }),
      ],
      { email: 'nope', phone: '1' }
    )
    expect(r.errors).toHaveLength(2)
  })
})
