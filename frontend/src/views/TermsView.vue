<template>
  <div class="container mx-auto px-4 py-8">
    <div class="hero min-h-48">
      <div class="hero-content text-center">
        <div class="max-w-3xl">
          <h1 class="text-5xl font-bold">Terms and Conditions</h1>
          <p class="py-4 text-sm text-base-content/60">Last updated {{ lastUpdated }}</p>
        </div>
      </div>
    </div>

    <div class="mx-auto max-w-2xl space-y-6 text-base-content/90">
      <div class="alert alert-warning text-sm">
        <span>
          This draft reflects what this site's software actually does as of today. It hasn't
          been formally adopted by Cambridge Hackspace or reviewed by legal counsel &mdash;
          please have someone review it before treating it as final.
        </span>
      </div>

      <section>
        <p class="leading-relaxed">
          These terms cover your use of this website and the membership system behind it &mdash;
          account registration, door and tool access, and everything else the site manages for
          Cambridge Hackspace. By creating an account or using the site, you agree to them. If
          you don't agree, please don't use the site, and reach out to us instead.
        </p>
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">Your account</h2>
        <p class="leading-relaxed">
          You're responsible for the accuracy of the information on your account and for keeping
          your login credentials, MFA methods, and any RFID card linked to your account secure.
          Don't share your login or your card with anyone else &mdash; access granted through
          your account is tied to you specifically, including for safety and liability reasons
          around tool use.
        </p>
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">Physical access and tool use</h2>
        <p class="leading-relaxed">
          Membership gives you access to a physical space and real tools, which carries real
          responsibilities. Follow the space's posted rules, any training requirements attached
          to a given tool, and reasonable instructions from staff. We may restrict, suspend, or
          revoke your access &mdash; to the site, to specific tools, or to the space itself
          &mdash; at our discretion, including without advance notice where we believe it's
          necessary for safety.
        </p>
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">Intellectual property</h2>
        <p class="leading-relaxed">
          Content on this site that's specific to Cambridge Hackspace &mdash; our name, the
          organization's own text and images &mdash; belongs to us. The software running this
          site is separate: it's Free Software, and its own license governs how it can be used
          and modified (see the
          <router-link to="/platform" class="link link-primary">platform page</router-link>
          for details).
        </p>
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">Links to other sites</h2>
        <p class="leading-relaxed">
          This site links to services we don't control, like Meetup, Instagram, and our payment
          processor. We're not responsible for their content or practices &mdash; check their
          own terms before you rely on them.
        </p>
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">Governing law</h2>
        <p class="leading-relaxed">
          These terms are governed by the laws of the Commonwealth of Massachusetts.
        </p>
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">Changes</h2>
        <p class="leading-relaxed">
          We may update these terms from time to time by posting a new version here, with the
          date at the top updated. Continuing to use the site after a change means you accept
          the new terms.
        </p>
      </section>

      <div class="border-t-2 border-dotted border-base-content/30 pt-6 text-center">
        <p class="text-sm text-base-content/60">
          Questions about these terms?
          <a href="mailto:info@cambridgehackspace.com" class="link link-primary">Get in touch</a>.
        </p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
const lastUpdated = new Date().toLocaleDateString('en-US', { year: 'numeric', month: 'long', day: 'numeric' })
</script>
