<template>
  <select
    :value="modelValue ?? ''"
    class="select select-bordered select-sm w-full"
    :disabled="disabled"
    @change="onChange(($event.target as HTMLSelectElement).value)"
  >
    <option :value="''">— Always —</option>
    <option v-for="s in schedules" :key="s.id" :value="s.id">
      {{ s.name }}
    </option>
  </select>
</template>

<script setup lang="ts">
import type { Schedule } from '@/types'

withDefaults(
  defineProps<{
    modelValue: string | null
    schedules: Schedule[]
    disabled?: boolean
  }>(),
  { disabled: false },
)

const emit = defineEmits<{
  'update:modelValue': [value: string | null]
}>()

function onChange(raw: string) {
  emit('update:modelValue', raw === '' ? null : raw)
}
</script>
