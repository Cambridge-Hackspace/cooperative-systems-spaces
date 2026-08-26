<template>
  <div class="card bg-base-100 shadow-xl">
    <div class="card-body">
      <div class="flex items-center justify-between mb-6">
        <h2 class="card-title text-2xl">
          <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"
            />
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
            />
          </svg>
          Profile Configuration
        </h2>

        <div class="flex gap-2">
          <button
            v-if="!isEditing"
            class="btn btn-primary btn-sm"
            :disabled="loading"
            @click="startEditing"
          >
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z"
              />
            </svg>
            Edit Configuration
          </button>

          <button
            v-if="isEditing"
            class="btn btn-ghost btn-sm"
            :disabled="loading"
            @click="cancelEditing"
          >
            Cancel
          </button>

          <button
            v-if="isEditing"
            class="btn btn-primary btn-sm"
            :disabled="loading || !isFormValid"
            @click="saveConfiguration"
          >
            <span v-if="loading" class="loading loading-spinner loading-sm"></span>
            <svg v-else class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M5 13l4 4L19 7"
              />
            </svg>
            Save Configuration
          </button>
        </div>
      </div>

      <!-- Loading state -->
      <div v-if="loading && !profileConfig" class="flex items-center justify-center py-12">
        <div class="loading loading-spinner loading-lg"></div>
        <span class="ml-3">Loading configuration...</span>
      </div>

      <!-- Error state -->
      <div v-else-if="error && !profileConfig" class="alert alert-error">
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
          />
        </svg>
        <div>
          <h3 class="font-bold">Error loading configuration</h3>
          <div class="text-xs">{{ error }}</div>
        </div>
        <button class="btn btn-sm" @click="loadConfiguration">Retry</button>
      </div>

      <!-- Configuration form -->
      <div v-else class="space-y-8">
        <!-- Global Settings -->
        <div class="card bg-base-200">
          <div class="card-body">
            <h3 class="card-title text-lg mb-4">Global Settings</h3>

            <div class="form-control">
              <label class="label cursor-pointer justify-start gap-4">
                <input
                  v-model="editConfig.profiles_enabled"
                  type="checkbox"
                  class="checkbox checkbox-primary"
                  :disabled="!isEditing"
                />
                <div>
                  <div class="label-text font-medium">Enable User Profiles</div>
                  <div class="label-text-alt">Allow users to manage their profile information</div>
                </div>
              </label>
            </div>
          </div>
        </div>

        <!-- Profile Fields -->
        <div class="card bg-base-200">
          <div class="card-body">
            <div class="flex items-center justify-between mb-4">
              <h3 class="card-title text-lg">Profile Fields</h3>
              <button v-if="isEditing" class="btn btn-primary btn-sm" @click="addField">
                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    stroke-width="2"
                    d="M12 4v16m8-8H4"
                  />
                </svg>
                Add Field
              </button>
            </div>

            <!-- Validation errors -->
            <div v-if="validationErrors.length > 0" class="alert alert-error mb-4">
              <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                />
              </svg>
              <div>
                <h3 class="font-bold">Please fix the following errors:</h3>
                <ul class="text-xs list-disc list-inside">
                  <li v-for="fieldError in validationErrors" :key="fieldError">{{ fieldError }}</li>
                </ul>
              </div>
            </div>

            <!-- Field list -->
            <div v-if="editConfig.profile_fields.length > 0" class="space-y-4">
              <div
                v-for="(field, index) in editConfig.profile_fields"
                :key="index"
                class="card bg-base-100 border border-base-300"
              >
                <div class="card-body p-4">
                  <div class="flex items-center justify-between mb-4">
                    <h4 class="font-semibold">Field {{ index + 1 }}</h4>
                    <button
                      v-if="isEditing"
                      class="btn btn-error btn-xs"
                      @click="removeField(index)"
                    >
                      <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path
                          stroke-linecap="round"
                          stroke-linejoin="round"
                          stroke-width="2"
                          d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"
                        />
                      </svg>
                      Remove
                    </button>
                  </div>

                  <div class="grid gap-4 md:grid-cols-2">
                    <!-- Field Key -->
                    <div class="form-control">
                      <label class="label">
                        <span class="label-text font-medium">Field Key *</span>
                      </label>
                      <input
                        v-model="field.key"
                        type="text"
                        placeholder="e.g. phone, bio, skills"
                        class="input input-bordered input-sm"
                        :disabled="!isEditing"
                        @input="validateConfiguration"
                      />
                    </div>

                    <!-- Field Label -->
                    <div class="form-control">
                      <label class="label">
                        <span class="label-text font-medium">Display Label *</span>
                      </label>
                      <input
                        v-model="field.label"
                        type="text"
                        placeholder="e.g. Phone Number, Bio, Skills"
                        class="input input-bordered input-sm"
                        :disabled="!isEditing"
                        @input="validateConfiguration"
                      />
                    </div>

                    <!-- Field Type -->
                    <div class="form-control">
                      <label class="label">
                        <span class="label-text font-medium">Field Type *</span>
                      </label>
                      <select
                        v-model="field.field_type"
                        class="select select-bordered select-sm"
                        :disabled="!isEditing"
                        @change="
                          handleFieldTypeChange(field, ($event.target as HTMLSelectElement)?.value)
                        "
                      >
                        <option value="">Select type...</option>
                        <option value="Text">Text</option>
                        <option value="TextArray">Text (list / multiple values)</option>
                        <option value="Email">Email</option>
                        <option value="Phone">Phone</option>
                        <option value="Number">Number</option>
                        <option value="Date">Date</option>
                        <option value="Boolean">Boolean</option>
                        <option value="Select">Select (Dropdown)</option>
                      </select>
                    </div>

                    <!-- Required -->
                    <div class="form-control">
                      <label class="label cursor-pointer justify-start gap-4">
                        <input
                          v-model="field.required"
                          type="checkbox"
                          class="checkbox checkbox-sm"
                          :disabled="!isEditing"
                        />
                        <span class="label-text">Required Field</span>
                      </label>
                    </div>

                    <!-- Select Options (only for Select type) -->
                    <div v-if="isSelectType(field.field_type)" class="form-control md:col-span-2">
                      <label class="label">
                        <span class="label-text font-medium">Select Options *</span>
                      </label>
                      <div class="space-y-2">
                        <div
                          v-for="(option, optionIndex) in getSelectOptions(field.field_type)"
                          :key="optionIndex"
                          class="flex items-center gap-2"
                        >
                          <input
                            :value="option"
                            type="text"
                            placeholder="Option text"
                            class="input input-bordered input-sm flex-1"
                            :disabled="!isEditing"
                            @input="
                              updateSelectOption(
                                field.field_type,
                                optionIndex,
                                ($event.target as HTMLInputElement)?.value
                              )
                            "
                          />
                          <button
                            v-if="isEditing"
                            class="btn btn-error btn-xs"
                            @click="removeSelectOption(field.field_type, optionIndex)"
                          >
                            <svg
                              class="w-3 h-3"
                              fill="none"
                              stroke="currentColor"
                              viewBox="0 0 24 24"
                            >
                              <path
                                stroke-linecap="round"
                                stroke-linejoin="round"
                                stroke-width="2"
                                d="M6 18L18 6M6 6l12 12"
                              />
                            </svg>
                          </button>
                        </div>
                        <button
                          v-if="isEditing"
                          class="btn btn-ghost btn-sm"
                          @click="addSelectOption(field.field_type)"
                        >
                          <svg
                            class="w-4 h-4"
                            fill="none"
                            stroke="currentColor"
                            viewBox="0 0 24 24"
                          >
                            <path
                              stroke-linecap="round"
                              stroke-linejoin="round"
                              stroke-width="2"
                              d="M12 4v16m8-8H4"
                            />
                          </svg>
                          Add Option
                        </button>
                      </div>
                    </div>

                    <!-- Help Text -->
                    <div class="form-control md:col-span-2">
                      <label class="label">
                        <span class="label-text font-medium">Help Text</span>
                      </label>
                      <textarea
                        v-model="field.help_text"
                        placeholder="Optional help text for users"
                        class="textarea textarea-bordered textarea-sm h-20"
                        :disabled="!isEditing"
                      ></textarea>
                    </div>
                  </div>
                </div>
              </div>
            </div>

            <!-- No fields message -->
            <div v-else class="text-center py-8">
              <svg
                class="w-16 h-16 mx-auto text-base-content/30 mb-4"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"
                />
              </svg>
              <h3 class="text-lg font-medium text-base-content/70 mb-2">No Profile Fields</h3>
              <p class="text-base-content/50 mb-4">
                Add profile fields to allow users to customize their profiles.
              </p>
              <button v-if="isEditing" class="btn btn-primary" @click="addField">
                Add Your First Field
              </button>
            </div>
          </div>
        </div>

        <!-- Preview -->
        <div v-if="editConfig.profile_fields.length > 0" class="card bg-base-200">
          <div class="card-body">
            <h3 class="card-title text-lg mb-4">Preview</h3>
            <div class="text-sm text-base-content/70 mb-4">
              This is how the profile form will look to users:
            </div>

            <div class="card bg-base-100">
              <div class="card-body">
                <div class="grid gap-4 md:grid-cols-2">
                  <div
                    v-for="field in editConfig.profile_fields"
                    :key="field.key"
                    class="form-control"
                  >
                    <label class="label">
                      <span class="label-text font-medium">
                        {{ field.label }}
                        <span v-if="field.required" class="text-error">*</span>
                      </span>
                    </label>
                    <div v-if="field.help_text" class="text-xs text-base-content/60 mb-1">
                      {{ field.help_text }}
                    </div>
                    <div class="text-sm text-base-content/50">
                      {{ getFieldTypeDisplay(field.field_type) }} field
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useProfileStore } from '@/stores/profile'
import type { ProfileField, ProfileFieldType, UpdateProfileConfigRequest } from '@/types'

const profileStore = useProfileStore()

// Local state
const isEditing = ref(false)
const editConfig = ref<UpdateProfileConfigRequest>({
  profiles_enabled: false,
  profile_fields: [],
})
const validationErrors = ref<string[]>([])

// Computed properties
const profileConfig = computed(() => profileStore.profileConfig)
const loading = computed(() => profileStore.loading)
const error = computed(() => profileStore.error)

const isFormValid = computed(() => {
  return validationErrors.value.length === 0
})

// Methods
function addField() {
  editConfig.value.profile_fields.push({
    key: '',
    label: '',
    field_type: 'Text' as ProfileFieldType,
    required: false,
    help_text: '',
  })
}

function removeField(index: number) {
  editConfig.value.profile_fields.splice(index, 1)
  validateConfiguration()
}

function handleFieldTypeChange(field: ProfileField, newType: string) {
  if (newType === 'Select') {
    field.field_type = { Select: { options: ['Option 1', 'Option 2'] } }
  } else {
    field.field_type = newType as ProfileFieldType
  }
  validateConfiguration()
}

function isSelectType(fieldType: any): boolean {
  return typeof fieldType === 'object' && fieldType && 'Select' in fieldType
}

function getSelectOptions(fieldType: any): string[] {
  if (isSelectType(fieldType)) {
    return fieldType.Select.options || []
  }
  return []
}

function updateSelectOption(fieldType: any, index: number, value: string) {
  if (isSelectType(fieldType)) {
    fieldType.Select.options[index] = value
    validateConfiguration()
  }
}

function addSelectOption(fieldType: any) {
  if (isSelectType(fieldType)) {
    fieldType.Select.options.push('New Option')
  }
}

function removeSelectOption(fieldType: any, index: number) {
  if (isSelectType(fieldType)) {
    fieldType.Select.options.splice(index, 1)
    validateConfiguration()
  }
}

function getFieldTypeDisplay(fieldType: any): string {
  if (typeof fieldType === 'string') {
    return fieldType
  } else if (typeof fieldType === 'object' && 'Select' in fieldType) {
    return 'Select'
  }
  return 'Text'
}

function validateConfiguration() {
  const errors: string[] = []
  const usedKeys = new Set<string>()

  for (let i = 0; i < editConfig.value.profile_fields.length; i++) {
    const field = editConfig.value.profile_fields[i]
    const fieldNum = i + 1

    // Required field validations
    if (!field.key || field.key.trim() === '') {
      errors.push(`Field ${fieldNum}: Key is required`)
    } else if (usedKeys.has(field.key)) {
      errors.push(`Field ${fieldNum}: Key "${field.key}" is already used`)
    } else {
      usedKeys.add(field.key)
    }

    if (!field.label || field.label.trim() === '') {
      errors.push(`Field ${fieldNum}: Label is required`)
    }

    if (!field.field_type) {
      errors.push(`Field ${fieldNum}: Field type is required`)
    } else if (isSelectType(field.field_type)) {
      const options = getSelectOptions(field.field_type)
      if (options.length === 0) {
        errors.push(`Field ${fieldNum}: Select fields must have at least one option`)
      } else {
        const emptyOptions = options.filter((opt) => !opt || opt.trim() === '')
        if (emptyOptions.length > 0) {
          errors.push(`Field ${fieldNum}: All select options must have values`)
        }
      }
    }
  }

  validationErrors.value = errors
}

function startEditing() {
  isEditing.value = true
  // Deep copy current configuration
  editConfig.value = {
    profiles_enabled: profileConfig.value?.profiles_enabled || false,
    profile_fields: JSON.parse(JSON.stringify(profileConfig.value?.profile_fields || [])),
  }
  validationErrors.value = []
}

function cancelEditing() {
  isEditing.value = false
  editConfig.value = {
    profiles_enabled: false,
    profile_fields: [],
  }
  validationErrors.value = []
  profileStore.clearError()
}

async function saveConfiguration() {
  validateConfiguration()

  if (!isFormValid.value) {
    return
  }

  try {
    await profileStore.updateProfileConfig(editConfig.value)
    isEditing.value = false
    validationErrors.value = []
  } catch (err) {
    // Logged rather than discarded: a swallowed error is indistinguishable
    // from a successful no-op to anyone reading the console.
    console.error(err)
    // Error is already handled by the store
  }
}

async function loadConfiguration() {
  try {
    await profileStore.fetchProfileConfig()
  } catch (err) {
    // Logged rather than discarded: a swallowed error is indistinguishable
    // from a successful no-op to anyone reading the console.
    console.error(err)
    // Error is already handled by the store
  }
}

// Lifecycle
onMounted(async () => {
  await loadConfiguration()
})
</script>
