// Profile validation, as a pure function.
//
// Extracted from `stores/profile.ts`, where it was a closure over the store's
// `profileConfig`. Behaviour is unchanged: the store now delegates and passes
// its own config in.
//
// The reason it moved is that two other things need it and neither can reach a
// Pinia store:
//
//   * Tier 5's fake API. It is a Vite *plugin*, so it is evaluated by Node when
//     the config loads -- before the `@/` alias exists. Anything it imports has
//     to resolve without the alias, and `stores/profile.ts` imports
//     `@/utils/api`. This module imports only types.
//   * anything that wants to check a profile without mounting an application.
//
// The alternative was a second copy of these rules inside the fake, which would
// agree with itself no matter what the application did -- the exact failure
// that makes fake-API tiers worthless.
//
// It is also the shape the rules should have had anyway: given a profile and a
// field list, is it valid? Nothing about that needs a store.

// A *relative* type import, deliberately. This module is imported by the tier-5
// fake, which is a Vite plugin evaluated by Node before the `@/` alias exists --
// so an aliased import here would put the alias back in the one place it cannot
// work. It is a type-only import either way, so it costs nothing at runtime.
import type { ProfileField } from '../types'

export interface ValidationResult {
  valid: boolean
  errors: string[]
}

export function validateProfileAgainst(
  profileData: Record<string, unknown>,
  fields: ProfileField[] | null | undefined
): ValidationResult {
  const errors: string[] = []

  // No configuration means nothing to validate against, not "everything is
  // invalid". A profile page that loaded before its config arrives must not
  // report every field as wrong.
  if (!fields) {
    return { valid: true, errors: [] }
  }

  for (const field of fields) {
    const value = profileData[field.key]

    // "Required" means the value was provided, not that it is truthy.
    //
    // This used to test `!value`, which made `false` and `0` indistinguishable
    // from a missing field: a required Boolean could not be saved as `false`
    // and a required Number could not be saved as `0`. The form reported "is
    // required" for a value the user had explicitly given, and one the server
    // would have accepted — `ProfileValidator::validate_profile` in
    // server/src/profile.rs only errors when the key is *absent* from the
    // object. A waiver checkbox that must be answered "no" was unsaveable.
    const isAbsent =
      value === undefined || value === null || (typeof value === 'string' && value.trim() === '')

    if (field.required && isAbsent) {
      errors.push(`${field.label} is required`)
      continue
    }

    // Nothing to type-check when an optional field was left blank.
    if (isAbsent) continue

    // Narrow exactly as ProfileField.vue does, so the validator and the
    // renderer never disagree about what a field is. This previously treated
    // *every* non-string field_type as a Select; the Select branch then
    // re-checked the shape, found it wrong, and silently validated nothing —
    // under a label claiming the field had been checked.
    const fieldType =
      typeof field.field_type === 'string'
        ? field.field_type
        : typeof field.field_type === 'object' &&
            field.field_type !== null &&
            'Select' in field.field_type
          ? 'Select'
          : 'Text'

    switch (fieldType) {
      case 'Email':
        if (typeof value !== 'string' || !value.includes('@')) {
          errors.push(`${field.label} must be a valid email address`)
        }
        break

      case 'Phone':
        if (typeof value !== 'string' || value.length < 7) {
          errors.push(`${field.label} must be a valid phone number`)
        }
        break

      case 'Number':
        if (typeof value !== 'number' && isNaN(Number(value))) {
          errors.push(`${field.label} must be a valid number`)
        }
        break

      case 'Date':
        if (typeof value === 'string') {
          const dateRegex = /^\d{4}-\d{2}-\d{2}$/
          if (!dateRegex.test(value)) {
            errors.push(`${field.label} must be in YYYY-MM-DD format`)
          }
        }
        break

      case 'Boolean':
        if (typeof value !== 'boolean') {
          errors.push(`${field.label} must be true or false`)
        }
        break

      case 'Select':
        if (typeof field.field_type === 'object' && 'Select' in field.field_type) {
          const options = field.field_type.Select.options
          // A Select value is a string, a number or a boolean. `String(value)`
          // on an object gives "[object Object]", which is never in the option
          // list -- so the message would say "must be one of: a, b" about a
          // value that is not the wrong option but the wrong *kind* of thing.
          const chosen =
            typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean'
              ? String(value)
              : null
          if (chosen === null || !options.includes(chosen)) {
            errors.push(`${field.label} must be one of: ${options.join(', ')}`)
          }
        }
        break
    }
  }

  return {
    valid: errors.length === 0,
    errors,
  }
}
