<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { cmi5Api } from '@/utils/api'
import type { Cmi5LearnerModule } from '@/types'

const router = useRouter()

const modules = ref<Cmi5LearnerModule[]>([])
const loading = ref(false)
const flash = ref('')
const flashOk = ref(true)

function notify(message: string, ok = true) {
  flash.value = message
  flashOk.value = ok
  window.setTimeout(() => {
    if (flash.value === message) flash.value = ''
  }, 5000)
}

async function load() {
  loading.value = true
  try {
    const r = await cmi5Api.listMyModules()
    if (r.success && r.data) modules.value = r.data
    else notify(r.error || 'Failed to load modules', false)
  } finally {
    loading.value = false
  }
}

function open(module: Cmi5LearnerModule) {
  // Render the module in the embedded player, which performs the launch and
  // hosts the content in an iframe. When the learner returns, refreshing this
  // list shows the updated completion state.
  void router.push(`/modules/${module.au_id}/play`)
}

onMounted(load)
</script>

<template>
  <div class="p-4">
    <div class="flex items-center justify-between mb-4">
      <h1 class="text-2xl font-bold">My Training Modules</h1>
      <button class="btn btn-sm" :disabled="loading" @click="load">Refresh</button>
    </div>

    <div v-if="flash" class="alert mb-4" :class="flashOk ? 'alert-success' : 'alert-error'">
      <span>{{ flash }}</span>
    </div>

    <div v-if="loading" class="flex justify-center p-8">
      <span class="loading loading-spinner loading-lg"></span>
    </div>

    <div v-else-if="!modules.length" class="text-center opacity-70 p-8">
      No training modules are available to you yet.
    </div>

    <div v-else class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      <div v-for="module in modules" :key="module.au_id" class="card bg-base-200 shadow">
        <div class="card-body">
          <h2 class="card-title text-base">
            {{ module.au_title ?? module.course_title ?? 'Training module' }}
          </h2>
          <p v-if="module.course_title" class="text-sm opacity-70">
            {{ module.course_title }}
          </p>
          <div class="mt-1">
            <span v-if="module.completed" class="badge badge-success">Completed</span>
            <span v-else class="badge badge-ghost">Not started</span>
          </div>
          <div class="card-actions justify-end mt-2">
            <button class="btn btn-primary btn-sm" @click="open(module)">
              {{ module.completed ? 'Review' : 'Start' }}
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
