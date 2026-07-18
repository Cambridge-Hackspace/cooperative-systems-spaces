<template>
  <select
    :value="modelValue ?? ''"
    class="select select-bordered select-sm w-full"
    :disabled="disabled || !places.length"
    @change="onChange(($event.target as HTMLSelectElement).value)"
  >
    <!-- Required pickers use a disabled placeholder; optional pickers
         keep the null label as a real selectable value. -->
    <option v-if="hideNull" :value="''" disabled>— Choose a place —</option>
    <option v-else :value="''">{{ nullLabel }}</option>
    <!-- Special places (Outside, Common Area, …) bubble to the top. -->
    <optgroup v-if="specials.length" label="Special">
      <option v-for="p in specials" :key="p.id" :value="p.id">
        ★ {{ p.name }} · {{ p.place_type }}
      </option>
    </optgroup>
    <option v-for="p in flat" :key="p.id" :value="p.id">
      {{ p.indent }}{{ p.name }}
      <span class="text-base-content/50"> · {{ p.place_type }}</span>
    </option>
  </select>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import type { Place } from '@/types'

const props = withDefaults(
  defineProps<{
    modelValue: string | null
    places: Place[]
    /** Label for the empty / null option (e.g. `— Outside —`, `— Unassigned —`). Ignored when `hideNull` is set. */
    nullLabel?: string
    /** When true, no selectable null option is rendered — the user must
        pick a real place. Use for fields where null isn't a legal value. */
    hideNull?: boolean
    disabled?: boolean
  }>(),
  {
    nullLabel: '— Unassigned —',
    hideNull: false,
    disabled: false,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: string | null]
}>()

/** Special places listed separately in their own optgroup. */
const specials = computed(() =>
  props.places.filter(p => p.is_special).sort((a, b) => a.name.localeCompare(b.name)),
)

/** Walks the regular hierarchy (specials excluded) into a depth-indented list. */
const flat = computed(() => {
  const regular = props.places.filter(p => !p.is_special)
  const byParent = new Map<string | null, Place[]>()
  for (const p of regular) {
    const k = p.parent_id
    const arr = byParent.get(k) ?? []
    arr.push(p)
    byParent.set(k, arr)
  }
  for (const list of byParent.values()) list.sort((a, b) => a.name.localeCompare(b.name))

  const out: Array<Place & { indent: string }> = []
  function visit(parent: string | null, depth: number) {
    for (const p of byParent.get(parent) ?? []) {
      out.push({ ...p, indent: '— '.repeat(depth) })
      visit(p.id, depth + 1)
    }
  }
  visit(null, 0)
  // A special place that has children (e.g. "Outside → Parking Lot A") will
  // have those children show up here rooted at the special's id; render them
  // at depth 0 so the picker isn't confusing.
  return out
})

function onChange(raw: string) {
  emit('update:modelValue', raw === '' ? null : raw)
}
</script>
