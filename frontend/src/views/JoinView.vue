<template>
  <div class="container mx-auto px-4 py-8">
    <div class="hero min-h-48">
      <div class="hero-content text-center">
        <div class="max-w-3xl">
          <h1 class="text-5xl font-bold">Join Us!</h1>
        </div>
      </div>
    </div>

    <div class="mx-auto max-w-2xl space-y-6 text-base-content/90">
      <section>
        <h2 class="text-2xl font-semibold mb-2">About</h2>
        <p class="leading-relaxed">
          {{ siteName }} grew out of a small group meeting weekly to work on projects and meet
          like-minded people. Years later the idea hasn't changed &mdash; we've just grown into it:
          our own space, a solid set of tools, and a community built up over time.
        </p>
      </section>

      <section>
        <h2 class="text-2xl font-semibold mb-2">Membership</h2>
        <ul class="list-disc pl-6 space-y-1 leading-relaxed">
          <li>$75 per month</li>
          <li>24/7 access to the space</li>
          <li>
            Full use of all our tools, including:
            <ul class="list-disc pl-6 mt-1 space-y-0.5">
              <li>Laser cutters</li>
              <li>Desktop CNC mills</li>
              <li>3D printers</li>
              <li>Drill press, band saw, scroll saw, belt sander</li>
              <li>Soldering tools and oscilloscopes</li>
              <li>Sewing machines</li>
              <li>Woodwork and hand tools</li>
            </ul>
          </li>
          <li>Support from like-minded people</li>
          <li>Storage space for your projects</li>
          <li>Be part of an amazing community</li>
        </ul>

        <div class="text-center mt-6">
          <router-link to="/register" class="btn btn-primary btn-lg btn-block">
            Become a Member
          </router-link>
        </div>

        <p class="text-xs text-base-content/60 mt-4 leading-relaxed">
          Laser cutter time is billed separately &mdash; contact us for current rates and details
          about pre-paying for a discount.
        </p>
      </section>

      <div class="border-t-2 border-dotted border-base-content/30 pt-6 text-center">
        <p class="text-sm text-base-content/60">
          Not ready to commit? Visiting us on Tuesday nights is free.
          <router-link to="/about" class="link link-primary">Learn more about us</router-link>.
        </p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
// Join page component
import { computed } from 'vue'
import { useConfigStore } from '@/stores/config'

const configStore = useConfigStore()
const siteName = computed(() => configStore.siteName())
</script>
