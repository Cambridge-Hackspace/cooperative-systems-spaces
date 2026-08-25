<template>
  <div class="card bg-base-100 shadow-xl">
    <div class="card-body">
      <div class="flex items-center justify-between mb-6">
        <h2 class="card-title text-2xl">
          <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z"/>
          </svg>
          {{ isOwnProfile ? 'My Profile' : `${user?.full_name || 'User'}'s Profile` }}
        </h2>
        
        <div class="flex gap-2">
          <button
            v-if="canEdit && !isEditing"
            @click="startEditing"
            class="btn btn-primary btn-sm"
            :disabled="loading"
          >
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z"/>
            </svg>
            Edit Profile
          </button>
          
          <button
            v-if="isEditing"
            @click="cancelEditing"
            class="btn btn-ghost btn-sm"
            :disabled="loading"
          >
            Cancel
          </button>
          
          <button
            v-if="isEditing"
            @click="saveProfile"
            class="btn btn-primary btn-sm"
            :disabled="loading || !isFormValid"
          >
            <span v-if="loading" class="loading loading-spinner loading-sm"></span>
            <svg v-else class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"/>
            </svg>
            Save Profile
          </button>
        </div>
      </div>

      <!-- Profile disabled message -->
      <div v-if="!profileStore.isProfilesEnabled" class="alert alert-info">
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/>
        </svg>
        <div>
          <h3 class="font-bold">Profiles Disabled</h3>
          <div class="text-xs">User profiles are currently disabled by the administrator.</div>
        </div>
      </div>

      <!-- Loading state -->
      <div v-else-if="loading && !profileData" class="flex items-center justify-center py-12">
        <div class="loading loading-spinner loading-lg"></div>
        <span class="ml-3">Loading profile...</span>
      </div>

      <!-- Error state -->
      <div v-else-if="error && !profileData" class="alert alert-error">
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/>
        </svg>
        <div>
          <h3 class="font-bold">Error loading profile</h3>
          <div class="text-xs">{{ error }}</div>
        </div>
        <button @click="loadProfile" class="btn btn-sm">Retry</button>
      </div>

      <!-- Profile content -->
      <div v-else-if="profileStore.isProfilesEnabled && profileFields.length > 0" class="space-y-6">
        <!-- Validation errors -->
        <div v-if="validationErrors.length > 0" class="alert alert-warning">
          <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L3.732 16c-.77.833.192 2.5 1.732 2.5z"/>
          </svg>
          <div>
            <h3 class="font-bold">Please fix the following errors:</h3>
            <ul class="text-xs list-disc list-inside">
              <li v-for="error in validationErrors" :key="error">{{ error }}</li>
            </ul>
          </div>
        </div>

        <!-- Profile fields -->
        <div class="grid gap-6 md:grid-cols-2">
          <ProfileField
            v-for="field in profileFields"
            :key="field.key"
            :field="field"
            :model-value="getFieldValue(field.key)"
            :error-message="fieldErrors[field.key]"
            :disabled="!isEditing"
            @update:model-value="updateFieldValue(field.key, $event)"
            @blur="validateField(field.key)"
          />
        </div>
      </div>

      <!-- Profiles enabled, but no fields have been configured yet -->
      <div v-else-if="profileStore.isProfilesEnabled && profileFields.length === 0" class="text-center py-12">
        <svg class="w-16 h-16 mx-auto text-base-content/30 mb-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z"/>
        </svg>
        <h3 class="text-lg font-medium text-base-content/70 mb-2">No Profile Fields</h3>
        <p class="text-base-content/50">No profile fields have been configured.</p>
        <div v-if="profileStore.canManageProfileConfig" class="mt-4">
          <router-link to="/admin/profile-config" class="btn btn-primary btn-sm">
            Configure Profile Fields
          </router-link>
        </div>
      </div>

      <!-- Profiles disabled -->
      <div v-else class="text-center py-12">
        <svg class="w-16 h-16 mx-auto text-base-content/30 mb-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z"/>
        </svg>
        <h3 class="text-lg font-medium text-base-content/70 mb-2">Profile Not Available</h3>
        <p class="text-base-content/50">User profiles are not configured or enabled.</p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useProfileStore } from '@/stores/profile'
import ProfileField from './ProfileField.vue'
import type { User } from '@/types'

interface Props {
  userId?: string
  user?: User
}

const props = withDefaults(defineProps<Props>(), {
  userId: '',
})

const authStore = useAuthStore()
const profileStore = useProfileStore()

// Local state
const isEditing = ref(false)
const editProfileData = ref<Record<string, any>>({})
const fieldErrors = ref<Record<string, string>>({})
const validationErrors = ref<string[]>([])

// Computed properties
const targetUserId = computed(() => props.userId || authStore.user?.id || '')
const isOwnProfile = computed(() => targetUserId.value === authStore.user?.id)
const canEdit = computed(() => profileStore.canEditProfile(targetUserId.value))

const profileData = computed(() => profileStore.getProfileForUser(targetUserId.value))
const profileFields = computed(() => profileStore.getProfileFields)
const loading = computed(() => profileStore.loading)
const error = computed(() => profileStore.error)

const isFormValid = computed(() => {
  return validationErrors.value.length === 0 && Object.keys(fieldErrors.value).length === 0
})

// Methods
function getFieldValue(fieldKey: string) {
  if (isEditing.value) {
    return editProfileData.value[fieldKey]
  }
  return profileData.value?.profile?.[fieldKey] || null
}

function updateFieldValue(fieldKey: string, value: any) {
  editProfileData.value[fieldKey] = value
  // Clear field error when user starts typing
  delete fieldErrors.value[fieldKey]
  validateForm()
}

function validateField(fieldKey: string) {
  const field = profileFields.value.find(f => f.key === fieldKey)
  if (!field) return

  const value = editProfileData.value[fieldKey]
  
  // Clear previous error
  delete fieldErrors.value[fieldKey]
  
  // Required field validation
  if (field.required && (!value || (typeof value === 'string' && value.trim() === ''))) {
    fieldErrors.value[fieldKey] = `${field.label} is required`
    return
  }
  
  // Skip further validation if field is empty and not required
  if (!value) return
  
  // Type-specific validation
  const fieldType = typeof field.field_type === 'string' ? field.field_type : 'Select'
  
  switch (fieldType) {
    case 'Email':
      if (typeof value !== 'string' || !value.includes('@')) {
        fieldErrors.value[fieldKey] = `${field.label} must be a valid email address`
      }
      break
      
    case 'Phone':
      if (typeof value !== 'string' || value.length < 7) {
        fieldErrors.value[fieldKey] = `${field.label} must be a valid phone number`
      }
      break
      
    case 'Number':
      if (typeof value !== 'number' && isNaN(Number(value))) {
        fieldErrors.value[fieldKey] = `${field.label} must be a valid number`
      }
      break
      
    case 'Date':
      if (typeof value === 'string') {
        const dateRegex = /^\d{4}-\d{2}-\d{2}$/
        if (!dateRegex.test(value)) {
          fieldErrors.value[fieldKey] = `${field.label} must be in YYYY-MM-DD format`
        }
      }
      break
  }
}

function validateForm() {
  // Validate all fields
  fieldErrors.value = {}
  profileFields.value.forEach(field => {
    validateField(field.key)
  })
  
  // Use store validation for additional checks
  const validation = profileStore.validateProfile(editProfileData.value)
  validationErrors.value = validation.errors
}

function startEditing() {
  isEditing.value = true
  // Copy current profile data to edit buffer
  editProfileData.value = { ...profileData.value?.profile } || {}
  fieldErrors.value = {}
  validationErrors.value = []
}

function cancelEditing() {
  isEditing.value = false
  editProfileData.value = {}
  fieldErrors.value = {}
  validationErrors.value = []
  profileStore.clearError()
}

async function saveProfile() {
  validateForm()
  
  if (!isFormValid.value) {
    return
  }
  
  try {
    await profileStore.updateUserProfile(targetUserId.value, editProfileData.value)
    isEditing.value = false
    editProfileData.value = {}
    fieldErrors.value = {}
    validationErrors.value = []
  } catch (err) {
    // Error is already handled by the store
  }
}

async function loadProfile() {
  if (!targetUserId.value) return
  
  try {
    await profileStore.fetchUserProfile(targetUserId.value)
  } catch (err) {
    // Error is already handled by the store
  }
}

async function loadProfileConfig() {
  if (!profileStore.profileConfig) {
    try {
      await profileStore.fetchProfileConfig()
    } catch (err) {
      // Error is already handled by the store
    }
  }
}

// Lifecycle
onMounted(async () => {
  await loadProfileConfig()
  if (targetUserId.value && profileStore.isProfilesEnabled) {
    await loadProfile()
  }
})

// Watch for user changes
watch(() => targetUserId.value, async (newUserId) => {
  if (newUserId && profileStore.isProfilesEnabled) {
    await loadProfile()
  }
})
</script>