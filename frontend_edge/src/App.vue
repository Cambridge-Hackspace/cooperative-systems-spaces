<template>
  <div class="min-h-screen flex items-center justify-center p-5 bg-gradient-to-br from-primary via-secondary to-primary">
    <div class="card bg-base-100 shadow-2xl w-full max-w-2xl">
      <div class="card-body">
        <router-view />
        <div role="tablist" class="tabs tabs-boxed mt-4">
          <router-link
            to="/"
            role="tab"
            class="tab"
            :class="{ 'tab-active': $route.name === 'status' }"
          >Device</router-link>
          <router-link
            to="/toolguard"
            role="tab"
            class="tab"
            :class="{ 'tab-active': $route.name === 'toolguard' }"
          >Tools</router-link>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
</script>
