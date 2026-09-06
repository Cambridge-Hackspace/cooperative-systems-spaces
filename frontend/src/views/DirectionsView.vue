<template>
  <div class="container mx-auto px-4 py-8">
    <div class="hero min-h-48">
      <div class="hero-content text-center">
        <div class="max-w-3xl">
          <h1 class="text-5xl font-bold">Directions</h1>
          <p class="py-4 text-base-content/70">
            Our door isn't visible from the street, so here's a step-by-step walkthrough with
            photos.
          </p>
        </div>
      </div>
    </div>

    <div class="mx-auto max-w-2xl space-y-8 text-base-content/90">
      <section>
        <h2 class="text-xl font-semibold mb-2">Map</h2>
        <img src="/images/map.png" :alt="`Map to ${siteName}`" class="rounded-lg max-w-full" />
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">Getting here</h2>
        <p class="leading-relaxed">
          We're at 440R Somerville Avenue, Somerville, MA &mdash; a rear building, tucked behind 440
          Somerville Ave. Our door faces the alley on the south side of the building, near the
          south-west corner.
        </p>
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">1. Start at Market Basket</h2>
        <p class="leading-relaxed">
          Find the Market Basket at 400 Somerville Ave. The historic Milk Row Cemetery sits just to
          its right, and our building is directly behind the cemetery.
        </p>
        <img
          src="/images/marketbasket.jpg"
          alt="View of Market Basket from the street"
          class="rounded-lg max-w-full mt-3"
        />
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">2. Turn into the alley</h2>
        <p class="leading-relaxed">
          Walk forward with the cemetery on your right, then turn right at the corner of the
          building behind it. You'll see an unnamed alley &mdash; that's the way in.
        </p>
        <img
          src="/images/440r_from_marketbasket.jpg"
          alt="View of the alley entrance from near Market Basket"
          class="rounded-lg max-w-full mt-3"
        />
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">3. Follow the wall to our door</h2>
        <p class="leading-relaxed">
          Follow the wall on your right down the alley, past a door in the middle that just leads
          upstairs. Near the end of the building you'll find a white door with three granite steps
          &mdash; "438R/440R" is painted on it in blue.
        </p>
        <img
          src="/images/440r_door.jpg"
          alt="The white door marked 438R/440R"
          class="rounded-lg max-w-full mt-3"
        />
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">4. Ring the doorbell</h2>
        <p class="leading-relaxed">
          The doorbell is on the left side of the door frame, about a foot above the knob, and glows
          orange so it's easy to spot at night. Ring it and wait for someone to let you in &mdash;
          if a neighbor has left the door open, ring anyway and step inside. Once you're in the
          building, head right down the hallway; we're at the end, with a slight right at the very
          end of the hall.
        </p>
        <img
          src="/images/cambridge_hackspace_doorbell.jpg"
          :alt="`The ${siteName} doorbell`"
          class="rounded-lg max-w-full mt-3"
        />
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">Before you come</h2>
        <p class="leading-relaxed">
          We're not generally open to the public outside of events &mdash; come to our next
          <a
            href="https://www.meetup.com/Cambridge-Hackspace/events/"
            target="_blank"
            rel="noopener"
            class="link link-primary"
            >Tuesday Project Night</a
          >
          to see the space for yourself.
        </p>
      </section>

      <div class="border-t-2 border-dotted border-base-content/30 pt-6 text-center">
        <p class="text-sm text-base-content/60">
          <router-link to="/about" class="link link-primary">Back to About</router-link>
        </p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
// Directions page component
import { computed } from 'vue'
import { useConfigStore } from '@/stores/config'

const configStore = useConfigStore()
const siteName = computed(() => configStore.siteName())
</script>
