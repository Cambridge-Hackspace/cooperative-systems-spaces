// Tier 2: ProfileConfigAdmin.
//
// The admin's view of the profile schema, and it does not show it.
//
// Every part of the page that displays configuration reads `editConfig` --
// the toggle, the field list, and the preview card. `editConfig` is
// initialized to `{ profiles_enabled: false, profile_fields: [] }` and is only
// ever filled in by `startEditing()`. `profileConfig`, which holds what the
// server actually returned, is used for nothing but the loading and error
// gates and as the source of `startEditing`'s deep copy.
//
// So an admin who opens this page sees "profiles disabled, no fields"
// regardless of the real configuration, and pressing Cancel puts it back to
// that. The only way to see the schema is to enter edit mode.
//
// The store is mocked as a reactive stub, as in the UserProfile spec: what is
// under test is which state the page renders and what it sends, not the
// store's own behavior.
//
// What this spec does NOT prove: that the server accepts the config shape.
// Tier 6 owns the round trip; `tests/unit/profile-validate.spec.ts` owns the
// validation rules the store applies.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick, reactive } from 'vue'
import { ProfileFieldType, type ProfileField } from '@/types'

interface StoreStub {
  loading: boolean
  error: string | null
  profileConfig: { profiles_enabled: boolean; profile_fields: ProfileField[] } | null
}

const state = vi.hoisted<{ current: StoreStub | null }>(() => ({ current: null }))
const actions = vi.hoisted(() => ({
  fetchProfileConfig: vi.fn(),
  updateProfileConfig: vi.fn(),
  clearError: vi.fn(),
}))

vi.mock('@/stores/profile', () => ({
  useProfileStore: () => {
    const s = state.current
    if (!s) throw new Error('the store stub is not installed; did beforeEach run?')
    return {
      get loading() {
        return s.loading
      },
      get error() {
        return s.error
      },
      get profileConfig() {
        return s.profileConfig
      },
      ...actions,
    }
  },
}))

import ProfileConfigAdmin from '@/components/ProfileConfigAdmin.vue'

function field(key: string, over: Partial<ProfileField> = {}): ProfileField {
  return { key, label: key, field_type: ProfileFieldType.Text, required: false, ...over }
}

const CONFIGURED: ProfileField[] = [
  field('bio', { label: 'Bio' }),
  field('shirt', { label: 'Shirt size', field_type: { Select: { options: ['S', 'M', 'L'] } } }),
]

beforeEach(() => {
  for (const a of Object.values(actions)) a.mockReset()
  actions.fetchProfileConfig.mockResolvedValue(undefined)
  actions.updateProfileConfig.mockResolvedValue(undefined)
  state.current = reactive<StoreStub>({
    loading: false,
    error: null,
    profileConfig: { profiles_enabled: true, profile_fields: CONFIGURED },
  })
})

function store(): StoreStub {
  const s = state.current
  if (!s) throw new Error('the store stub is not installed; did beforeEach run?')
  return s
}

async function page() {
  const w = mount(ProfileConfigAdmin)
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof page>>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labeled ${JSON.stringify(label)}`)
  return b
}

const enabledToggle = (w: Wrapper) => w.find('input[type="checkbox"].checkbox-primary')
const fieldCards = (w: Wrapper) => w.findAll('.space-y-4 > .card.bg-base-100')
const sent = () =>
  actions.updateProfileConfig.mock.calls[0][0] as {
    profiles_enabled: boolean
    profile_fields: ProfileField[]
  }

describe('what the page shows before anything is edited', () => {
  it('reads the configuration on open', async () => {
    await page()
    expect(actions.fetchProfileConfig).toHaveBeenCalled()
  })

  // Was a pinned FINDING: the field list is
  // `v-if="editConfig.profile_fields.length > 0"` over `editConfig`, which
  // started empty and was only filled by `startEditing()` -- so the configured
  // fields sat on the store the whole time and were never rendered until the
  // admin pressed Edit. `loadConfiguration()` now calls
  // `syncEditConfigFromProfile()`, so the read-only view shows real data.
  //
  // The pin said to delete this test if the binding was fixed. Kept instead,
  // inverted, because the fix is one call in one function and nothing else
  // would notice if it were dropped.
  it('shows the configured fields before anything is edited', async () => {
    const w = await page()
    expect(store().profileConfig?.profile_fields).toHaveLength(2)
    expect(fieldCards(w)).toHaveLength(2)
    expect(w.text()).toContain('Shirt size')
  })

  // Same cause, same fix: the toggle is bound to
  // `editConfig.profiles_enabled`, which was `false` until edit mode opened,
  // so an admin looking at a system with profiles switched *on* was shown a
  // switch in the off position.
  it('shows profiles as enabled when they are enabled', async () => {
    const w = await page()
    expect(store().profileConfig?.profiles_enabled).toBe(true)
    expect((enabledToggle(w).element as HTMLInputElement).checked).toBe(true)
  })

  it('does at least refuse to let the toggle be moved outside edit mode', async () => {
    const w = await page()
    expect(enabledToggle(w).attributes('disabled')).toBeDefined()
  })

  it('shows the real configuration once editing starts', async () => {
    const w = await page()
    await buttonNamed(w, 'Edit Configuration').trigger('click')
    await nextTick()

    expect(fieldCards(w)).toHaveLength(2)
    expect(w.text()).toContain('Shirt size')
    expect((enabledToggle(w).element as HTMLInputElement).checked).toBe(true)
  })

  // Was a pinned FINDING: cancelling reset `editConfig` to the empty default
  // rather than to what the server sent, so the page went back to claiming
  // there was no configuration. `cancelEditing()` now re-syncs.
  it('restores the stored configuration when the edit is canceled', async () => {
    const w = await page()
    await buttonNamed(w, 'Edit Configuration').trigger('click')
    await nextTick()
    expect(fieldCards(w)).toHaveLength(2)

    // Edit something, so a cancel that merely left the buffer alone would be
    // indistinguishable from one that restored it.
    await buttonNamed(w, 'Add Field').trigger('click')
    await nextTick()
    expect(fieldCards(w)).toHaveLength(3)

    await buttonNamed(w, 'Cancel').trigger('click')
    await nextTick()

    expect(fieldCards(w)).toHaveLength(2)
    expect(w.text()).toContain('Shirt size')
    expect(actions.clearError).toHaveBeenCalled()
  })

  it('shows a spinner while loading and nothing has arrived', async () => {
    store().profileConfig = null
    store().loading = true
    const w = await page()
    expect(w.find('.loading-spinner').exists()).toBe(true)
  })

  it('shows the store error with a retry when nothing has arrived', async () => {
    store().profileConfig = null
    store().error = 'Admin role required'
    const w = await page()

    expect(w.find('.alert-error').text()).toContain('Admin role required')
    actions.fetchProfileConfig.mockClear()
    await buttonNamed(w, 'Retry').trigger('click')
    expect(actions.fetchProfileConfig).toHaveBeenCalled()
  })
})

describe('editing the field list', () => {
  const startEditing = async (w: Wrapper) => {
    await buttonNamed(w, 'Edit Configuration').trigger('click')
    await nextTick()
  }

  it('offers every field type the enum defines', async () => {
    const w = await page()
    await startEditing(w)
    const offered = w
      .findAll('select option')
      .map((o) => o.attributes('value'))
      .filter((v) => v !== '')
      .filter((v, i, all) => all.indexOf(v) === i)

    expect([...offered].sort()).toEqual([...Object.values(ProfileFieldType)].sort())
  })

  it('adds an empty field and removes the one at the index clicked', async () => {
    const w = await page()
    await startEditing(w)
    await buttonNamed(w, 'Add Field').trigger('click')
    await nextTick()
    expect(fieldCards(w)).toHaveLength(3)

    // The *second* field, so that a handler ignoring its index and always
    // removing the first would be visible. Removing index 0 in a test cannot
    // tell `splice(index, 1)` from `splice(0, 1)`.
    await fieldCards(w)[1].find('button').trigger('click')
    await nextTick()
    expect(fieldCards(w)).toHaveLength(2)
    expect(w.text()).toContain('Bio')
    expect(w.text()).not.toContain('Shirt size')
  })

  it('does not mutate the stored configuration while editing', async () => {
    // `startEditing` deep-copies, so abandoning an edit cannot leave the store
    // holding half of it.
    const w = await page()
    await startEditing(w)
    await fieldCards(w)[0].find('button').trigger('click')
    await nextTick()

    expect(store().profileConfig?.profile_fields).toHaveLength(2)
  })
})

describe('validation', () => {
  const startEditing = async (w: Wrapper) => {
    await buttonNamed(w, 'Edit Configuration').trigger('click')
    await nextTick()
  }

  it('refuses a field with no key and no label, and names which field', async () => {
    const w = await page()
    await startEditing(w)
    await buttonNamed(w, 'Add Field').trigger('click')
    await nextTick()
    await buttonNamed(w, 'Save Configuration').trigger('click')
    await flushPromises()

    const errors = w.find('.alert-error').text()
    expect(errors).toContain('Field 3: Key is required')
    expect(errors).toContain('Field 3: Label is required')
    expect(actions.updateProfileConfig).not.toHaveBeenCalled()
  })

  it('refuses two fields with the same key', async () => {
    store().profileConfig = {
      profiles_enabled: true,
      profile_fields: [field('bio', { label: 'Bio' }), field('bio', { label: 'Biography' })],
    }
    const w = await page()
    await startEditing(w)
    await buttonNamed(w, 'Save Configuration').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('Key "bio" is already used')
    expect(actions.updateProfileConfig).not.toHaveBeenCalled()
  })

  it('refuses a select field with no options', async () => {
    store().profileConfig = {
      profiles_enabled: true,
      profile_fields: [field('shirt', { label: 'Shirt', field_type: { Select: { options: [] } } })],
    }
    const w = await page()
    await startEditing(w)
    await buttonNamed(w, 'Save Configuration').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('at least one option')
  })

  it('refuses a select field with a blank option', async () => {
    store().profileConfig = {
      profiles_enabled: true,
      profile_fields: [
        field('shirt', { label: 'Shirt', field_type: { Select: { options: ['S', '  '] } } }),
      ],
    }
    const w = await page()
    await startEditing(w)
    await buttonNamed(w, 'Save Configuration').trigger('click')
    await flushPromises()

    expect(w.find('.alert-error').text()).toContain('All select options must have values')
  })

  it('saves a configuration that validates', async () => {
    const w = await page()
    await startEditing(w)
    await buttonNamed(w, 'Save Configuration').trigger('click')
    await flushPromises()

    expect(sent().profiles_enabled).toBe(true)
    expect(sent().profile_fields.map((f) => f.key)).toEqual(['bio', 'shirt'])
    expect(w.findAll('button').map((b) => b.text().trim())).toContain('Edit Configuration')
  })
})

describe('when the save fails', () => {
  // FINDING, pinned, and the same one UserProfile has. `saveConfiguration`
  // catches and logs on the grounds that the store handles the error -- but
  // the template renders the store error only under
  // `v-else-if="error && !profileConfig"`. Once a configuration has loaded,
  // `profileConfig` is truthy, so a failed *save* has nowhere to appear. The
  // error branch was written for a failed load and the save path borrowed it.
  it('says nothing, because the error branch needs an empty configuration', async () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})
    actions.updateProfileConfig.mockImplementation(() => {
      store().error = 'Profile config is locked'
      return Promise.reject(new Error('409'))
    })

    const w = await page()
    await buttonNamed(w, 'Edit Configuration').trigger('click')
    await nextTick()
    await buttonNamed(w, 'Save Configuration').trigger('click')
    await flushPromises()

    expect(
      w.find('.alert-error').exists(),
      'a failed save now reports itself -- if the error branch was widened, ' +
        'this test should assert the message instead'
    ).toBe(false)
    expect(w.text()).not.toContain('locked')
    // Still in edit mode, so the work is not lost. That part is right.
    expect(w.findAll('button').map((b) => b.text().trim())).toContain('Save Configuration')
    expect(spy).toHaveBeenCalled()
    spy.mockRestore()
  })
})
