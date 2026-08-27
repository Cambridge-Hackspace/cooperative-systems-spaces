<template>
  <div class="container mx-auto px-4 py-8">
    <div class="hero min-h-48">
      <div class="hero-content text-center">
        <div class="max-w-3xl">
          <h1 class="text-5xl font-bold">Contact</h1>
        </div>
      </div>
    </div>

    <div class="mx-auto max-w-2xl space-y-6 text-base-content/90">
      <section>
        <p class="leading-relaxed">
          Feel free to drop us a line at
          <a href="mailto:info@cambridgehackspace.com" class="link link-primary">info@cambridgehackspace.com</a>.
        </p>
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">You can find us at</h2>
        <p class="leading-relaxed">
          440R Somerville Avenue<br />
          Somerville<br />
          MA, 02143
        </p>
        <p class="leading-relaxed mt-2">
          <router-link to="/directions" class="link link-primary">Click here for detailed directions</router-link>.
        </p>
        <p class="leading-relaxed mt-4">
          We're not generally open to the public outside of our events &mdash; come to our next
          <a href="https://www.meetup.com/Cambridge-Hackspace/events/" target="_blank" rel="noopener" class="link link-primary">Tuesday Project Night</a>.
        </p>
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">Accessibility</h2>
        <p class="leading-relaxed">
          Cambridge Hackspace welcomes individuals with disabilities to participate in our
          activities. Although we're on the ground floor, the main building entrance has three
          steps up from street level, and we don't have a ramp or lift. We're happy to arrange
          accommodations so you can join us on a Tuesday Project Night or other event &mdash;
          just reach out to the email above.
        </p>
      </section>

      <img src="/images/map.png" alt="Map to Cambridge Hackspace" class="rounded-lg max-w-full" />
    </div>
  </div>
</template>

<script setup lang="ts">
// Contact page component
</script>
