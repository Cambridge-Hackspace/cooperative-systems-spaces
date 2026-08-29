// Tier 2: UserProfile.
//
// The profile store is mocked as a reactive stub rather than driven through
// `createTestingPinia`. The component reads six getters and calls five actions,
// and what is under test here is which branch it renders and what it hands to
// `ProfileField` -- not the store's own behaviour, which `tests/unit` already
// covers. `ProfileField` is stubbed for the same reason: it has its own spec.
//
// Two findings come out of the branch chain and one out of the save path.
//
// What this spec does NOT prove: that the store's `validateProfile` is correct
// (tests/unit/profile-validate.spec.ts), or that ProfileField renders anything
// (tests/components/ProfileField.spec.ts).

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick, reactive } from 'vue'
import type { ProfileField as ProfileFieldDef, ProfileResponse, User } from '@/types'
import { ProfileFieldType, UserRole } from '@/types'

interface StoreStub {
  loading: boolean
  error: string | null
  profiles: Record<string, ProfileResponse>
  fields: ProfileFieldDef[]
  profilesEnabled: boolean
  canEdit: boolean
  canManageConfig: boolean
  profileConfig: unknown
  validationErrors: string[]
}

const state = vi.hoisted<{ current: StoreStub | null }>(() => ({ current: null }))
const actions = vi.hoisted(() => ({
  fetchUserProfile: vi.fn(),
  fetchProfileConfig: vi.fn(),
  updateUserProfile: vi.fn(),
  clearError: vi.fn(),
}))
const authState = vi.hoisted<{ user: User | null }>(() => ({ user: null }))

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
      get isProfilesEnabled() {
        return s.profilesEnabled
      },
      get getProfileFields() {
        return s.fields
      },
      get canManageProfileConfig() {
        return s.canManageConfig
      },
      getProfileForUser: (id: string) => s.profiles[id] ?? null,
      canEditProfile: () => s.canEdit,
      validateProfile: () => ({
        isValid: s.validationErrors.length === 0,
        errors: s.validationErrors,
      }),
      ...actions,
    }
  },
}))

vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({
    get user() {
      return authState.user
    },
  }),
}))

import UserProfile from '@/components/UserProfile.vue'

function field(key: string, over: Partial<ProfileFieldDef> = {}): ProfileFieldDef {
  return { key, label: key, field_type: ProfileFieldType.Text, required: false, ...over }
}

const ME: User = {
  id: 'me',
  username: 'ada',
  email: 'ada@example.test',
  full_name: 'Ada Lovelace',
  is_active: true,
  role: UserRole.Member,
  created_at: '2025-01-01T00:00:00Z',
  updated_at: '2025-01-01T00:00:00Z',
  profile: {},
  meta: {},
}

// Renders enough to assert what UserProfile passes down, and nothing more.
const ProfileFieldStub = {
  props: ['field', 'modelValue', 'errorMessage', 'disabled'],
  emits: ['update:modelValue', 'blur'],
  template:
    '<div class="pf" :data-key="field.key" :data-disabled="String(disabled)" ' +
    ':data-error="errorMessage || \'\'">{{ JSON.stringify(modelValue) }}</div>',
}

const stubs = {
  'router-link': { props: ['to'], template: '<a><slot /></a>' },
  ProfileField: ProfileFieldStub,
}

beforeEach(() => {
  for (const a of Object.values(actions)) a.mockReset()
  actions.fetchProfileConfig.mockResolvedValue(undefined)
  actions.fetchUserProfile.mockResolvedValue(undefined)
  actions.updateUserProfile.mockResolvedValue(undefined)
  authState.user = ME
  state.current = reactive<StoreStub>({
    loading: false,
    error: null,
    profiles: {},
    fields: [],
    profilesEnabled: true,
    canEdit: true,
    canManageConfig: false,
    profileConfig: null,
    validationErrors: [],
  })
})

function store(): StoreStub {
  const s = state.current
  if (!s) throw new Error('the store stub is not installed; did beforeEach run?')
  return s
}

async function profile(props: Record<string, unknown> = {}) {
  const w = mount(UserProfile, { props, global: { stubs } })
  await flushPromises()
  return w
}

type Wrapper = Awaited<ReturnType<typeof profile>>

// `findAll('.pf')` returns DOM wrappers, which have no `vm` to emit from.
// Component wrappers are needed to drive `update:modelValue` the way the real
// ProfileField does.
const fieldAt = (w: Wrapper, i: number) => w.findAllComponents(ProfileFieldStub)[i]

// The one cast in the file, and it is here rather than at eleven call sites.
// `findAllComponents` on a plain options object yields an untyped `vm`; naming
// the single method being called keeps the type-aware lint honest about what is
// actually being assumed.
function emitFromField(w: Wrapper, i: number, event: string, payload: unknown) {
  const c = fieldAt(w, i)
  if (!c) throw new Error(`no ProfileField rendered at index ${i}`)
  ;(c.vm as unknown as { $emit: (e: string, p: unknown) => void }).$emit(event, payload)
}

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}

describe('whose profile is being shown', () => {
  it('says "My Profile" when it is the signed-in user', async () => {
    expect((await profile()).find('.card-title').text()).toContain('My Profile')
  })

  it('names the other user when it is not', async () => {
    const w = await profile({ userId: 'someone-else', user: { ...ME, full_name: 'Grace Hopper' } })
    expect(w.find('.card-title').text()).toContain("Grace Hopper's Profile")
  })

  it('falls back to "User" when the other user has no name to show', async () => {
    const w = await profile({ userId: 'someone-else' })
    expect(w.find('.card-title').text()).toContain("User's Profile")
  })

  it('loads the signed-in user when no userId is given', async () => {
    await profile()
    expect(actions.fetchUserProfile).toHaveBeenCalledWith('me')
  })

  it('reloads when the target user changes', async () => {
    const w = await profile({ userId: 'a' })
    expect(actions.fetchUserProfile).toHaveBeenCalledWith('a')

    await w.setProps({ userId: 'b' })
    await flushPromises()
    expect(actions.fetchUserProfile).toHaveBeenCalledWith('b')
  })
})

describe('which branch renders', () => {
  it('reads the config before it reads the profile', async () => {
    await profile()
    expect(actions.fetchProfileConfig).toHaveBeenCalled()
    expect(actions.fetchUserProfile).toHaveBeenCalled()
  })

  it('does not re-read a config it already has', async () => {
    store().profileConfig = {}
    await profile()
    expect(actions.fetchProfileConfig).not.toHaveBeenCalled()
    expect(actions.fetchUserProfile).toHaveBeenCalled()
  })

  it('does not fetch a profile at all when profiles are switched off', async () => {
    store().profilesEnabled = false
    const w = await profile()

    expect(w.find('.alert-info').text()).toContain('Profiles Disabled')
    expect(actions.fetchUserProfile).not.toHaveBeenCalled()
  })

  it('shows a spinner while loading and nothing has arrived yet', async () => {
    store().loading = true
    const w = await profile()
    expect(w.find('.loading-spinner').exists()).toBe(true)
    expect(w.text()).toContain('Loading profile...')
  })

  it('shows the store error, with a retry, when nothing has arrived', async () => {
    store().error = 'Forbidden'
    const w = await profile()

    expect(w.find('.alert-error').text()).toContain('Forbidden')
    actions.fetchUserProfile.mockClear()
    await buttonNamed(w, 'Retry').trigger('click')
    expect(actions.fetchUserProfile).toHaveBeenCalledWith('me')
  })

  it('offers a link to the admin page when there are no fields and the user may configure them', async () => {
    store().canManageConfig = true
    const w = await profile()
    expect(w.text()).toContain('No Profile Fields')
    expect(w.find('a').exists()).toBe(true)
  })

  it('says there are no fields, without the admin link, for everyone else', async () => {
    const w = await profile()
    expect(w.text()).toContain('No Profile Fields')
    expect(w.findAll('a')).toHaveLength(0)
  })

  // FINDING, pinned. The final `v-else` -- "Profile Not Available" -- cannot
  // render. By the time the chain reaches it, `isProfilesEnabled` is true (the
  // first branch took the false case), so one of the two preceding branches
  // matches: `fields.length > 0` or `fields.length === 0` exhaust the integers.
  // It is dead template, and its text contradicts the branch that actually
  // fires in the case it looks like it was written for.
  it('never shows "Profile Not Available", whatever the store says', async () => {
    for (const [enabled, fields] of [
      [true, []],
      [true, [field('bio')]],
      [false, []],
      [false, [field('bio')]],
    ] as const) {
      store().profilesEnabled = enabled
      store().fields = [...fields]
      const w = await profile()
      expect(
        w.text(),
        'the branch chain now reaches its final v-else -- if it was restructured, ' +
          'this test should assert the case that reaches it'
      ).not.toContain('Profile Not Available')
    }
  })
})

describe('what the fields are given', () => {
  beforeEach(() => {
    store().fields = [field('bio'), field('shop_number', { field_type: ProfileFieldType.Number })]
    store().profiles = { me: { user_id: 'me', profile: { bio: 'Machinist', shop_number: 7 } } }
  })

  it('renders one field per definition, disabled until editing starts', async () => {
    const w = await profile()
    const rendered = w.findAll('.pf')

    expect(rendered.map((f) => f.attributes('data-key'))).toEqual(['bio', 'shop_number'])
    expect(rendered.every((f) => f.attributes('data-disabled') === 'true')).toBe(true)
    expect(rendered[0].text()).toBe('"Machinist"')
  })

  it('enables them once editing starts, seeded from the stored profile', async () => {
    const w = await profile()
    await buttonNamed(w, 'Edit Profile').trigger('click')
    await nextTick()

    const rendered = w.findAll('.pf')
    expect(rendered.every((f) => f.attributes('data-disabled') === 'false')).toBe(true)
    expect(rendered[0].text()).toBe('"Machinist"')
  })

  // FINDING, pinned. `getFieldValue` ends in `|| null`, so every falsy stored
  // value is displayed as empty: a Number field holding 0, a checkbox holding
  // false, a Text field holding "". A member whose shop number is 0 sees a
  // blank field and, on save, writes the blank back.
  it('displays a stored zero as nothing at all', async () => {
    store().profiles = { me: { user_id: 'me', profile: { bio: '', shop_number: 0 } } }
    const w = await profile()

    expect(
      w.findAll('.pf')[1].text(),
      'a stored 0 now renders -- if the `|| null` was changed to a nullish ' +
        'check, delete this test'
    ).toBe('null')
    expect(w.findAll('.pf')[0].text()).toBe('null')
  })

  it('hides the edit button from someone who may not edit', async () => {
    store().canEdit = false
    const w = await profile()
    expect(w.findAll('button').map((b) => b.text().trim())).not.toContain('Edit Profile')
  })
})

describe('validation while editing', () => {
  beforeEach(() => {
    store().fields = [
      field('bio', { required: true, label: 'Bio' }),
      field('contact', { field_type: ProfileFieldType.Email, label: 'Contact' }),
    ]
    store().profiles = { me: { user_id: 'me', profile: {} } }
  })

  it('reports a required field left empty', async () => {
    const w = await profile()
    await buttonNamed(w, 'Edit Profile').trigger('click')
    await nextTick()

    emitFromField(w, 0, 'update:modelValue', '')
    await nextTick()

    expect(w.findAll('.pf')[0].attributes('data-error')).toBe('Bio is required')
    expect(buttonNamed(w, 'Save Profile').attributes('disabled')).toBeDefined()
  })

  it('clears the error once the field is filled in', async () => {
    const w = await profile()
    await buttonNamed(w, 'Edit Profile').trigger('click')
    await nextTick()

    emitFromField(w, 0, 'update:modelValue', '')
    await nextTick()
    emitFromField(w, 0, 'update:modelValue', 'Machinist')
    await nextTick()

    expect(w.findAll('.pf')[0].attributes('data-error')).toBe('')
  })

  // FINDING, pinned. The Email check is `value.includes('@')`. A bare "@" is
  // accepted, and so is "a@" or "@b". The store's `validateProfileAgainst` is
  // the stronger check, but this one runs first and populates `fieldErrors`,
  // so what the user is told about their email address comes from here.
  it('accepts a bare @ as a valid email address', async () => {
    const w = await profile()
    await buttonNamed(w, 'Edit Profile').trigger('click')
    await nextTick()

    emitFromField(w, 1, 'update:modelValue', '@')
    await nextTick()

    expect(
      w.findAll('.pf')[1].attributes('data-error'),
      'the email check is now stronger than `includes("@")` -- if it was ' +
        'tightened, delete this test'
    ).toBe('')
  })

  it('rejects an address with no @ at all', async () => {
    const w = await profile()
    await buttonNamed(w, 'Edit Profile').trigger('click')
    await nextTick()

    emitFromField(w, 1, 'update:modelValue', 'ada.example.test')
    await nextTick()

    expect(w.findAll('.pf')[1].attributes('data-error')).toBe(
      'Contact must be a valid email address'
    )
  })

  it("surfaces the store's own validation errors above the fields", async () => {
    store().validationErrors = ['Bio is too long']
    const w = await profile()
    await buttonNamed(w, 'Edit Profile').trigger('click')
    await nextTick()
    emitFromField(w, 0, 'update:modelValue', 'x')
    await nextTick()

    expect(w.find('.alert-warning').text()).toContain('Bio is too long')
  })
})

describe('saving', () => {
  beforeEach(() => {
    store().fields = [field('bio')]
    store().profiles = { me: { user_id: 'me', profile: { bio: 'Machinist' } } }
  })

  it('sends the edit buffer and leaves edit mode', async () => {
    const w = await profile()
    await buttonNamed(w, 'Edit Profile').trigger('click')
    await nextTick()
    emitFromField(w, 0, 'update:modelValue', 'Welder')
    await nextTick()
    await buttonNamed(w, 'Save Profile').trigger('click')
    await flushPromises()

    expect(actions.updateUserProfile).toHaveBeenCalledWith('me', { bio: 'Welder' })
    expect(w.findAll('button').map((b) => b.text().trim())).toContain('Edit Profile')
  })

  it('does not send anything when a touched field is invalid', async () => {
    store().fields = [field('bio', { required: true, label: 'Bio' })]
    const w = await profile()
    await buttonNamed(w, 'Edit Profile').trigger('click')
    await nextTick()
    emitFromField(w, 0, 'update:modelValue', '')
    await nextTick()
    // The button is disabled here, so this asserts the button -- not the guard
    // inside `saveProfile`. The next test covers the guard, which is reachable
    // by a different route.
    expect(buttonNamed(w, 'Save Profile').attributes('disabled')).toBeDefined()
    await buttonNamed(w, 'Save Profile').trigger('click')
    await flushPromises()

    expect(actions.updateUserProfile).not.toHaveBeenCalled()
  })

  it('re-validates on submit, because entering edit mode clears the errors first', async () => {
    // `startEditing` resets `fieldErrors` and `validationErrors`, so
    // `isFormValid` is true and Save is *enabled* the instant edit mode opens,
    // whatever the stored profile is missing. The guard inside `saveProfile` is
    // what actually stops an empty required field -- not the disabled
    // attribute, which is not set in this state.
    store().fields = [field('bio', { required: true, label: 'Bio' })]
    store().profiles = { me: { user_id: 'me', profile: {} } }
    const w = await profile()
    await buttonNamed(w, 'Edit Profile').trigger('click')
    await nextTick()

    expect(buttonNamed(w, 'Save Profile').attributes('disabled')).toBeUndefined()
    await buttonNamed(w, 'Save Profile').trigger('click')
    await flushPromises()

    expect(actions.updateUserProfile).not.toHaveBeenCalled()
    expect(fieldAt(w, 0).props('errorMessage')).toBe('Bio is required')
  })

  it('discards the edit buffer on Cancel and clears the store error', async () => {
    const w = await profile()
    await buttonNamed(w, 'Edit Profile').trigger('click')
    await nextTick()
    emitFromField(w, 0, 'update:modelValue', 'Welder')
    await nextTick()
    await buttonNamed(w, 'Cancel').trigger('click')
    await nextTick()

    expect(actions.clearError).toHaveBeenCalled()
    expect(w.findAll('.pf')[0].text()).toBe('"Machinist"')

    // And the abandoned edit does not come back on the next attempt.
    // `cancelEditing` also empties the edit buffer, which is belt-and-braces:
    // `startEditing` re-seeds it from the stored profile either way, so
    // removing that line changes nothing observable. Recorded rather than
    // asserted -- a test that could only fail by reading private state would be
    // asserting the implementation, not the behaviour.
    await buttonNamed(w, 'Edit Profile').trigger('click')
    await nextTick()
    expect(w.findAll('.pf')[0].text()).toBe('"Machinist"')
  })

  // FINDING, pinned. `saveProfile` catches and logs, on the stated grounds
  // that "error is already handled by the store" -- but the template only
  // renders the store error under `v-else-if="error && !profileData"`. Once a
  // profile has loaded, `profileData` is truthy, so a failed *save* has
  // nowhere to appear. The user presses Save, stays in edit mode, and is told
  // nothing.
  it('says nothing when the save fails, because the error branch needs an empty profile', async () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})
    actions.updateUserProfile.mockImplementation(() => {
      store().error = 'Profile is read-only for your role'
      return Promise.reject(new Error('403'))
    })

    const w = await profile()
    await buttonNamed(w, 'Edit Profile').trigger('click')
    await nextTick()
    emitFromField(w, 0, 'update:modelValue', 'Welder')
    await nextTick()
    await buttonNamed(w, 'Save Profile').trigger('click')
    await flushPromises()

    expect(
      w.find('.alert-error').exists(),
      'a failed save now reports itself -- if the error branch was widened, ' +
        'this test should assert the message instead'
    ).toBe(false)
    expect(w.text()).not.toContain('read-only for your role')
    // Still in edit mode, so the edit is not lost -- that part is right.
    expect(w.findAll('button').map((b) => b.text().trim())).toContain('Save Profile')
    expect(spy).toHaveBeenCalled()
    spy.mockRestore()
  })
})
