<template>
  <div class="container mx-auto px-4 py-8">
    <div class="hero min-h-96">
      <div class="hero-content text-center">
        <div class="max-w-3xl">
          <h1 class="text-5xl font-bold">
            About <strong>Cooperative Systems:</strong> <i>Spaces</i>
          </h1>
          <p class="py-6 text-base-content/80">
            Welcome to your collaborative workspace management system. Manage your makerspace,
            hackerspace, or community workspace with ease.
          </p>
        </div>
      </div>
    </div>

    <!-- Features Section -->
    <div class="grid gap-8 md:grid-cols-3 mt-16">
      <div class="card bg-base-200 shadow-xl">
        <div class="card-body items-center text-center">
          <svg class="w-12 h-12 text-primary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z"
            />
          </svg>
          <h2 class="card-title">User Profiles</h2>
          <p>Customizable user profiles with configurable fields for your community.</p>
        </div>
      </div>

      <div class="card bg-base-200 shadow-xl">
        <div class="card-body items-center text-center">
          <svg class="w-12 h-12 text-primary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M19.428 15.428a2 2 0 00-1.022-.547l-2.387-.477a6 6 0 00-3.86.517l-.318.158a6 6 0 01-3.86.517L6.05 15.21a2 2 0 00-1.806.547M8 4h8l-1 1v5.172a2 2 0 00.586 1.414l5 5c1.26 1.26.367 3.414-1.415 3.414H4.828c-1.782 0-2.674-2.154-1.414-3.414l5-5A2 2 0 009 10.172V5L8 4z"
            />
          </svg>
          <h2 class="card-title">Tool Management</h2>
          <p>
            Track tools, equipment, and resources with checkout systems and training requirements.
          </p>
        </div>
      </div>

      <div class="card bg-base-200 shadow-xl">
        <div class="card-body items-center text-center">
          <svg class="w-12 h-12 text-primary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v4a2 2 0 01-2 2h-2a2 2 0 01-2-2z"
            />
          </svg>
          <h2 class="card-title">Analytics</h2>
          <p>Track usage, member activity, and generate reports for your space.</p>
        </div>
      </div>
    </div>

    <!-- Additional Information Section -->
    <!-- No `prose` here: this project doesn't have @tailwindcss/typography,
         so prose is a no-op and Preflight would collapse h2/h3 to body size.
         Headings are sized explicitly to match the rest of the app. -->
    <div class="mt-16 mx-auto max-w-3xl space-y-6 text-base-content/90">
      <section>
        <h2 class="text-3xl font-bold mb-3">
          What is <strong>Cooperative Systems:</strong> <i>Spaces</i>?
        </h2>
        <p class="leading-relaxed">
          <strong>Cooperative Systems:</strong> <i>Spaces</i> is a comprehensive management platform
          designed specifically for makerspaces, hackerspaces, and community workshops. It provides
          the tools you need to efficiently manage your collaborative workspace.
        </p>
      </section>

      <section>
        <h3 class="text-2xl font-semibold mb-2 mt-8">Key Features</h3>
        <ul class="list-disc pl-6 space-y-1 leading-relaxed">
          <li>
            <strong>Single Configuration File:</strong> Settings, customization, and feature toggles
            in one place, with hot-reload support.
          </li>
          <li>
            <strong>User Management:</strong> Flexible user profiles with customizable fields
            tailored to your community.
          </li>
          <li>
            <strong>Tool Tracking:</strong> Equipment inventory with checkout flows and training
            requirements.
          </li>
          <li>
            <strong>Access Control:</strong> Role-based permissions so the right people reach the
            right resources.
          </li>
          <li>
            <strong>Analytics &amp; Reporting:</strong> Insights into usage patterns and member
            activity.
          </li>
          <li>
            <strong>Calendar Integration:</strong> Schedule events, workshops, and equipment
            reservations.
          </li>
          <li>
            <strong>Outbound Webhooks:</strong> Fire signed HTTP requests on any audit event &mdash;
            see below.
          </li>
          <li>
            <strong>MQTT Publishing:</strong> Push the same events onto an MQTT bus for real-time
            consumers.
          </li>
          <li>
            <strong>Facility Model:</strong> A configurable place hierarchy plus doors, special
            places (Outside, Common Area, …), and a graph view &mdash; see below.
          </li>
          <li>
            <strong>Door Access:</strong> RFID at the door or QR-driven check-in, with audit logs
            and edge-cached decisions that survive a server outage.
          </li>
          <li>
            <strong>Working Hours:</strong> Reusable schedules attached to door rules and to tools
            &mdash; one-click templates make Mon&ndash;Fri 9&ndash;5 a two-second setup.
          </li>
          <li>
            <strong>Secure Edge Component:</strong> Talk to devices on the edge and build new
            automations.
          </li>
          <li>
            <strong>Edge Kiosk Component:</strong> Show calendar events, tool status, and more on
            displays around your space.
          </li>
        </ul>
      </section>

      <section>
        <h3 class="text-2xl font-semibold mb-2 mt-8">Webhooks</h3>
        <p class="leading-relaxed">
          Admins can define any number of outbound webhooks from
          <router-link to="/admin/webhooks" class="link link-primary"
            >Admin &rarr; Webhooks</router-link
          >. Each webhook subscribes to one or more audit events (user logins, tool status changes,
          device registrations, training milestones, and so on) and fires a JSON POST to a URL of
          your choice whenever a matching event occurs.
        </p>
        <ul class="list-disc pl-6 space-y-1 leading-relaxed mt-2">
          <li>
            <strong>Reusable auth credentials:</strong> Define header-based credentials once and
            attach them to any webhook. Stored values are write-only &mdash; never returned through
            the API.
          </li>
          <li>
            <strong>Signed payloads:</strong> Every request carries an
            <code class="text-sm bg-base-300 px-1 rounded"
              >X-Webhook-Signature: sha256=&hellip;</code
            >
            HMAC-SHA256 of the body, keyed by a per-webhook secret you can copy from the edit
            dialog.
          </li>
          <li>
            <strong>Reliable delivery:</strong> Up to three attempts with exponential backoff per
            event, with every attempt recorded in a delivery log you can inspect from the UI.
          </li>
          <li>
            <strong>Test sink:</strong> The
            <code class="text-sm bg-base-300 px-1 rounded">css-webhook-recvr</code> binary ships
            alongside the server &mdash; run it locally to see exactly what each webhook will send
            and to verify signatures.
          </li>
        </ul>
      </section>

      <section>
        <h3 class="text-2xl font-semibold mb-2 mt-8">Facility model</h3>
        <p class="leading-relaxed">
          Most spaces grow into a hierarchy &mdash; rooms inside floors inside buildings. The level
          vocabulary lives in your server config (<code class="text-sm bg-base-300 px-1 rounded"
            >[place].types</code
          >), from a trivial <em>Room &rarr; Spot</em> default to a deep
          <em>Facility &rarr; Building &rarr; Floor &rarr; Room &rarr; Zone &rarr; Spot</em>.
        </p>
        <ul class="list-disc pl-6 space-y-1 leading-relaxed mt-2">
          <li>
            <strong>Self-referential tree:</strong> each place has at most one parent; child levels
            must be strictly deeper than their parent.
          </li>
          <li>
            <strong>Special places:</strong> mark places like <strong>Outside</strong>,
            <strong>Common Area</strong>, or <strong>Parking Lot</strong> as special so they sit
            outside the hierarchy rules.
          </li>
          <li>
            <strong>Doors connect places:</strong> every door explicitly references the place on
            each side, so exterior doors point at a special place (like <em>Outside</em>) rather
            than relying on null semantics.
          </li>
          <li>
            <strong>Tools and devices</strong> can be tagged with a place so administrators can
            answer "what's in this room?".
          </li>
          <li>
            <strong>Graph view</strong> renders places as nodes, doors as labelled edges, and
            special places in amber &mdash; useful for surveying connectivity at a glance.
          </li>
        </ul>
      </section>

      <section>
        <h3 class="text-2xl font-semibold mb-2 mt-8">Working hours</h3>
        <p class="leading-relaxed">
          Reusable weekly schedules &mdash; <em>Member Hours</em>, <em>Staff Always</em>,
          <em>Open House</em> &mdash; that gate when access rules and tools apply. One-click
          templates (<code class="text-sm bg-base-300 px-1 rounded">24 / 7</code>,
          <code class="text-sm bg-base-300 px-1 rounded">9&ndash;5 weekdays</code>,
          <code class="text-sm bg-base-300 px-1 rounded">Weekday evenings</code>, &hellip;) make a
          fresh schedule a two-click affair.
        </p>
        <ul class="list-disc pl-6 space-y-1 leading-relaxed mt-2">
          <li>
            <strong>Attach to a door rule:</strong> the rule is silently inactive outside its
            window. A "members may enter" rule with a Mon&ndash;Fri 9&ndash;5 schedule denies on
            weekends without you writing a deny rule.
          </li>
          <li>
            <strong>Attach to a tool:</strong> when the schedule is closed the tool is removed from
            every authorized list at sync time &mdash; same effect as deactivating it, but
            automatic.
          </li>
          <li>
            <strong>Edge stays simple:</strong> the server recomputes per-device snapshots on the
            minute and republishes when the allow-list changes, so the edge keeps its "is this card
            allowed right now?" decision local.
          </li>
          <li>
            <strong>Public schedules</strong> surface on the home page
            <strong>Hours today</strong> card so visitors can see when the space is open.
          </li>
        </ul>
      </section>

      <section>
        <h3 class="text-2xl font-semibold mb-2 mt-8">Built for Communities</h3>
        <p class="leading-relaxed">
          Whether you're running a small community workshop or a large makerspace,
          <strong>Cooperative Systems:</strong> <i>Spaces</i> scales to meet your needs. Its
          component-based, open-source design means you can customize and extend the platform to fit
          your unique requirements.
        </p>
      </section>

      <section>
        <h3 class="text-2xl font-semibold mb-2 mt-8">Free Software</h3>
        <p class="leading-relaxed">
          <strong>Cooperative Systems:</strong> <i>Spaces</i> is Free Software under the AGPL. The
          source code for this release is served by the server <em>(@TODO)</em>. Contributions are
          accepted on GitHub at
          <a
            href="https://github.com/neiam/cooperative-systems-spaces"
            target="_blank"
            rel="noopener"
            class="link link-primary"
            >github.com/neiam/cooperative-systems-spaces</a
          >.
        </p>
      </section>

      <section>
        <h3 class="text-2xl font-semibold mb-2 mt-8">Commoning</h3>
        <p class="leading-relaxed">
          This project is built in the spirit of
          <a
            href="https://garagehq.deuxfleurs.fr/blog/2025-commoning-opensource/"
            target="_blank"
            rel="noopener"
            class="link link-primary"
            >commoning open source</a
          >
          &mdash; software developed and maintained as shared infrastructure by and for the
          communities that depend on it, rather than as a vendor product handed down from above.
          Forks, adaptations, and contributions back are not just welcome, they're the point. If you
          run a space and have adapted <strong>CS</strong>:<i>S</i> to fit it, please consider
          sending your improvements upstream so the next community doesn't have to solve the same
          problem twice.
        </p>
      </section>
    </div>
  </div>
</template>

<script setup lang="ts">
// About page component
</script>
