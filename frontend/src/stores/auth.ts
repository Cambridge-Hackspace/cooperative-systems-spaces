import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type {
  User,
  LoginRequest,
  LoginResponse,
  LoginOutcome,
  MfaChallenge,
  RegisterRequest,
} from '@/types'
import { isMfaChallenge, UserRole } from '@/types'
import { apiClient } from '@/utils/api'

export const useAuthStore = defineStore('auth', () => {
  // State
  const user = ref<User | null>(null)
  // #135: the session JWT now lives in an httpOnly cookie the browser sends
  // automatically. The SPA never holds it in JS, so there is no `token` here and
  // nothing in localStorage for an XSS on the app origin to steal. "Are we
  // signed in?" is answered by whether /auth/me returned a user.
  const isLoading = ref(false)
  const error = ref<string | null>(null)
  const initialized = ref(false)
  /** When set, an MFA challenge is awaiting completion (set by `login`). */
  const pendingMfa = ref<MfaChallenge | null>(null)
  /** True when the just-completed login flagged the user must enroll in MFA. */
  const mustEnrollMfa = ref(false)
  /** RBAC role names the signed-in user holds, from /auth/me. */
  const roles = ref<string[]>([])
  /** Effective RBAC permission keys, from /auth/me. Gated UI reads these. */
  const permissions = ref<string[]>([])

  // Getters
  const isAuthenticated = computed(() => !!user.value)
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
    return role === 'active' || role === 'staff' || role === 'admin'
  })
  /** True when the user's effective permissions include `key`. */
  const hasPermission = (key: string): boolean => permissions.value.includes(key)
  /** True when the user holds any of `keys`. */
  const hasAnyPermission = (keys: string[]): boolean =>
    keys.some((k) => permissions.value.includes(k))

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

      const data = response.data
      // The server set the httpOnly session cookie on this response; the token in
      // the body is ignored (#135). Only the user is kept, in memory.
      user.value = data.user
      mustEnrollMfa.value = !!data.must_enroll_mfa
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
    // The /verify response set the httpOnly session cookie (#135); the body token
    // is ignored. Keep only the user in memory.
    user.value = resp.user
    mustEnrollMfa.value = !!resp.must_enroll_mfa
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

  /** Drop the in-memory signed-in state. Does not touch the server cookie. */
  const clearSession = () => {
    roles.value = []
    permissions.value = []
    user.value = null
  }

  const logout = async () => {
    // The session cookie is httpOnly, so JS cannot clear it -- the server must,
    // via /auth/logout (#135). Best-effort: even if the call fails (offline, an
    // already-expired session), the in-memory state is still cleared so the SPA
    // reflects a signed-out user.
    try {
      await apiClient.post('/auth/logout')
    } catch (err) {
      console.error(err)
    }
    clearSession()
  }

  const getCurrentUser = async (): Promise<boolean> => {
    // No client-side token to gate on: the httpOnly cookie decides. Always ask
    // /auth/me; a 401 (no/expired cookie) resolves us to signed-out.
    isLoading.value = true
    error.value = null

    try {
      const response = await apiClient.get<User & { roles?: string[]; permissions?: string[] }>(
        '/auth/me'
      )

      if (response.success && response.data) {
        user.value = response.data
        // /auth/me flattens the user and adds the RBAC role + permission sets.
        roles.value = response.data.roles ?? []
        permissions.value = response.data.permissions ?? []
        return true
      } else {
        // No valid session -- clear in memory. No server round-trip: the cookie
        // is already absent/expired, so calling /auth/logout would be pointless.
        clearSession()
        return false
      }
    } catch (err: any) {
      // Logged rather than discarded: a swallowed error is indistinguishable
      // from a successful no-op to anyone reading the console.
      console.error(err)
      // Session is likely absent or expired.
      clearSession()
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
      guest: 1,
      historical: 2,
      active: 3,
      staff: 4,
      admin: 5,
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

  // Initialize auth state on store creation. Always probe /auth/me: with the
  // token in an httpOnly cookie there is no client-side flag to gate on, and the
  // cookie (if any) rehydrates the session; otherwise we resolve to signed-out.
  const initialize = async () => {
    await getCurrentUser()
    initialized.value = true
  }

  return {
    // State
    user,
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
    roles,
    permissions,
    hasPermission,
    hasAnyPermission,
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
