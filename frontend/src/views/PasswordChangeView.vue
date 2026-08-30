<template>
  <div class="container mx-auto px-4 py-8 max-w-2xl">
    <div class="breadcrumbs text-sm mb-6">
      <ul>
        <li><router-link to="/" class="link">Home</router-link></li>
        <li><router-link :to="`/profile/me`" class="link">My Profile</router-link></li>
        <li>Change Password</li>
      </ul>
    </div>

    <ChangePasswordCard />
  </div>
</template>

<script setup lang="ts">
import ChangePasswordCard from '@/components/ChangePasswordCard.vue'
</script>
