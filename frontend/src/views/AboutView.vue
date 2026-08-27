<template>
  <div class="container mx-auto px-4 py-8">
    <div class="hero min-h-64">
      <div class="hero-content text-center">
        <div class="max-w-3xl">
          <h1 class="text-5xl font-bold">About</h1>
        </div>
      </div>
    </div>

    <!-- No `prose` here: this project doesn't have @tailwindcss/typography,
         so prose is a no-op and Preflight would collapse h2/h3 to body size.
         Headings are sized explicitly to match the rest of the app. -->
    <div class="mx-auto max-w-3xl space-y-6 text-base-content/90">
      <section>
        <h2 class="text-2xl font-semibold mb-2">What is Cambridge Hackspace?</h2>
        <p class="leading-relaxed">
          Cambridge Hackspace is a 501(c)(3) non-profit makerspace in the center of Somerville, MA.
          We hold weekly meetings where we get together and make things. We have a dedicated space
          where members can come along and work on their projects any day of the week. Our
          community has a common interest in making, building, computers, technology, craft, and
          art. Members join not only to work on their projects, but to share knowledge and
          resources, to learn, and to be part of the growing community.
        </p>
      </section>

      <section>
        <h2 class="text-2xl font-semibold mb-2">What's a hackspace?</h2>
        <p class="leading-relaxed">
          A hackspace is a place where members get together to work on their projects. The
          hackspace provides not only the physical space, but the tools and a place for the magic
          to happen. There are hundreds of hackspaces around the world.
        </p>
      </section>

      <section>
        <h2 class="text-2xl font-semibold mb-2">But being a hacker sounds like a bad thing.</h2>
        <p class="leading-relaxed">
          People often ask us this. What we do is very different from the malicious and illegal
          activity associated with hackers who steal credit card numbers, etc. The word "hack" to
          us means making the most of the resources, the tools, and the space you have, and
          making something awesome with it. Our members are interested in DIY, sharing, and
          learning; they are makers.
        </p>
      </section>

      <section>
        <h2 class="text-2xl font-semibold mb-2">How much does it cost?</h2>
        <p class="leading-relaxed">
          Visiting us on Tuesday nights is free — come along and say hello!
        </p>
        <p class="leading-relaxed">
          Our keyholder membership gives you 24/7 access and use of all our tools. You can
          <router-link to="/join" class="link link-primary">become a member</router-link>
          right away.
        </p>
        <p class="leading-relaxed">
          All membership fees and donations go towards rent, the purchase and maintenance of
          tools, insurance, and maintaining our space.
        </p>
      </section>

      <section>
        <h2 class="text-2xl font-semibold mb-2">Are there requirements?</h2>
        <p class="leading-relaxed">
          You must be over 18 to be a member of Cambridge Hackspace.
        </p>
      </section>

      <section>
        <h2 class="text-2xl font-semibold mb-2">Accessibility</h2>
        <p class="leading-relaxed">
          Cambridge Hackspace welcomes individuals with disabilities to participate in our
          activities. Although we are on the ground floor, the main building entrance has three
          (3) steps from street level. Our building does not have a wheelchair ramp or lift, but
          we're happy to arrange accommodations and access for you to join us on our Tuesday
          Project Night or other events. Please contact us directly at
          <a href="mailto:info@cambridgehackspace.com" class="link link-primary">info@cambridgehackspace.com</a>.
        </p>
      </section>

      <section>
        <h2 class="text-2xl font-semibold mb-2">Where can you find us?</h2>
        <p class="leading-relaxed">
          We are at 440R Somerville Avenue, Somerville — in the same building as Little India
          Market (next to Market Basket), at the opposite end of the building.
          <router-link to="/directions" class="link link-primary">Get directions</router-link>.
        </p>
        <p class="leading-relaxed">
          We are not generally open to the public outside of our events — come to our next
          <a href="https://www.meetup.com/Cambridge-Hackspace/events/" target="_blank" rel="noopener" class="link link-primary">Tuesday Project Night</a>
          to see for yourself.
        </p>
        <img src="/images/map.png" alt="Map to Cambridge Hackspace" class="rounded-lg mt-4 max-w-full" />
      </section>

      <div class="border-t-2 border-dotted border-base-content/30 pt-6">
        <p class="text-sm text-base-content/60">
          Interested in the open-source software running this site?
          <router-link to="/platform" class="link link-primary">Read about the platform</router-link>.
        </p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
// About page component
</script>
