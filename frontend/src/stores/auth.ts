import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type { User, LoginRequest, LoginResponse, RegisterRequest, UserRole } from '@/types'
import { apiClient } from '@/utils/api'

export const useAuthStore = defineStore('auth', () => {
  // State
  const user = ref<User | null>(null)
  const token = ref<string | null>(localStorage.getItem('css_token'))
  const isLoading = ref(false)
  const error = ref<string | null>(null)

  // Getters
  const isAuthenticated = computed(() => !!token.value && !!user.value)
  const isAdmin = computed(() => user.value?.role === 'admin')
  const isStaff = computed(() => ['staff', 'admin'].includes(user.value?.role ?? ''))
  const isMember = computed(() => ['member', 'staff', 'admin'].includes(user.value?.role ?? ''))
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
    
    const roleHierarchy = {
      unknown: 0,
      newbie: 1,
      member: 2,
      staff: 3,
      admin: 4
    }
    
    const userRoleLevel = roleHierarchy[user.value.role as keyof typeof roleHierarchy] || 0
    const requiredRoleLevel = roleHierarchy[requiredRole as keyof typeof roleHierarchy] || 0
    
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
  }

  return {
    // State
    user,
    token,
    isLoading,
    error,
    
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