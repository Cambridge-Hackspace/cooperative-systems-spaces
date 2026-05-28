import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type { User, LoginRequest, LoginResponse, LoginOutcome, MfaChallenge, RegisterRequest } from '@/types'
import { isMfaChallenge, UserRole } from '@/types'
import { apiClient } from '@/utils/api'

export const useAuthStore = defineStore('auth', () => {
  // State
  const user = ref<User | null>(null)
  const token = ref<string | null>(localStorage.getItem('css_token'))
  const isLoading = ref(false)
  const error = ref<string | null>(null)
  const initialized = ref(false)
  /** When set, an MFA challenge is awaiting completion (set by `login`). */
  const pendingMfa = ref<MfaChallenge | null>(null)
  /** True when the just-completed login flagged the user must enroll in MFA. */
  const mustEnrollMfa = ref(false)

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
  /**
   * Result of `login`:
   *   - 'ok'   → token issued; user is signed in.
   *   - 'mfa'  → MFA challenge required; see `pendingMfa`. Caller should route
   *              to the challenge form and eventually call `completeMfa`.
   *   - 'error'→ login failed; see `error`.
   */
  type LoginResult = 'ok' | 'mfa' | 'error'

  const login = async (credentials: LoginRequest): Promise<LoginResult> => {
    isLoading.value = true
    error.value = null
    pendingMfa.value = null
    mustEnrollMfa.value = false

    try {
      const response = await apiClient.post<LoginOutcome>('/auth/login', credentials)

      if (!response.success || !response.data) {
        error.value = response.error || 'Login failed'
        return 'error'
      }

      if (isMfaChallenge(response.data)) {
        pendingMfa.value = response.data
        return 'mfa'
      }

      const data = response.data as LoginResponse
      token.value = data.token
      user.value = data.user
      mustEnrollMfa.value = !!data.must_enroll_mfa
      localStorage.setItem('css_token', data.token)
      return 'ok'
    } catch (err: any) {
      error.value = err.response?.data?.error || 'Network error during login'
      return 'error'
    } finally {
      isLoading.value = false
    }
  }

  /** Apply a successful MFA `/verify` response to the auth store. */
  const completeMfa = (resp: LoginResponse) => {
    token.value = resp.token
    user.value = resp.user
    mustEnrollMfa.value = !!resp.must_enroll_mfa
    localStorage.setItem('css_token', resp.token)
    pendingMfa.value = null
  }

  const cancelMfa = () => {
    pendingMfa.value = null
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
    pendingMfa,
    mustEnrollMfa,

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
    completeMfa,
    cancelMfa,
    register,
    logout,
    getCurrentUser,
    updateProfile,
    hasRole,
    clearError,
    initialize,
  }
})