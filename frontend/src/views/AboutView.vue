<template>
  <div class="container mx-auto px-4 py-8">
    <div class="hero min-h-96">
      <div class="hero-content text-center">
        <div class="max-w-md">
          <h1 class="text-5xl">About <strong>Cooperative Systems:</strong> <i>Spaces</i></h1>
          <p class="py-6">
            Welcome to your collaborative workspace management system. 
            Manage your makerspace, hackerspace, or community workspace with ease.
          </p>
        </div>
      </div>
    </div>

    <!-- Features Section -->
    <div class="grid gap-8 md:grid-cols-3 mt-16">
      <div class="card bg-base-200 shadow-xl">
        <div class="card-body items-center text-center">
          <svg class="w-12 h-12 text-primary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z"/>
          </svg>
          <h2 class="card-title">User Profiles</h2>
          <p>Customizable user profiles with configurable fields for your community.</p>
        </div>
      </div>

      <div class="card bg-base-200 shadow-xl">
        <div class="card-body items-center text-center">
          <svg class="w-12 h-12 text-primary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19.428 15.428a2 2 0 00-1.022-.547l-2.387-.477a6 6 0 00-3.86.517l-.318.158a6 6 0 01-3.86.517L6.05 15.21a2 2 0 00-1.806.547M8 4h8l-1 1v5.172a2 2 0 00.586 1.414l5 5c1.26 1.26.367 3.414-1.415 3.414H4.828c-1.782 0-2.674-2.154-1.414-3.414l5-5A2 2 0 009 10.172V5L8 4z"/>
          </svg>
          <h2 class="card-title">Tool Management</h2>
          <p>Track tools, equipment, and resources with checkout systems and training requirements.</p>
        </div>
      </div>

      <div class="card bg-base-200 shadow-xl">
        <div class="card-body items-center text-center">
          <svg class="w-12 h-12 text-primary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v4a2 2 0 01-2 2h-2a2 2 0 01-2-2z"/>
          </svg>
          <h2 class="card-title">Analytics</h2>
          <p>Track usage, member activity, and generate reports for your space.</p>
        </div>
      </div>
    </div>

    <!-- Additional Information Section -->
    <div class="mt-16 prose lg:prose-xl mx-auto">
      <h2 class="text-2xl text-gray-800">What is <strong>Cooperative Systems:</strong> <i>Spaces</i>?</h2>
      <p>
        <strong>Cooperative Systems:</strong> <i>Spaces</i> is a comprehensive management platform designed specifically for
        makerspaces, hackerspaces, and community workshops. Our system provides the tools you need
        to efficiently manage your collaborative workspace.
      </p>

      <h3 class="text-lg font-medium mb-2 mt-4">Key Features</h3>
      <ul>
        <li><strong>Single Configuration File:</strong> Important Settings, Customization, and Feature Control in a single place, with the server supporting configuration hot-reloads. </li>
        <li><strong>User Management:</strong> Flexible user profiles with customizable fields tailored to your community's needs.</li>
        <li><strong>Tool Tracking:</strong> Keep track of all your equipment with checkout systems and training requirements</li>
        <li><strong>Access Control:</strong> Role-based permissions ensure the right people have access to the right resources</li>
        <li><strong>Analytics & Reporting:</strong> Gain insights into usage patterns and member activity</li>
        <li><strong>Calendar Integration:</strong> Schedule events, workshops, and equipment reservations</li>
        <li><strong>Notification Platform:</strong> Webhooks / MQTT publishing for any kind of system event</li>
        <li><strong>Secure Edge Component:</strong> Talk to devices on the edge directly and build new automations</li>
        <li><strong>Edge Kiosk Component:</strong> Show Calendar Events, Tool Statuses and more on multiple displays within your space</li>
      </ul>

      <h3 class="text-lg font-medium mb-2 mt-4">Built for Communities</h3>
      <p>
        Whether you're running a small community workshop or a large makerspace, Cooperative Systems Spaces 
        scales to meet your needs. Our component-based open-source approach means you can customize and extend the platform
        to fit your unique requirements.
      </p>
      <h3 class="text-lg font-medium mb-2 mt-4">Free Software</h3>
      <p>
        <strong>Cooperative Systems</strong>: <i>Spaces</i> is free software under the AGPL. The source code for this release is served by the server @TODO. Contributions are presently accepted on GitHub at <a href="https://github.com/neiam/cooperative-systems-spaces">https://github.com/neiam/cooperative-systems-spaces</a>.
      </p>
    </div>
  </div>
</template>

<script setup lang="ts">
// About page component
</script>
