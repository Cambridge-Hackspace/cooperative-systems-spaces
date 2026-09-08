<template>
  <div class="container mx-auto px-4 py-8">
    <div class="hero min-h-48">
      <div class="hero-content text-center">
        <div class="max-w-3xl">
          <h1 class="text-5xl font-bold">Privacy Policy</h1>
          <p class="py-4 text-sm text-base-content/60">Last updated {{ lastUpdated }}</p>
        </div>
      </div>
    </div>

    <div class="mx-auto max-w-2xl space-y-6 text-base-content/90">
      <div class="alert alert-warning text-sm">
        <span>
          This draft reflects what this site's software actually does as of today. It hasn't been
          formally adopted by {{ siteName }} or reviewed by legal counsel &mdash; please have
          someone review it before treating it as final.
        </span>
      </div>

      <section>
        <p class="leading-relaxed">
          This policy explains what information {{ siteName }} collects through this site and its
          membership system, why we collect it, and who can see it.
        </p>
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">Information we collect</h2>
        <ul class="list-disc pl-6 space-y-2 leading-relaxed">
          <li>
            <strong>Account information:</strong> your name, username, email address, and password
            (stored as a salted hash, never in plain text).
          </li>
          <li>
            <strong>Profile fields:</strong> additional fields our admins configure for the
            community &mdash; this can include things like an RFID card identifier used for physical
            door and tool access.
          </li>
          <li>
            <strong>Multi-factor authentication data:</strong> if you enable MFA, we store a TOTP
            secret or WebAuthn credential and a set of hashed recovery codes.
          </li>
          <li>
            <strong>Access and usage records:</strong> door check-ins and unlocks, tool checkout and
            usage events, and training completion records, each with a timestamp and the member
            involved.
          </li>
          <li>
            <strong>Audit logs:</strong> security-relevant actions (logins, role changes,
            configuration changes, and similar events) are logged with a timestamp and, where
            available, the IP address and browser used.
          </li>
        </ul>
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">How we use it</h2>
        <p class="leading-relaxed">
          We use this information to run the space: authenticating members, controlling door and
          tool access, tracking training and certifications, and keeping an audit trail for security
          and accountability. We don't use it for advertising, and we don't sell it.
        </p>
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">Who can see it</h2>
        <p class="leading-relaxed">
          Access follows your role. Ordinary members see their own data. Staff and admins can see
          roster, access, and training information as needed to run the space and help members. Only
          admins can see the full audit log.
        </p>
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">Third-party sharing</h2>
        <p class="leading-relaxed">
          We do not sell or rent your information. Admins can configure outbound webhooks that send
          event data (like a login or a tool status change) to a URL of their choosing, for
          integrations they set up &mdash; for example, posting activity to a chat tool. If you'd
          like to know what webhooks are currently active, ask an admin.
        </p>
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">Cookies and local storage</h2>
        <p class="leading-relaxed">
          This site doesn't use tracking cookies. When you log in, your browser's local storage
          keeps a login token so you stay signed in; it's cleared when you log out.
        </p>
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">Security</h2>
        <p class="leading-relaxed">
          Passwords and recovery codes are hashed, not stored in plain text, and we apply reasonable
          technical safeguards throughout the system. No system is perfectly secure, and we can't
          guarantee against every possible incident.
        </p>
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">Your choices</h2>
        <p class="leading-relaxed">
          You can review and update your own profile information at any time. To request that your
          account or its data be deleted, contact an admin &mdash; some records, like audit and
          access logs, may be retained longer where needed for security or legal reasons.
        </p>
      </section>

      <section>
        <h2 class="text-xl font-semibold mb-2">Changes to this policy</h2>
        <p class="leading-relaxed">
          If this policy changes, we'll update the date at the top of this page.
        </p>
      </section>

      <div class="border-t-2 border-dotted border-base-content/30 pt-6 text-center">
        <p class="text-sm text-base-content/60">
          Questions about this policy? Reach us at 440R Somerville Avenue, Somerville, MA, or
          <a href="mailto:info@cambridgehackspace.com" class="link link-primary">by email</a>.
        </p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useConfigStore } from '@/stores/config'

const configStore = useConfigStore()
const siteName = computed(() => configStore.siteName())

const lastUpdated = new Date().toLocaleDateString('en-US', {
  year: 'numeric',
  month: 'long',
  day: 'numeric',
})
</script>
