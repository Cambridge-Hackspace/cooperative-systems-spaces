import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * The tool forms submit whatever is in their `form` object. The server accepts
 * whatever is in `CreateToolRequest` / `UpdateToolRequest`. Nothing connected
 * the two, and three defects grew in the gap -- all three silent, because serde
 * ignores an unknown key and answers 200:
 *
 *   - `manufacturer` and `model` existed in both modals, in the frontend `Tool`
 *     type, and on a `ToolCard` row that displayed them. There is no such
 *     column and no such request field. The inputs discarded whatever was typed
 *     and the card could never render.
 *   - The Notes textarea bound `notes`; the column and the request field are
 *     `maintenance_notes`. Notes never saved, with a success toast.
 *   - `external_id` was in the edit form and absent from the create form, so a
 *     tool could only get the identifier firmware is configured with by being
 *     created and then edited.
 *
 * Which direction matters: a form key the API does not accept is dropped in
 * silence, so the operator believes they saved something they did not. That is
 * the hard rule here. The reverse -- an API field no form offers -- is not a
 * defect (plenty of fields are set elsewhere, or deliberately not exposed), so
 * it is not asserted.
 *
 * Read as text rather than by importing the components, for the same reason the
 * other specs in this tier do: the question is what the source says, and a
 * mounted component would answer about a rendered instance instead.
 */

const root = join(__dirname, '..', '..', '..')
const read = (p: string) => readFileSync(join(root, p), 'utf8')

const CREATE_MODAL = 'frontend/src/components/ToolCreateModal.vue'
const EDIT_MODAL = 'frontend/src/components/ToolEditModal.vue'
const TOOLS_API = 'server/src/api/tools.rs'

/**
 * The keys of the `const form = ref(...)` object literal in a modal.
 *
 * Scoped to that one literal: a component holds other objects, and collecting
 * every `key:` in the file would report local state and props as submitted
 * fields. Anchored on `const form = ` and closed at the `})` that ends the
 * `ref(`, which is the shape both modals use.
 */
function formKeys(src: string): string[] {
  const start = src.indexOf('const form = ')
  expect(start, 'neither modal shape found: `const form = ` is gone').toBeGreaterThan(-1)
  const open = src.indexOf('{', start)
  const close = src.indexOf('\n})', open)
  expect(close, 'could not find the end of the form literal').toBeGreaterThan(open)
  const body = src.slice(open, close)

  const keys: string[] = []
  for (const line of body.split('\n')) {
    const name = /^\s{2}([a-z_][a-z0-9_]*):/i.exec(line)?.[1]
    if (name) keys.push(name)
  }
  expect(keys.length, 'extracted no form keys; the literal shape changed').toBeGreaterThan(5)
  return keys
}

/**
 * The serde field names of one request struct in `api/tools.rs`.
 *
 * Takes `pub <name>:` lines between the struct declaration and its closing
 * brace in column zero. `#[serde(...)]` attribute lines are skipped, so a
 * `deserialize_with` on a clearable field does not read as a field.
 */
function structFields(src: string, name: string): string[] {
  const decl = `pub struct ${name} {`
  const start = src.indexOf(decl)
  expect(start, `${name} not found in ${TOOLS_API}`).toBeGreaterThan(-1)
  const close = src.indexOf('\n}', start)
  const body = src.slice(start + decl.length, close)

  const fields: string[] = []
  for (const line of body.split('\n')) {
    const name = /^\s*pub ([a-z_][a-z0-9_]*):/.exec(line)?.[1]
    if (name) fields.push(name)
  }
  expect(fields.length, `extracted no fields from ${name}`).toBeGreaterThan(5)
  return fields
}

/**
 * Form keys that are the component's own business rather than payload.
 *
 * `schedule_id` IS sent and IS accepted, so it is not here. This list exists
 * for keys a form carries for its own bookkeeping; it is empty today, and
 * stated explicitly so that adding to it is a visible decision rather than a
 * quiet exclusion.
 */
const NOT_SUBMITTED: string[] = []

describe('the tool forms only submit fields the API accepts', () => {
  const tools = read(TOOLS_API)

  it('every create-form key is a CreateToolRequest field', () => {
    const accepted = new Set(structFields(tools, 'CreateToolRequest'))
    const unknown = formKeys(read(CREATE_MODAL))
      .filter((k) => !NOT_SUBMITTED.includes(k))
      .filter((k) => !accepted.has(k))

    expect(
      unknown,
      `the create form submits keys POST /api/tools does not accept: ${unknown.join(', ')}. ` +
        `serde drops them and answers 200, so the operator is told it saved.`
    ).toEqual([])
  })

  it('every edit-form key is an UpdateToolRequest field', () => {
    const accepted = new Set(structFields(tools, 'UpdateToolRequest'))
    const unknown = formKeys(read(EDIT_MODAL))
      .filter((k) => !NOT_SUBMITTED.includes(k))
      .filter((k) => !accepted.has(k))

    expect(
      unknown,
      `the edit form submits keys PUT /api/tools/{id} does not accept: ${unknown.join(', ')}. ` +
        `serde drops them and answers 200, so the operator is told it saved.`
    ).toEqual([])
  })

  /**
   * The specific regression, named rather than left to the general rule: the
   * identifier firmware is configured with must be settable at creation. It
   * was edit-only, which meant provisioning hardware required two round trips
   * through a form that was itself broken.
   */
  it('external_id is offered on create, not only on edit', () => {
    for (const [label, path] of [
      ['create', CREATE_MODAL],
      ['edit', EDIT_MODAL],
    ] as const) {
      const src = read(path)
      expect(formKeys(src), `${label} form must carry external_id`).toContain('external_id')
      expect(src, `${label} form must render an External ID input`).toContain('id="external_id"')
    }
  })

  /**
   * The self-test. A check that reads files passes vacuously the moment an
   * anchor moves, so prove the comparison discriminates by feeding it the
   * defect it was written for.
   */
  it('rejects a form key with no counterpart on the server', () => {
    const accepted = new Set(structFields(tools, 'UpdateToolRequest'))
    const mutated = read(EDIT_MODAL).replace(
      /^(const form = ref\(\{\n)/m,
      "$1  manufacturer: '',\n"
    )
    expect(mutated, 'the mutant did not apply; the form literal shape changed').toContain(
      'manufacturer'
    )
    const unknown = formKeys(mutated).filter((k) => !accepted.has(k))
    expect(unknown, 'a phantom field must be caught').toContain('manufacturer')
  })

  it('rejects the notes/maintenance_notes mismatch specifically', () => {
    const accepted = new Set(structFields(tools, 'UpdateToolRequest'))
    expect(accepted.has('maintenance_notes'), 'the server field is maintenance_notes').toBe(true)
    expect(accepted.has('notes'), 'the server has no `notes` field on a tool').toBe(false)

    const mutated = read(EDIT_MODAL).replace('  maintenance_notes:', '  notes:')
    expect(mutated).toContain('  notes:')
    const unknown = formKeys(mutated).filter((k) => !accepted.has(k))
    expect(unknown, 'binding `notes` must be caught').toContain('notes')
  })
})
