// Tier 2: EditTrainerModal.
//
// Three fields and a submit, and the submit is where the interesting part is.
// The server declares the expiry as a *double* Option --
//
//     pub expires_at: Option<Option<DateTime<Utc>>>,   // api/trainers.rs:31
//     #[diesel(... treat_none_as_null = false)]        // models/trainers.rs:36
//
// -- which is the deliberate three-state encoding: field absent means "leave
// it alone", JSON `null` means "clear it", a timestamp means "set it". The
// attribute is written out explicitly, so this is a capability the server
// means to offer.
//
// This modal cannot reach two of the three states. It sends
// `formData.expires_at || undefined`, and `undefined` serialises to *absent* --
// so the field labelled "Leave blank for no expiration" cannot remove an
// expiration. And when a date is chosen it sends `"2026-03-01"`, a date with no
// time, where the server asks for an RFC 3339 timestamp.
//
// What this spec does NOT prove: what the server actually answers to either
// payload. Tier 2's remit is the bytes this component puts on the wire; Tier 4
// owns the response. Both findings are pinned to the wire shape.

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const mocks = vi.hoisted(() => ({ updateToolTrainer: vi.fn() }))
vi.mock('@/utils/api', () => ({ trainerApi: mocks }))

import EditTrainerModal from '@/components/EditTrainerModal.vue'
import type { Tool } from '@/types'
import type { ToolTrainerWithUser } from '@/types/training'

const TOOL = { id: 'tool-1', name: 'Lathe' } as unknown as Tool

function trainer(over: Partial<ToolTrainerWithUser['trainer']> = {}): ToolTrainerWithUser {
  return {
    trainer: {
      id: 'tt-1',
      user_id: 'user-1',
      tool_id: 'tool-1',
      authorized_by: 'admin-1',
      authorized_at: '2025-06-01T00:00:00Z',
      notes: 'Signed off on the safety module.',
      expires_at: '2026-03-01T00:00:00Z',
      is_active: true,
      created_at: '2025-06-01T00:00:00Z',
      updated_at: '2025-06-01T00:00:00Z',
      ...over,
    },
    user_name: 'ada',
    user_email: 'ada@example.test',
    user_full_name: 'Ada Lovelace',
  }
}

beforeEach(() => {
  mocks.updateToolTrainer.mockReset()
  mocks.updateToolTrainer.mockResolvedValue({ success: true })
})

const modal = (t: ToolTrainerWithUser = trainer()) =>
  mount(EditTrainerModal, { props: { tool: TOOL, trainerWithUser: t } })

type Wrapper = ReturnType<typeof modal>

function buttonNamed(w: Wrapper, label: string) {
  const b = w.findAll('button').find((btn) => btn.text().trim() === label)
  if (!b) throw new Error(`no button labelled ${JSON.stringify(label)}`)
  return b
}

const sent = () => mocks.updateToolTrainer.mock.calls[0][2] as Record<string, unknown>

describe('who is being edited', () => {
  it('leads with the full name and the email', () => {
    const w = modal()
    expect(w.find('.trainer-info h4').text()).toBe('Ada Lovelace')
    expect(w.find('.trainer-email').text()).toBe('ada@example.test')
  })

  it('falls back to the username when there is no full name', () => {
    const t = trainer()
    t.user_full_name = undefined
    expect(modal(t).find('.trainer-info h4').text()).toBe('ada')
  })
})

describe('what the form starts with', () => {
  it('shows the existing notes, expiry and active flag', () => {
    const w = modal()
    expect((w.find('#notes').element as HTMLTextAreaElement).value).toBe(
      'Signed off on the safety module.'
    )
    expect((w.find('#expires_at').element as HTMLInputElement).value).toBe('2026-03-01')
    expect((w.find('.checkbox').element as HTMLInputElement).checked).toBe(true)
  })

  it('starts blank when the assignment has no expiry and no notes', () => {
    const w = modal(trainer({ expires_at: undefined, notes: undefined, is_active: false }))
    expect((w.find('#expires_at').element as HTMLInputElement).value).toBe('')
    expect((w.find('#notes').element as HTMLTextAreaElement).value).toBe('')
    expect((w.find('.checkbox').element as HTMLInputElement).checked).toBe(false)
  })

  // The clock is frozen at 2026-01-15T12:00:00Z by `tests/setup.ts`, so this is
  // a constant rather than a function of when the suite ran.
  //
  // FINDING, pinned. `today` is `new Date().toISOString().split('T')[0]`, which
  // is the date in *UTC*, not the user's date. For anyone west of UTC the two
  // disagree for the last hours of their day: at 02:00Z on the 16th it is still
  // the 15th in Chicago, and `min` is set to the 16th -- so a trainer cannot be
  // given an expiry of today.
  // FIXED. `today` used to be `toISOString().split('T')[0]`, the UTC date, so
  // west of UTC a trainer could not be given an expiry of today.
  it("floors the date picker at the user's date", () => {
    expect(modal().find('#expires_at').attributes('min')).toBe('2026-01-15')

    // 02:00Z on the 16th is 20:00 on the 15th in the suite's timezone, which
    // `vitest.config.ts` pins to America/Chicago precisely so this distinction
    // exists. Under UTC the fixed and the broken implementation agree, and the
    // test would prove nothing.
    vi.setSystemTime(new Date('2026-01-16T02:00:00Z'))
    expect(new Date().getDate(), 'the suite timezone is not what this test assumes').toBe(15)
    expect(modal().find('#expires_at').attributes('min')).toBe('2026-01-15')
    vi.setSystemTime(new Date('2026-01-15T12:00:00.000Z'))
  })
})

describe('what the form sends', () => {
  it('addresses the right tool and the right trainer', async () => {
    const w = modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(mocks.updateToolTrainer.mock.calls[0][0]).toBe('tool-1')
    expect(mocks.updateToolTrainer.mock.calls[0][1]).toBe('user-1')
  })

  it('sends the edited notes and active flag', async () => {
    const w = modal()
    await w.find('#notes').setValue('Renewed after the refresher.')
    await w.find('.checkbox').setValue(false)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(sent().notes).toBe('Renewed after the refresher.')
    expect(sent().is_active).toBe(false)
  })

  // FINDING, pinned. The `<input type="date">` yields a date with no time, and
  // it is forwarded verbatim. The server's field is `DateTime<Utc>`, whose
  // serde implementation wants RFC 3339 -- "2026-03-01" is not one, so this is
  // a payload the server cannot deserialise. Nothing in this component turns
  // the date into a timestamp, and nothing in the handler coerces it.
  it('sends a bare calendar date where the server declares a timestamp', async () => {
    const w = modal()
    await w.find('#expires_at').setValue('2026-04-01')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(
      sent().expires_at,
      'the expiry is now sent as a timestamp -- if that was fixed, delete this ' +
        'test; the server field is Option<Option<DateTime<Utc>>> and wants RFC 3339'
    ).toBe('2026-04-01')
  })

  // FINDING, pinned, and the one with a user-visible consequence. The label
  // under the field reads "Leave blank for no expiration". Blanking it sends
  // `undefined`, which `JSON.stringify` drops entirely -- and an absent field
  // is exactly the server's "leave it alone". The server's `Some(None)` state,
  // which clears the column, is reachable only by sending an explicit `null`,
  // and this component never sends one.
  it('cannot clear an expiry, despite the field saying it can', async () => {
    const w = modal()
    expect(w.text()).toContain('Leave blank for no expiration')

    await w.find('#expires_at').setValue('')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(
      Object.prototype.hasOwnProperty.call(sent(), 'expires_at') && sent().expires_at !== undefined,
      'the modal now sends something for a blanked expiry -- if `null` was ' +
        'added, delete this test and assert the null instead'
    ).toBe(false)
    expect(JSON.parse(JSON.stringify(sent()))).not.toHaveProperty('expires_at')
  })

  it('cannot clear the notes either, for the same reason', async () => {
    // Recorded rather than pinned as a defect of this component: the server's
    // `notes` is a single `Option<String>` under `treat_none_as_null = false`,
    // so it has no "clear it" state to send. The gap is end-to-end, and fixing
    // it here alone would achieve nothing.
    const w = modal()
    await w.find('#notes').setValue('')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(JSON.parse(JSON.stringify(sent()))).not.toHaveProperty('notes')
  })
})

describe('what happens after the request', () => {
  it('emits updated and shows no error when the server agrees', async () => {
    const w = modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.emitted('updated')).toHaveLength(1)
    expect(w.find('.error').exists()).toBe(false)
  })

  it("shows the server's reason for a refusal, and does not emit updated", async () => {
    mocks.updateToolTrainer.mockResolvedValue({ success: false, error: 'Not a trainer' })
    const w = modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error').text()).toBe('Not a trainer')
    expect(w.emitted('updated')).toBeUndefined()
  })

  it('falls back to a generic message when a refusal carries no reason', async () => {
    mocks.updateToolTrainer.mockResolvedValue({ success: false })
    const w = modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error').text()).toBe('Failed to update trainer')
  })

  // Same shape as StartTrainingModal: `err.message` is the axios prose, and
  // the server's own words at `err.response.data.error` are discarded. Recorded
  // once per component because the fix is per-component.
  it('shows axios prose rather than the server body on a thrown error', async () => {
    mocks.updateToolTrainer.mockRejectedValue(
      Object.assign(new Error('Request failed with status code 422'), {
        response: { data: { error: 'expires_at: premature end of input' } },
      })
    )
    const w = modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('.error').text()).toBe('Request failed with status code 422')
    expect(w.text()).not.toContain('premature end of input')
  })

  it('re-enables the submit button whether the request succeeded or failed', async () => {
    mocks.updateToolTrainer.mockRejectedValue(new Error('down'))
    const w = modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('button[type="submit"]').attributes('disabled')).toBeUndefined()
    expect(w.find('button[type="submit"]').text()).toBe('Update Trainer')
  })

  it('disables the submit button and says so while the request is in flight', async () => {
    mocks.updateToolTrainer.mockReturnValue(new Promise(() => {}))
    const w = modal()
    await w.find('form').trigger('submit')
    await flushPromises()

    const submit = w.find('button[type="submit"]')
    expect(submit.attributes('disabled')).toBeDefined()
    expect(submit.text()).toBe('Updating...')
  })

  it('clears a previous error when the form is submitted again', async () => {
    mocks.updateToolTrainer.mockResolvedValue({ success: false, error: 'Not a trainer' })
    const w = modal()
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(w.find('.error').exists()).toBe(true)

    mocks.updateToolTrainer.mockReturnValue(new Promise(() => {}))
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(w.find('.error').exists()).toBe(false)
  })
})

describe('closing', () => {
  it('closes on the overlay, the header button and Cancel', async () => {
    const w = modal()
    await w.find('.modal-overlay').trigger('click')
    await w.find('.close-btn').trigger('click')
    await buttonNamed(w, 'Cancel').trigger('click')
    expect(w.emitted('close')).toHaveLength(3)
  })

  it('does not close when the modal body itself is clicked', async () => {
    const w = modal()
    await w.find('.modal').trigger('click')
    expect(w.emitted('close')).toBeUndefined()
  })
})
