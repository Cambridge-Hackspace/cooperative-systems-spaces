import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type { User, LoginRequest, LoginResponse, RegisterRequest } from '@/types'
import { UserRole } from '@/types'
import { apiClient } from '@/utils/api'

export const useAuthStore = defineStore('auth', () => {
  // State
  const user = ref<User | null>(null)
  const token = ref<string | null>(localStorage.getItem('css_token'))
  const isLoading = ref(false)
  const error = ref<string | null>(null)
  const initialized = ref(false)

  // Getters
  const isAuthenticated = computed(() => !!token.value && !!user.value)
  const isAdmin = computed(() => {
    if (!user.value?.role) return false
    const role = String(user.value.role).toLowerCase()
    return role === 'admin'
  })
  const isStaff = computed(() => {
    if (!user.value?.role) return false
    const role = String(user.value.role).toLowerCase()
    return role === 'staff' || role === 'admin'
  })
  const isMember = computed(() => {
    if (!user.value?.role) return false
    const role = String(user.value.role).toLowerCase()
    return role === 'member' || role === 'staff' || role === 'admin'
  })
  const userRole = computed(() => user.value?.role)
  const userName = computed(() => user.value?.username)
  const userFullName = computed(() => user.value?.full_name)

  // Actions
  const login = async (credentials: LoginRequest): Promise<boolean> => {
    isLoading.value = true
    error.value = null

    try {
      const response = await apiClient.post<LoginResponse>('/auth/login', credentials)
      
      if (response.success && response.data) {
        token.value = response.data.token
        user.value = response.data.user
        
        // Store token in localStorage
        localStorage.setItem('css_token', response.data.token)
        
        return true
      } else {
        error.value = response.error || 'Login failed'
        return false
      }
    } catch (err: any) {
      error.value = err.response?.data?.error || 'Network error during login'
      return false
    } finally {
      isLoading.value = false
    }
  }

  const register = async (userData: RegisterRequest): Promise<boolean> => {
    isLoading.value = true
    error.value = null

    try {
      const response = await apiClient.post<User>('/auth/register', userData)
      
      if (response.success) {
        // Registration successful, but user needs to login
        return true
      } else {
        error.value = response.error || 'Registration failed'
        return false
      }
    } catch (err: any) {
      error.value = err.response?.data?.error || 'Network error during registration'
      return false
    } finally {
      isLoading.value = false
    }
  }

  const logout = () => {
    user.value = null
    token.value = null
    localStorage.removeItem('css_token')
    // Note: We could also call the server logout endpoint here if implemented
  }

  const getCurrentUser = async (): Promise<boolean> => {
    if (!token.value) {
      return false
    }

    isLoading.value = true
    error.value = null

    try {
      const response = await apiClient.get<User>('/auth/me')
      
      if (response.success && response.data) {
        user.value = response.data
        return true
      } else {
        // Token might be invalid
        logout()
        return false
      }
    } catch (err: any) {
      // Token is likely invalid
      logout()
      return false
    } finally {
      isLoading.value = false
    }
  }

  const updateProfile = async (updates: Partial<User>): Promise<boolean> => {
    if (!user.value) return false

    isLoading.value = true
    error.value = null

    try {
      const response = await apiClient.put<User>(`/users/${user.value.id}`, updates)
      
      if (response.success && response.data) {
        user.value = response.data
        return true
      } else {
        error.value = response.error || 'Profile update failed'
        return false
      }
    } catch (err: any) {
      error.value = err.response?.data?.error || 'Network error during profile update'
      return false
    } finally {
      isLoading.value = false
    }
  }

  const hasRole = (requiredRole: UserRole): boolean => {
    if (!user.value) return false
    
    const roleHierarchy: Record<string, number> = {
      'unknown': 0,
      'newbie': 1,
      'member': 2,
      'staff': 3,
      'admin': 4
    }
    
    const userRoleString = String(user.value.role).toLowerCase()
    const requiredRoleString = String(requiredRole).toLowerCase()
    
    const userRoleLevel = roleHierarchy[userRoleString] || 0
    const requiredRoleLevel = roleHierarchy[requiredRoleString] || 0
    
    return userRoleLevel >= requiredRoleLevel
  }

  const clearError = () => {
    error.value = null
  }

  // Initialize auth state on store creation
  const initialize = async () => {
    if (token.value) {
      await getCurrentUser()
    }
    initialized.value = true
  }

  return {
    // State
    user,
    token,
    isLoading,
    error,
    initialized,
    
    // Getters
    isAuthenticated,
    isAdmin,
    isStaff,
    isMember,
    userRole,
    userName,
    userFullName,
    
    // Actions
    login,
    register,
    logout,
    getCurrentUser,
    updateProfile,
    hasRole,
    clearError,
    initialize,
  }
})