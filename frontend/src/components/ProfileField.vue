<template>
  <div class="form-control w-full">
    <label class="label">
      <span class="label-text font-medium">
        {{ field.label }}
        <span v-if="field.required" class="text-error">*</span>
      </span>
      <span
        v-if="field.help_text"
        class="label-text-alt tooltip tooltip-left"
        :data-tip="field.help_text"
      >
        <svg
          class="w-4 h-4 text-base-content/70"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
          />
        </svg>
      </span>
    </label>

    <!-- Text Field -->
    <input
      v-if="fieldType === 'Text'"
      v-model="internalValue"
      type="text"
      :placeholder="field.label"
      class="input input-bordered w-full"
      :class="{ 'input-error': hasError }"
      :disabled="disabled"
      @blur="$emit('blur')"
    />

    <!-- TextArray Field (chip input) -->
    <div
      v-else-if="fieldType === 'TextArray'"
      class="input input-bordered w-full h-auto min-h-12 flex flex-wrap items-center gap-1 py-2 cursor-text"
      :class="{ 'input-error': hasError, 'opacity-60': disabled }"
      @click="focusTagInput"
    >
      <span v-for="(tag, i) in tagValues" :key="`${tag}-${i}`" class="badge badge-neutral gap-1">
        {{ tag }}
        <button
          type="button"
          class="opacity-70 hover:opacity-100"
          :disabled="disabled"
          aria-label="Remove"
          @click.stop="removeTag(i)"
        >
          &times;
        </button>
      </span>
      <input
        ref="tagInputRef"
        v-model="pendingTag"
        type="text"
        class="flex-1 min-w-[8rem] bg-transparent outline-none border-0 p-0 focus:ring-0"
        :placeholder="tagValues.length ? '' : field.help_text || 'type and press Enter'"
        :disabled="disabled"
        @keydown.enter.prevent="commitTag()"
        @keydown="onTagKeydown"
        @blur="onTagBlur"
      />
    </div>

    <!-- Email Field -->
    <input
      v-else-if="fieldType === 'Email'"
      v-model="internalValue"
      type="email"
      :placeholder="`Enter ${field.label.toLowerCase()}`"
      class="input input-bordered w-full"
      :class="{ 'input-error': hasError }"
      :disabled="disabled"
      @blur="$emit('blur')"
    />

    <!-- Phone Field -->
    <input
      v-else-if="fieldType === 'Phone'"
      v-model="internalValue"
      type="tel"
      placeholder="+1-555-123-4567"
      class="input input-bordered w-full"
      :class="{ 'input-error': hasError }"
      :disabled="disabled"
      @blur="$emit('blur')"
    />

    <!-- Number Field -->
    <input
      v-else-if="fieldType === 'Number'"
      :value="numberValue"
      type="number"
      :placeholder="field.label"
      class="input input-bordered w-full"
      :class="{ 'input-error': hasError }"
      :disabled="disabled"
      @input="updateNumberValue"
      @blur="$emit('blur')"
    />

    <!-- Date Field -->
    <input
      v-else-if="fieldType === 'Date'"
      v-model="internalValue"
      type="date"
      class="input input-bordered w-full"
      :class="{ 'input-error': hasError }"
      :disabled="disabled"
      @blur="$emit('blur')"
    />

    <!-- Boolean Field -->
    <div v-else-if="fieldType === 'Boolean'" class="form-control">
      <label class="label cursor-pointer justify-start gap-4">
        <input
          v-model="internalValue"
          type="checkbox"
          class="checkbox"
          :class="{ 'checkbox-error': hasError }"
          :disabled="disabled"
          @change="$emit('blur')"
        />
        <span class="label-text">{{ field.label }}</span>
      </label>
    </div>

    <!-- Select Field -->
    <select
      v-else-if="fieldType === 'Select'"
      v-model="internalValue"
      class="select select-bordered w-full"
      :class="{ 'select-error': hasError }"
      :disabled="disabled"
      @blur="$emit('blur')"
    >
      <option value="">Select {{ field.label }}</option>
      <option v-for="option in selectOptions" :key="option" :value="option">
        {{ option }}
      </option>
    </select>

    <!-- Textarea for long text fields -->
    <textarea
      v-else
      v-model="internalValue"
      :placeholder="field.help_text || `Enter ${field.label.toLowerCase()}`"
      class="textarea textarea-bordered w-full h-24"
      :class="{ 'textarea-error': hasError }"
      :disabled="disabled"
      @blur="$emit('blur')"
    ></textarea>

    <!-- Error Message -->
    <label v-if="errorMessage" class="label">
      <span class="label-text-alt text-error">{{ errorMessage }}</span>
    </label>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import type { ProfileField } from '@/types'

interface Props {
  field: ProfileField
  modelValue: any
  errorMessage?: string
  disabled?: boolean
}

interface Emits {
  (e: 'update:modelValue', value: any): void
  (e: 'blur'): void
}

const props = withDefaults(defineProps<Props>(), {
  errorMessage: '',
  disabled: false,
})

const emit = defineEmits<Emits>()

const fieldType = computed(() => {
  if (typeof props.field.field_type === 'string') {
    return props.field.field_type
  } else if (typeof props.field.field_type === 'object' && 'Select' in props.field.field_type) {
    return 'Select'
  }
  return 'Text'
})

const selectOptions = computed(() => {
  if (typeof props.field.field_type === 'object' && 'Select' in props.field.field_type) {
    return props.field.field_type.Select.options
  }
  return []
})

const hasError = computed(() => {
  return !!props.errorMessage
})

const numberValue = computed(() => {
  if (fieldType.value === 'Number') {
    return props.modelValue === null || props.modelValue === undefined
      ? ''
      : String(props.modelValue)
  }
  return ''
})

const internalValue = computed({
  get() {
    // Handle boolean fields
    if (fieldType.value === 'Boolean') {
      return !!props.modelValue
    }

    // Skip number fields - they use their own handler
    if (fieldType.value === 'Number') {
      return ''
    }

    return props.modelValue || ''
  },
  set(value) {
    // Handle different field types
    let processedValue = value

    if (fieldType.value === 'Boolean') {
      processedValue = !!value
    } else if (fieldType.value === 'Date' && value === '') {
      processedValue = null
    }

    emit('update:modelValue', processedValue)
  },
})

function updateNumberValue(event: Event) {
  const target = event.target as HTMLInputElement
  const value = target.value
  const processedValue = value === '' ? null : Number(value)
  emit('update:modelValue', processedValue)
}

// ----- TextArray (chip input) -----
const pendingTag = ref('')
const tagInputRef = ref<HTMLInputElement | null>(null)

const tagValues = computed<string[]>(() =>
  Array.isArray(props.modelValue)
    ? (props.modelValue as unknown[]).filter((v): v is string => typeof v === 'string')
    : []
)

function commitTag() {
  const t = pendingTag.value.trim()
  if (!t) return
  if (!tagValues.value.includes(t)) {
    emit('update:modelValue', [...tagValues.value, t])
  }
  pendingTag.value = ''
}

function removeTag(i: number) {
  const next = tagValues.value.slice()
  next.splice(i, 1)
  emit('update:modelValue', next)
}

function onTagKeydown(e: KeyboardEvent) {
  // Comma also adds a tag; Backspace on empty input pops the last one.
  if (e.key === ',') {
    e.preventDefault()
    commitTag()
  } else if (e.key === 'Backspace' && pendingTag.value === '' && tagValues.value.length > 0) {
    e.preventDefault()
    removeTag(tagValues.value.length - 1)
  }
}

function onTagBlur() {
  // Don't lose what's been typed but not yet pressed Enter on.
  commitTag()
  emit('blur')
}

function focusTagInput() {
  tagInputRef.value?.focus()
}
</script>

<style scoped>
.tooltip:before {
  white-space: pre-wrap;
  max-width: 200px;
}
</style>
