<template>
  <div class="min-h-screen flex items-center justify-center bg-base-200">
    <div class="text-center">
      <div class="text-9xl font-bold text-primary mb-4">404</div>
      <h1 class="text-4xl font-bold text-base-content mb-4">Page Not Found</h1>
      <p class="text-lg text-base-content/70 mb-8">
        The page you're looking for doesn't exist or has been moved.
      </p>
      <div class="flex gap-4 justify-center">
        <router-link to="/" class="btn btn-primary">
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6"
            />
          </svg>
          Go Home
        </router-link>
        <button class="btn btn-ghost" @click="$router.go(-1)">
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M10 19l-7-7m0 0l7-7m-7 7h18"
            />
          </svg>
          Go Back
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
// This is a simple 404 page component
</script>
