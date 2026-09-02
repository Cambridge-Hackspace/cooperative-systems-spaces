<template>
  <div>
    <div class="flex items-start justify-between mb-3 gap-4">
      <p class="text-base-content/70 text-sm">
        Places shown as nodes, doors as edges. Hierarchy lines are dashed; disabled doors are
        dotted; operator-defined
        <span class="badge badge-warning badge-sm">special</span> places (Outside, Common Area, …)
        are amber; doors with an unassigned side land on a small
        <span class="badge badge-ghost badge-sm">(unset)</span> placeholder.
      </p>
      <div class="flex items-center gap-2 whitespace-nowrap">
        <select v-model="layoutName" class="select select-bordered select-sm">
          <option value="cose">Layout: force</option>
          <option value="breadthfirst">Layout: tree</option>
          <option value="concentric">Layout: concentric</option>
          <option value="grid">Layout: grid</option>
        </select>
        <button class="btn btn-ghost btn-sm" @click="load">Reload</button>
      </div>
    </div>

    <div
      v-if="loading"
      class="h-[600px] flex items-center justify-center bg-base-100 border border-base-300 rounded"
    >
      <span class="loading loading-spinner loading-lg"></span>
    </div>
    <div v-else-if="error" class="alert alert-error">
      <span>{{ error }}</span>
      <button class="btn btn-sm" @click="load">Retry</button>
    </div>
    <div
      v-else-if="!places.length && !doors.length"
      class="h-[600px] flex items-center justify-center bg-base-100 border border-base-300 rounded text-base-content/60"
    >
      Nothing to graph yet — add some places or doors first.
    </div>
    <div
      v-else
      ref="cyContainer"
      class="h-[600px] bg-base-100 border border-base-300 rounded"
    ></div>

    <!-- Selection details strip -->
    <div v-if="selected" class="mt-3 card bg-base-200">
      <div class="card-body py-3">
        <div class="text-sm flex items-center gap-2">
          <span class="badge" :class="selected.kindBadge">{{ selected.kindLabel }}</span>
          <strong>{{ selected.label }}</strong>
          <span v-if="selected.subtitle" class="text-base-content/60"
            >— {{ selected.subtitle }}</span
          >
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, ref, watch, nextTick } from 'vue'
import { useReloadOnReactivate } from '@/composables/useReloadOnReactivate'
import cytoscape, { type Core, type ElementDefinition } from 'cytoscape'
import { placesApi, doorsApi } from '@/utils/api'
import type { Door, Place } from '@/types'

type LayoutName = 'cose' | 'breadthfirst' | 'concentric' | 'grid'

const cyContainer = ref<HTMLDivElement | null>(null)
const loading = ref(false)
const error = ref('')
const places = ref<Place[]>([])
const doors = ref<Door[]>([])
const layoutName = ref<LayoutName>('cose')
const selected = ref<{
  label: string
  subtitle: string
  kindLabel: string
  kindBadge: string
} | null>(null)
let cy: Core | null = null

async function load() {
  loading.value = true
  error.value = ''
  try {
    const [p, d] = await Promise.all([placesApi.list(), doorsApi.list()])
    if (p.success && p.data) places.value = p.data
    else error.value = p.error || 'Could not load the places'
    if (d.success && d.data) doors.value = d.data
    else error.value = d.error || 'Could not load the doors'
  } catch (e) {
    // This component had no error surface at all, so a rejected load left the
    // spinner up forever and the rejection went to an
    // `app.config.errorHandler` that `src/main.ts` never sets -- reaching the
    // browser console and nowhere else.
    error.value = e instanceof Error ? e.message : 'Could not load the graph'
  } finally {
    loading.value = false
  }
  await nextTick()
  rebuild()
}

function buildElements(): ElementDefinition[] {
  const els: ElementDefinition[] = []

  // One node per place. Special places get a distinct visual class.
  for (const p of places.value) {
    els.push({
      data: { id: p.id, label: p.name, kind: 'place', placeType: p.place_type },
      classes: p.is_special ? 'place special' : 'place',
    })
  }

  // Hierarchy parent → child edges. Special places live at root, so their
  // children (if any) still produce ordinary hierarchy edges.
  for (const p of places.value) {
    if (p.parent_id) {
      els.push({
        data: { source: p.parent_id, target: p.id, kind: 'hierarchy' },
        classes: 'hierarchy',
      })
    }
  }

  // Doors as edges. If one side is null, draw a small "(unset)" tag node so
  // the door still appears with a meaningful loose end.
  let unsetCounter = 0
  for (const d of doors.value) {
    if (!d.place_id_from && !d.place_id_to) continue
    let a = d.place_id_from
    let b = d.place_id_to
    if (!a) {
      a = `__unset_${unsetCounter++}`
      els.push({ data: { id: a, label: '(unset)', kind: 'unset' }, classes: 'unset' })
    }
    if (!b) {
      b = `__unset_${unsetCounter++}`
      els.push({ data: { id: b, label: '(unset)', kind: 'unset' }, classes: 'unset' })
    }
    els.push({
      data: {
        id: `door-${d.id}`,
        source: a,
        target: b,
        label: d.name,
        kind: 'door',
        enabled: d.enabled,
      },
      classes: 'door ' + (d.enabled ? 'door-enabled' : 'door-disabled'),
    })
  }
  return els
}

function rebuild() {
  if (!cyContainer.value) return
  if (cy) {
    cy.destroy()
    cy = null
  }

  cy = cytoscape({
    container: cyContainer.value,
    elements: buildElements(),
    // Cytoscape's strict TS types disagree with itself on string-vs-number
    // for several style props; the runtime accepts both. Cast the whole
    // stylesheet to keep the source readable.
    style: [
      {
        selector: 'node',
        style: {
          'background-color': '#3b82f6',
          label: 'data(label)',
          color: '#ffffff',
          'text-valign': 'center',
          'text-halign': 'center',
          'font-size': 11,
          padding: '8px',
          width: 'label' as any,
          height: 'label' as any,
          shape: 'round-rectangle',
          'text-wrap': 'wrap',
          'text-max-width': '140px',
        },
      },
      {
        // Operator-defined special places (Outside, Common Area, Parking, …)
        selector: 'node.special',
        style: { 'background-color': '#f59e0b', color: '#000' },
      },
      {
        // Loose-end placeholder for doors with one side unset.
        selector: 'node.unset',
        style: {
          'background-color': '#e5e7eb',
          color: '#374151',
          shape: 'ellipse',
          'font-size': 10,
        },
      },
      {
        selector: 'node:selected',
        style: { 'border-width': 3, 'border-color': '#fbbf24' },
      },
      {
        selector: 'edge.hierarchy',
        style: {
          'line-style': 'dashed',
          'line-color': '#9ca3af',
          'target-arrow-color': '#9ca3af',
          'target-arrow-shape': 'triangle',
          'curve-style': 'bezier',
          width: 1,
          opacity: 0.7,
        },
      },
      {
        selector: 'edge.door',
        style: {
          label: 'data(label)',
          'font-size': 10,
          color: '#1f2937',
          'text-background-color': '#ffffff',
          'text-background-opacity': 0.8,
          'text-background-padding': 2,
          'text-rotation': 'autorotate' as any,
          'curve-style': 'bezier',
          width: 2,
        },
      },
      { selector: 'edge.door-enabled', style: { 'line-color': '#22c55e' } },
      {
        selector: 'edge.door-disabled',
        style: { 'line-color': '#9ca3af', 'line-style': 'dotted' },
      },
      {
        selector: 'edge:selected',
        style: { width: 4, 'line-color': '#fbbf24' },
      },
    ] as any,
    layout: layoutOptions(layoutName.value),
    wheelSensitivity: 0.2,
  })

  cy.on('tap', 'node', (evt) => {
    const n = evt.target
    const kind = n.data('kind')
    if (kind === 'unset') {
      selected.value = {
        label: '(unset)',
        subtitle: 'door side not assigned to any place',
        kindLabel: 'placeholder',
        kindBadge: 'badge-ghost',
      }
      return
    }
    const isSpecial = n.hasClass('special')
    selected.value = {
      label: n.data('label'),
      subtitle: n.data('placeType') || '',
      kindLabel: isSpecial ? 'special' : 'place',
      kindBadge: isSpecial ? 'badge-warning' : 'badge-info',
    }
  })

  cy.on('tap', 'edge', (evt) => {
    const e = evt.target
    if (e.data('kind') === 'door') {
      const a = cy.getElementById(e.data('source')).data('label')
      const b = cy.getElementById(e.data('target')).data('label')
      selected.value = {
        label: e.data('label'),
        subtitle: `${a} ↔ ${b} · ${e.data('enabled') ? 'enabled' : 'disabled'}`,
        kindLabel: 'door',
        kindBadge: e.data('enabled') ? 'badge-success' : 'badge-neutral',
      }
    } else {
      selected.value = {
        label: 'hierarchy',
        subtitle: `${cy.getElementById(e.data('source')).data('label')} → ${cy.getElementById(e.data('target')).data('label')}`,
        kindLabel: 'parent',
        kindBadge: 'badge-ghost',
      }
    }
  })

  cy.on('tap', (evt) => {
    if (evt.target === cy) selected.value = null
  })
}

function layoutOptions(name: LayoutName) {
  switch (name) {
    case 'breadthfirst':
      return {
        name: 'breadthfirst',
        directed: true,
        padding: 30,
        spacingFactor: 1.2,
        animate: true,
      } as any
    case 'concentric':
      return {
        name: 'concentric',
        padding: 30,
        animate: true,
        // Special places ring the outside; loose-end placeholders sit on the
        // outermost ring; ordinary places fill the inside.
        concentric: (n: any) => (n.hasClass('special') ? 100 : n.hasClass('unset') ? 200 : 1),
        levelWidth: () => 1,
      } as any
    case 'grid':
      return { name: 'grid', padding: 30, animate: true } as any
    case 'cose':
    default:
      return {
        name: 'cose',
        animate: true,
        padding: 30,
        idealEdgeLength: 80,
        nodeRepulsion: 12000,
      } as any
  }
}

watch(layoutName, () => {
  if (!cy) return
  cy.layout(layoutOptions(layoutName.value)).run()
})

// The graph draws places *and* doors, so it is stale with respect to both
// sibling tabs. It is also the tab somebody opens last, to check their work.
useReloadOnReactivate(load)
onBeforeUnmount(() => {
  if (cy) {
    cy.destroy()
    cy = null
  }
})
</script>
