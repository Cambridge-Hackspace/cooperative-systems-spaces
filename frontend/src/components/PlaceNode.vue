<template>
  <div class="border-l border-base-300 pl-3 mt-2">
    <div class="flex items-center justify-between gap-2 py-1">
      <div class="flex items-center gap-2">
        <span class="font-medium">{{ place.name }}</span>
        <span class="badge badge-sm" :class="place.is_special ? 'badge-warning' : 'badge-ghost'">{{
          place.place_type
        }}</span>
        <span v-if="place.is_special" class="badge badge-warning badge-outline badge-sm"
          >special</span
        >
      </div>
      <div class="flex items-center gap-1">
        <button v-if="canAddChild" class="btn btn-ghost btn-xs" @click="$emit('add-child', place)">
          + Child
        </button>
        <button class="btn btn-ghost btn-xs" @click="$emit('edit', place)">Edit</button>
        <button class="btn btn-ghost btn-xs text-error" @click="$emit('delete', place)">
          Delete
        </button>
      </div>
    </div>

    <PlaceNode
      v-for="c in children"
      :key="c.id"
      :place="c"
      :children-map="childrenMap"
      :config="config"
      @add-child="(p) => $emit('add-child', p)"
      @edit="(p) => $emit('edit', p)"
      @delete="(p) => $emit('delete', p)"
    />
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import type { Place, PlaceConfig } from '@/types'

const props = defineProps<{
  place: Place
  childrenMap: Map<string | null, Place[]>
  config: PlaceConfig | null
}>()

defineEmits<{
  'add-child': [parent: Place]
  edit: [place: Place]
  delete: [place: Place]
}>()

const children = computed(() => props.childrenMap.get(props.place.id) ?? [])

/** A node only allows adding a child if a deeper type exists in config. */
const canAddChild = computed(() => {
  if (!props.config?.types?.length) return false
  const i = props.config.types.indexOf(props.place.place_type)
  return i >= 0 && i < props.config.types.length - 1
})
</script>
