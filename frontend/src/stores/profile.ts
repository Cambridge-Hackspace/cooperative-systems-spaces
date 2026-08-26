import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { profileApi } from '@/utils/api'
import type {
  ProfileResponse,
  ProfileConfigResponse,
  UpdateProfileRequest,
  UpdateProfileConfigRequest,
} from '@/types'
import { UserRole } from '@/types'
import { useAuthStore } from './auth'
import { validateProfileAgainst, type ValidationResult } from '@/lib/profileValidation'

export const useProfileStore = defineStore('profile', () => {
  // State
  const profiles = ref<Record<string, ProfileResponse>>({})
  const profileConfig = ref<ProfileConfigResponse | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  // Getters
  const getProfileForUser = computed(() => {
    return (userId: string): ProfileResponse | null => {
      return profiles.value[userId] || null
    }
  })

  const canEditProfile = computed(() => {
    return (targetUserId: string): boolean => {
      const authStore = useAuthStore()
      const currentUser = authStore.user

      if (!currentUser) return false

      // Users can edit their own profile
      if (currentUser.id === targetUserId) return true

      // Staff and admin can edit any profile
      return currentUser.role === UserRole.Staff || currentUser.role === UserRole.Admin
    }
  })

  const canManageProfileConfig = computed(() => {
    const authStore = useAuthStore()
    const currentUser = authStore.user

    if (!currentUser) return false
    return currentUser.role === UserRole.Admin
  })

  const isProfilesEnabled = computed(() => {
    return profileConfig.value?.profiles_enabled ?? false
  })

  const getProfileFields = computed(() => {
    return profileConfig.value?.profile_fields ?? []
  })

  const getRequiredFields = computed(() => {
    return getProfileFields.value.filter((field) => field.required)
  })

  // Actions
  async function fetchUserProfile(userId: string) {
    loading.value = true
    error.value = null

    try {
      const response = await profileApi.getUserProfile(userId)

      if (response.success && response.data) {
        profiles.value[userId] = response.data
        return response.data
      } else {
        throw new Error(response.error || 'Failed to fetch profile')
      }
    } catch (err: any) {
      error.value = err.message || 'Failed to fetch profile'
      throw err
    } finally {
      loading.value = false
    }
  }

  async function updateUserProfile(userId: string, profileData: Record<string, any>) {
    loading.value = true
    error.value = null

    try {
      const request: UpdateProfileRequest = { profile: profileData }
      const response = await profileApi.updateUserProfile(userId, request)

      if (response.success && response.data) {
        profiles.value[userId] = response.data
        return response.data
      } else {
        throw new Error(response.error || 'Failed to update profile')
      }
    } catch (err: any) {
      error.value = err.message || 'Failed to update profile'
      throw err
    } finally {
      loading.value = false
    }
  }

  async function fetchProfileConfig() {
    loading.value = true
    error.value = null

    try {
      const response = await profileApi.getProfileConfig()

      if (response.success && response.data) {
        profileConfig.value = response.data
        return response.data
      } else {
        throw new Error(response.error || 'Failed to fetch profile config')
      }
    } catch (err: any) {
      error.value = err.message || 'Failed to fetch profile config'
      throw err
    } finally {
      loading.value = false
    }
  }

  async function updateProfileConfig(config: UpdateProfileConfigRequest) {
    loading.value = true
    error.value = null

    try {
      const response = await profileApi.updateProfileConfig(config)

      if (response.success && response.data) {
        profileConfig.value = response.data
        return response.data
      } else {
        throw new Error(response.error || 'Failed to update profile config')
      }
    } catch (err: any) {
      error.value = err.message || 'Failed to update profile config'
      throw err
    } finally {
      loading.value = false
    }
  }

  /**
   * Validate against this store's current configuration.
   *
   * The rules live in `@/lib/profileValidation` so that things which cannot
   * reach a Pinia store -- the tier-5 fake API, in particular -- use the same
   * ones rather than a copy.
   */
  function validateProfile(profileData: Record<string, any>): ValidationResult {
    return validateProfileAgainst(profileData, profileConfig.value?.profile_fields ?? null)
  }

  function clearError() {
    error.value = null
  }

  function clearProfiles() {
    profiles.value = {}
  }

  return {
    // State
    profiles,
    profileConfig,
    loading,
    error,

    // Getters
    getProfileForUser,
    canEditProfile,
    canManageProfileConfig,
    isProfilesEnabled,
    getProfileFields,
    getRequiredFields,

    // Actions
    fetchUserProfile,
    updateUserProfile,
    fetchProfileConfig,
    updateProfileConfig,
    validateProfile,
    clearError,
    clearProfiles,
  }
})
