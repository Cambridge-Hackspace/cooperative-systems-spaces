<template>
  <div class="container mx-auto px-4 py-8">
    <div class="hero min-h-48">
      <div class="hero-content text-center">
        <div class="max-w-3xl">
          <h1 class="text-5xl font-bold">Events</h1>
          <p class="py-4 text-base-content/70">
            Join us for
            <a href="https://www.meetup.com/Cambridge-Hackspace/events/" target="_blank" rel="noopener" class="link link-primary">
              Hackspace Project Night
            </a>
            every Tuesday at 6:30pm, or check what else is coming up below.
          </p>
        </div>
      </div>
    </div>

    <div class="max-w-2xl mx-auto">
      <CalendarEvents />
    </div>
  </div>
</template>

<script setup lang="ts">
import CalendarEvents from '@/components/CalendarEvents.vue'
</script>
