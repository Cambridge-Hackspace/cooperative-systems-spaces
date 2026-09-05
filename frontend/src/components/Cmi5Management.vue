<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { cmi5Api, trainingApi } from '@/utils/api'
import type { Cmi5AssignableUnit, Cmi5Course, TrainingStep } from '@/types'

const courses = ref<Cmi5Course[]>([])
const steps = ref<TrainingStep[]>([])
const loading = ref(false)
const importing = ref(false)
const flash = ref('')
const flashOk = ref(true)

// The currently expanded course and its assignable units.
const openCourseId = ref<string | null>(null)
const aus = ref<Cmi5AssignableUnit[]>([])
const ausLoading = ref(false)
// Pending training-step selection per AU id (before Save).
const pendingStep = ref<Record<string, string>>({})

const fileInput = ref<HTMLInputElement | null>(null)

function notify(message: string, ok = true) {
  flash.value = message
  flashOk.value = ok
  window.setTimeout(() => {
    if (flash.value === message) flash.value = ''
  }, 5000)
}

async function loadCourses() {
  loading.value = true
  try {
    const r = await cmi5Api.listCourses()
    if (r.success && r.data) courses.value = r.data
    else notify(r.error || 'Failed to load courses', false)
  } finally {
    loading.value = false
  }
}

async function loadSteps() {
  const r = await trainingApi.getTrainingSteps()
  if (r.success && r.data) steps.value = r.data
}

async function openCourse(course: Cmi5Course) {
  if (openCourseId.value === course.id) {
    openCourseId.value = null
    aus.value = []
    return
  }
  openCourseId.value = course.id
  ausLoading.value = true
  try {
    const r = await cmi5Api.getCourse(course.id)
    if (r.success && r.data) {
      aus.value = r.data.aus
      pendingStep.value = {}
      for (const au of r.data.aus) {
        pendingStep.value[au.id] = au.training_step_id ?? ''
      }
    } else {
      notify(r.error || 'Failed to load course', false)
    }
  } finally {
    ausLoading.value = false
  }
}

async function saveAssignment(au: Cmi5AssignableUnit) {
  if (!openCourseId.value) return
  const chosen = pendingStep.value[au.id] || null
  const r = await cmi5Api.assignAu(openCourseId.value, au.id, { training_step_id: chosen })
  if (r.success && r.data) {
    au.training_step_id = r.data.training_step_id
    notify(chosen ? 'Assignment saved.' : 'Assignment cleared.')
  } else {
    notify(r.error || 'Failed to save assignment', false)
  }
}

function triggerImport() {
  fileInput.value?.click()
}

async function onFileChosen(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  importing.value = true
  try {
    const r = await cmi5Api.importCourse(file)
    if (r.success && r.data) {
      notify(`Imported "${r.data.course.title ?? r.data.course.course_iri}" (${r.data.aus.length} AU).`)
      await loadCourses()
    } else {
      notify(r.error || 'Import failed', false)
    }
  } finally {
    importing.value = false
    if (fileInput.value) fileInput.value.value = ''
  }
}

async function exportCourse(course: Cmi5Course) {
  try {
    const blob = await cmi5Api.exportCourse(course.id)
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `cmi5-${course.id}.zip`
    document.body.appendChild(a)
    a.click()
    a.remove()
    URL.revokeObjectURL(url)
  } catch {
    notify('Export failed', false)
  }
}

async function deleteCourse(course: Cmi5Course) {
  if (!window.confirm(`Delete "${course.title ?? course.course_iri}"? This cannot be undone.`)) {
    return
  }
  const r = await cmi5Api.deleteCourse(course.id)
  if (r.success) {
    if (openCourseId.value === course.id) {
      openCourseId.value = null
      aus.value = []
    }
    notify('Course deleted.')
    await loadCourses()
  } else {
    notify(r.error || 'Failed to delete course', false)
  }
}

function stepLabel(step: TrainingStep): string {
  return `${step.step_name} (step ${step.step_number})`
}

onMounted(async () => {
  await Promise.all([loadCourses(), loadSteps()])
})
</script>

<template>
  <div class="p-4">
    <div class="text-sm breadcrumbs mb-2">
      <ul>
        <li><router-link to="/">Home</router-link></li>
        <li><router-link to="/admin">Admin</router-link></li>
        <li>Training Modules</li>
      </ul>
    </div>

    <div class="flex items-center justify-between mb-4">
      <div>
        <h1 class="text-2xl font-bold">cmi5 Training Modules</h1>
        <p class="text-sm opacity-70">
          Import cmi5 packages, bind an assignable unit to a training step, and export.
        </p>
      </div>
      <div>
        <input
          ref="fileInput"
          type="file"
          accept=".zip,application/zip"
          class="hidden"
          @change="onFileChosen"
        />
        <button class="btn btn-primary" :disabled="importing" @click="triggerImport">
          <span v-if="importing" class="loading loading-spinner loading-sm"></span>
          Import package
        </button>
      </div>
    </div>

    <div v-if="flash" class="alert mb-4" :class="flashOk ? 'alert-success' : 'alert-error'">
      <span>{{ flash }}</span>
    </div>

    <div v-if="loading" class="flex justify-center p-8">
      <span class="loading loading-spinner loading-lg"></span>
    </div>

    <div v-else-if="!courses.length" class="text-center opacity-70 p-8">
      No cmi5 courses imported yet.
    </div>

    <div v-else class="overflow-x-auto">
      <table class="table">
        <thead>
          <tr>
            <th>Title</th>
            <th>Course IRI</th>
            <th>Imported</th>
            <th class="text-right">Actions</th>
          </tr>
        </thead>
        <tbody>
          <template v-for="course in courses" :key="course.id">
            <tr>
              <td class="font-medium">{{ course.title ?? '(untitled)' }}</td>
              <td class="font-mono text-xs opacity-70">{{ course.course_iri }}</td>
              <td class="text-xs">{{ new Date(course.created_at).toLocaleString() }}</td>
              <td class="text-right space-x-1">
                <button class="btn btn-xs" @click="openCourse(course)">
                  {{ openCourseId === course.id ? 'Hide units' : 'Units' }}
                </button>
                <button class="btn btn-xs" @click="exportCourse(course)">Export</button>
                <button class="btn btn-xs btn-error" @click="deleteCourse(course)">Delete</button>
              </td>
            </tr>
            <tr v-if="openCourseId === course.id">
              <td colspan="4" class="bg-base-200">
                <div v-if="ausLoading" class="p-4">
                  <span class="loading loading-spinner loading-sm"></span>
                </div>
                <div v-else class="p-2 space-y-2">
                  <div
                    v-for="au in aus"
                    :key="au.id"
                    class="flex flex-wrap items-center gap-2"
                  >
                    <span class="font-medium">{{ au.title ?? au.au_iri }}</span>
                    <span class="badge badge-ghost">moveOn: {{ au.move_on }}</span>
                    <span v-if="au.mastery_score != null" class="badge badge-ghost">
                      mastery: {{ au.mastery_score }}
                    </span>
                    <select
                      v-model="pendingStep[au.id]"
                      class="select select-bordered select-sm"
                    >
                      <option value="">— not gating a tool —</option>
                      <option v-for="step in steps" :key="step.id" :value="step.id">
                        {{ stepLabel(step) }}
                      </option>
                    </select>
                    <button class="btn btn-sm" @click="saveAssignment(au)">Save</button>
                  </div>
                </div>
              </td>
            </tr>
          </template>
        </tbody>
      </table>
    </div>
  </div>
</template>
