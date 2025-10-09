<template>
  <div class="container mx-auto px-4 py-8">
    <div class="hero min-h-96">
      <div class="hero-content text-center">
        <div class="max-w-md">
          <h1 class="text-5xl font-bold">Cooperative Systems Spaces</h1>
          <p class="py-6">
            Welcome to your collaborative workspace management system. 
            Manage your makerspace, hackerspace, or community workspace with ease.
          </p>
          <div class="flex gap-4 justify-center">
            <router-link 
              v-if="!authStore.isAuthenticated" 
              to="/login" 
              class="btn btn-primary"
            >
              Get Started
            </router-link>
            <router-link 
              v-if="authStore.isAuthenticated" 
              to="/profile/me" 
              class="btn btn-primary"
            >
              View My Profile
            </router-link>
            <router-link 
              v-if="authStore.isAuthenticated"
              to="/tools"
              class="btn btn-secondary"
            >
              Browse Tools
            </router-link>
            <router-link
              v-if="authStore.isAuthenticated && authStore.isAdmin"
              to="/admin" 
              class="btn btn-secondary"
            >
              Admin Panel
            </router-link>
          </div>
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
  </div>
</template>

<script setup lang="ts">
import { useAuthStore } from '@/stores/auth'

const authStore = useAuthStore()
</script>