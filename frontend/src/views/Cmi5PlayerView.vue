<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { cmi5Api } from '@/utils/api'

const route = useRoute()
const router = useRouter()

const launchUrl = ref<string | null>(null)
const error = ref('')
const loading = ref(true)

onMounted(async () => {
  const auId = route.params.auId as string
  const r = await cmi5Api.launch(auId)
  if (r.success && r.data) {
    // The content is served from our own origin, so it renders in an iframe
    // without frame/CORS restrictions and talks to the LRS same-origin using the
    // launch parameters in the URL.
    launchUrl.value = r.data.launch_url
  } else {
    error.value = r.error || 'Could not launch this module.'
  }
  loading.value = false
})
</script>

<template>
  <div class="flex flex-col" style="height: calc(100vh - 8rem)">
    <div class="flex items-center gap-2 p-2">
      <button class="btn btn-sm" @click="router.push('/modules')">← Back to modules</button>
      <a
        v-if="launchUrl"
        :href="launchUrl"
        target="_blank"
        rel="noopener"
        class="btn btn-sm btn-ghost"
        >Open in a new tab</a
      >
    </div>

    <div v-if="loading" class="flex justify-center p-8">
      <span class="loading loading-spinner loading-lg"></span>
    </div>
    <div v-else-if="error" class="alert alert-error m-2">
      <span>{{ error }}</span>
    </div>
    <iframe
      v-else-if="launchUrl"
      :src="launchUrl"
      class="flex-1 w-full"
      style="border: 0"
      title="cmi5 content"
    ></iframe>
  </div>
</template>
