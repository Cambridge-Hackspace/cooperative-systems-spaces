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
    return getProfileFields.value.filter(field => field.required)
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

  function validateProfile(profileData: Record<string, any>): { valid: boolean, errors: string[] } {
    const errors: string[] = []
    
    if (!profileConfig.value) {
      return { valid: true, errors: [] }
    }

    for (const field of profileConfig.value.profile_fields) {
      const value = profileData[field.key]
      
      // Check required fields
      if (field.required && (!value || (typeof value === 'string' && value.trim() === ''))) {
        errors.push(`${field.label} is required`)
        continue
      }
      
      // Skip validation if field is empty and not required
      if (!value) continue
      
      // Validate field types
      const fieldType = typeof field.field_type === 'string' ? field.field_type : 'Select'
      
      switch (fieldType) {
        case 'Email':
          if (typeof value !== 'string' || !value.includes('@')) {
            errors.push(`${field.label} must be a valid email address`)
          }
          break
          
        case 'Phone':
          if (typeof value !== 'string' || value.length < 7) {
            errors.push(`${field.label} must be a valid phone number`)
          }
          break
          
        case 'Number':
          if (typeof value !== 'number' && isNaN(Number(value))) {
            errors.push(`${field.label} must be a valid number`)
          }
          break
          
        case 'Date':
          if (typeof value === 'string') {
            const dateRegex = /^\d{4}-\d{2}-\d{2}$/
            if (!dateRegex.test(value)) {
              errors.push(`${field.label} must be in YYYY-MM-DD format`)
            }
          }
          break
          
        case 'Boolean':
          if (typeof value !== 'boolean') {
            errors.push(`${field.label} must be true or false`)
          }
          break
          
        case 'Select':
          if (typeof field.field_type === 'object' && 'Select' in field.field_type) {
            const options = field.field_type.Select.options
            if (!options.includes(String(value))) {
              errors.push(`${field.label} must be one of: ${options.join(', ')}`)
            }
          }
          break
      }
    }

    return {
      valid: errors.length === 0,
      errors
    }
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