import axios, { type AxiosResponse, type AxiosError } from 'axios'
import type {
  ApiResponse,
  ProfileResponse,
  UpdateProfileRequest,
  ProfileConfigResponse,
  UpdateProfileConfigRequest,
  User,
  UserRole,
  AuditLog,
  Tool,
  ToolQuery,
  CreateToolRequest,
  UpdateToolRequest,
  ChangeToolStatusRequest,
  ToolStatus,
  ToolEvent,
  TrainingStep,
  CreateTrainingStepRequest,
  UpdateTrainingStepRequest,
  TrainingPrerequisite,
  CreateTrainingPrerequisiteRequest,
  UserTrainingProgress,
  StartTrainingRequest,
  CompleteTrainingRequest,
  TrainingInstructor,
  CertifyInstructorRequest,
  ToolTrainingOverview,
  TrainingQuery,
  ToolTrainer,
  ToolTrainerWithUser,
  AssignTrainerRequest,
  UpdateTrainerRequest,
  TrainingRecord,
  TrainingRecordWithUsers,
  CreateTrainingRecordRequest,
  UpdateTrainingRecordRequest,
  TrainingRecordsQuery,
} from '@/types'
import { useAuthStore } from '@/stores/auth'

// Create axios instance with default config
const api = axios.create({
  baseURL: '/api',
  timeout: 30000,
  headers: {
    'Content-Type': 'application/json',
  },
})

// Request interceptor to add auth token
api.interceptors.request.use(
  (config) => {
    const authStore = useAuthStore()
    const token = authStore.token

    if (token) {
      config.headers.Authorization = `Bearer ${token}`
    }

    return config
  },
  (error: unknown) => {
    // Typed so the rejection reason is visibly an Error rather than `any`.
    // axios always rejects a request interceptor with an AxiosError.
    return Promise.reject(
      error instanceof Error
        ? error
        : new Error(typeof error === 'string' ? error : JSON.stringify(error))
    )
  }
)

// Response interceptor to handle common error cases
api.interceptors.response.use(
  (response: AxiosResponse) => {
    return response
  },
  (error: AxiosError) => {
    const authStore = useAuthStore()

    // Handle 401 unauthorized - clear auth and redirect to login
    if (error.response?.status === 401) {
      authStore.logout()
      // Note: In a real app, you might want to redirect to login page here
      // But we'll let the components handle this
    }

    return Promise.reject(error)
  }
)

// API helper functions
/**
 * Turn a rejected request into the envelope the rest of the app expects.
 *
 * Forty-two call sites used to do this inline, and every one read
 * `error.message` -- which for axios is the status restated ("Request failed
 * with status code 409") while the server's own explanation sat unread in
 * `error.response.data.error`. So every component that showed a failure to a
 * user showed the wrong half of it.
 *
 * What this does NOT fix: a caller that never reads `.success` still cannot
 * tell a refusal from a success, because this still resolves. No helper can fix
 * that from here; the components that did it are corrected individually and
 * their specs assert the correction.
 */
export function envelopeError<T>(error: unknown, fallback?: string): ApiResponse<T> {
  const e = error as {
    response?: { data?: { error?: unknown; message?: unknown } }
    message?: unknown
  }
  const fromBody = e?.response?.data?.error ?? e?.response?.data?.message
  const text = (v: unknown): string | null => (typeof v === 'string' && v.trim() !== '' ? v : null)

  const serverSaid = text(fromBody)
  if (serverSaid === null) {
    // Not discarded, just not shown. `e.message` is the only remaining clue to
    // why a request failed with no body, and swallowing it makes a transport
    // failure indistinguishable from a server that answered `{}`.
    console.warn('[api] request failed with no server message:', e?.message ?? e)
  }

  // `e.message` is deliberately NOT a candidate for the returned string, and
  // this is the second time that decision has been made in this function.
  //
  // Every value axios puts there is a developer string: "Request failed with
  // status code 500", "Network Error", "timeout of 10000ms exceeded",
  // "canceled". Showing the first of those to a user is the exact defect this
  // helper was written to remove -- forty-two call sites read `error.message`
  // and rendered the status restated, while the server's own explanation sat
  // unread in `response.data.error`. Keeping it as a *second* choice fixed the
  // common case and left the same class of string reaching users whenever the
  // body was empty.
  //
  // The fallback is written by the caller, at the call site, in the words a
  // user should read ("Failed to load door"). It is the better answer whenever
  // the server has not supplied one, so it wins outright.
  //
  // Caught by tests/e2e/door-checkin.spec.ts on the first run of the browser
  // tier: it asserts a dropped connection shows "Failed to load door", while
  // tests/unit/api-envelope.spec.ts asserted it shows "Network Error". Two of
  // my own tests contradicting each other, for as long as the tier covering
  // the real behavior had never executed.
  // `?? undefined` rather than `?? ''`: an absent `error` lets a caller's own
  // `r.error || 'Failed to load door'` fire, which is the point of the
  // optional parameter. An empty string is truthy-adjacent enough to be a trap
  // and would render a blank alert.
  return { success: false, error: serverSaid ?? fallback ?? undefined }
}

export const apiClient = {
  async get<T>(url: string, params?: any): Promise<ApiResponse<T>> {
    const response = await api.get<ApiResponse<T>>(url, { params })
    return response.data
  },

  async post<T>(url: string, data?: any): Promise<ApiResponse<T>> {
    const response = await api.post<ApiResponse<T>>(url, data)
    return response.data
  },

  async put<T>(url: string, data?: any): Promise<ApiResponse<T>> {
    const response = await api.put<ApiResponse<T>>(url, data)
    return response.data
  },

  async patch<T>(url: string, data?: any): Promise<ApiResponse<T>> {
    const response = await api.patch<ApiResponse<T>>(url, data)
    return response.data
  },

  async delete<T>(url: string): Promise<ApiResponse<T>> {
    const response = await api.delete<ApiResponse<T>>(url)
    return response.data
  },

  // Raw axios instance for direct access if needed
  raw: api,
}

// Profile API functions
export const profileApi = {
  // Get user profile
  getUserProfile(userId: string): Promise<ApiResponse<ProfileResponse>> {
    return apiClient.get(`/profiles/${userId}`)
  },

  // Update user profile
  updateUserProfile(
    userId: string,
    data: UpdateProfileRequest
  ): Promise<ApiResponse<ProfileResponse>> {
    return apiClient.put(`/profiles/${userId}`, data)
  },

  // Get profile configuration (admin only)
  getProfileConfig(): Promise<ApiResponse<ProfileConfigResponse>> {
    return apiClient.get('/profiles/config')
  },

  // Update profile configuration (admin only)
  updateProfileConfig(
    data: UpdateProfileConfigRequest
  ): Promise<ApiResponse<ProfileConfigResponse>> {
    return apiClient.put('/profiles/config', data)
  },
}

// User/Roster API functions
export const userApi = {
  // Get all users for roster (admin only)
  getAllUsers(): Promise<ApiResponse<any>> {
    // Use the admin roster endpoint instead
    return apiClient
      .get('/admin/roster')
      .then((response) => {
        if (response.success && response.data) {
          // Transform the response to match expected paginated format
          const users = Array.isArray(response.data) ? response.data : []
          return {
            success: true,
            data: {
              items: users,
              page: 1,
              per_page: users.length,
              total: users.length,
              total_pages: 1,
            },
          }
        }
        return response
      })
      .catch((error) => {
        console.error('Error fetching roster:', error)
        return envelopeError(error, 'Failed to fetch users')
      })
  },

  // Get users for training purposes (trainers can access)
  getUsersForTraining(toolId?: string): Promise<ApiResponse<any>> {
    // Use the new training-specific roster endpoint that allows trainers to see all users
    const endpoint = toolId ? `/training/roster/${toolId}` : '/training/roster'

    return apiClient
      .get(endpoint)
      .then((response) => {
        if (response.success && response.data) {
          const users = Array.isArray(response.data)
            ? response.data
            : (response.data as any).items || []
          return {
            success: true,
            data: {
              items: users,
              page: 1,
              per_page: users.length,
              total: users.length,
              total_pages: 1,
            },
          }
        }
        return response
      })
      .catch((error) => {
        // If training roster endpoint fails, try the old trainer-specific endpoint
        if (error.response?.status === 404) {
          console.log('Training roster endpoint not available, trying legacy trainer endpoint')
          return apiClient
            .get('/trainers/users')
            .then((response) => {
              if (response.success && response.data) {
                const users = Array.isArray(response.data)
                  ? response.data
                  : (response.data as any).items || []
                return {
                  success: true,
                  data: {
                    items: users,
                    page: 1,
                    per_page: users.length,
                    total: users.length,
                    total_pages: 1,
                  },
                }
              }
              return response
            })
            .catch((legacyError) => {
              // If both training roster and legacy trainer endpoints fail, try admin roster for admins
              if (legacyError.response?.status === 401 || legacyError.response?.status === 403) {
                console.log('Trainer endpoints not accessible, trying admin roster')
                return this.getAllUsers()
              }
              throw legacyError
            })
        }

        console.error('Error fetching users for training:', error)
        return envelopeError(error, 'Failed to fetch users for training')
      })
  },

  // Get training history for a specific tool (trainers and staff can access)
  getTrainingHistory(toolId: string, queryParams?: any): Promise<ApiResponse<any>> {
    return apiClient
      .get(`/training/history/${toolId}`, queryParams)
      .then((response) => {
        if (response.success && response.data) {
          const records = Array.isArray(response.data) ? response.data : []
          return {
            success: true,
            data: records,
          }
        }
        return response
      })
      .catch((error) => {
        console.error('Error fetching training history:', error)
        return envelopeError(error, 'Failed to fetch training history')
      })
  },

  // Get single user
  getUser(userId: string): Promise<ApiResponse<User>> {
    return apiClient.get(`/users/${userId}`)
  },

  // Update user role (admin only) - FIXED endpoint
  updateUserRole(userId: string, role: UserRole): Promise<ApiResponse<User>> {
    return apiClient.put(`/admin/users/${userId}/role`, { role })
  },

  // Update user (admin/staff only)
  updateUser(userId: string, updates: Partial<User>): Promise<ApiResponse<User>> {
    return apiClient.put(`/users/${userId}`, updates)
  },

  // Deactivate user (admin only)
  deactivateUser(userId: string): Promise<ApiResponse<User>> {
    return apiClient.put(`/admin/users/${userId}/deactivate`)
  },

  // Activate user (admin only)
  activateUser(userId: string): Promise<ApiResponse<User>> {
    return apiClient.put(`/admin/users/${userId}/activate`)
  },

  // Change your own password (requires current password)
  changePassword(currentPassword: string, newPassword: string): Promise<ApiResponse<void>> {
    return apiClient.put<void>('/users/me/password', {
      current_password: currentPassword,
      new_password: newPassword,
    })
  },
}

export default apiClient

// Admin API functions
export const adminApi = {
  // Get audit logs (admin only)
  getAuditLogs(
    page: number = 1,
    per_page: number = 50,
    event_type?: string
  ): Promise<ApiResponse<AuditLog[]>> {
    const params: any = { page, per_page }
    if (event_type) params.event_type = event_type

    return apiClient.get<AuditLog[]>('/admin/audit-logs', params).catch((error) => {
      console.error('Error fetching audit logs:', error)
      return { ...envelopeError(error, 'Failed to fetch audit logs'), data: [] }
    })
  },

  /** Wipe every MFA artifact for a user — lockout recovery (admin only). */
  resetUserMfa(userId: string): Promise<ApiResponse<{ user_id: string }>> {
    return apiClient.delete<{ user_id: string }>(`/admin/users/${userId}/mfa`)
  },
}

// Home links API (admin CRUD + public list)
export const homeLinksApi = {
  list() {
    return apiClient.get<import('@/types').HomeLink[]>('/admin/home-links')
  },
  get(id: string) {
    return apiClient.get<import('@/types').HomeLink>(`/admin/home-links/${id}`)
  },
  create(body: import('@/types').CreateHomeLinkRequest) {
    return apiClient.post<import('@/types').HomeLink>('/admin/home-links', body)
  },
  update(id: string, body: import('@/types').UpdateHomeLinkRequest) {
    return apiClient.patch<import('@/types').HomeLink>(`/admin/home-links/${id}`, body)
  },
  remove(id: string) {
    return apiClient.delete<void>(`/admin/home-links/${id}`)
  },
  /** No-auth list, audience-filtered server-side based on the caller's bearer (if any). */
  publicList() {
    return apiClient.get<import('@/types').HomeLink[]>('/public/home-links')
  },
}

// Schedules API
export const schedulesApi = {
  list() {
    return apiClient.get<import('@/types').Schedule[]>('/admin/schedules')
  },
  get(id: string) {
    return apiClient.get<import('@/types').Schedule>(`/admin/schedules/${id}`)
  },
  create(body: import('@/types').CreateScheduleRequest) {
    return apiClient.post<import('@/types').Schedule>('/admin/schedules', body)
  },
  update(id: string, body: import('@/types').UpdateScheduleRequest) {
    return apiClient.patch<import('@/types').Schedule>(`/admin/schedules/${id}`, body)
  },
  remove(id: string) {
    return apiClient.delete<void>(`/admin/schedules/${id}`)
  },
  /** No-auth list of schedules marked `is_public`; used by the home page. */
  publicList() {
    return apiClient.get<import('@/types').Schedule[]>('/public/schedules')
  },
}

// Places API
export const placesApi = {
  config() {
    return apiClient.get<import('@/types').PlaceConfig>('/admin/places/config')
  },
  list() {
    return apiClient.get<import('@/types').Place[]>('/admin/places')
  },
  get(id: string) {
    return apiClient.get<import('@/types').PlaceDetail>(`/admin/places/${id}`)
  },
  create(body: import('@/types').CreatePlaceRequest) {
    return apiClient.post<import('@/types').Place>('/admin/places', body)
  },
  update(id: string, body: import('@/types').UpdatePlaceRequest) {
    return apiClient.patch<import('@/types').Place>(`/admin/places/${id}`, body)
  },
  remove(id: string) {
    return apiClient.delete<void>(`/admin/places/${id}`)
  },
  // Member-facing
  memberList() {
    return apiClient.get<import('@/types').Place[]>('/places')
  },
}

// Wraps every method on an API object so a rejected request (network
// failure, or any non-2xx response, since apiClient/axios reject on
// those) resolves to the same ApiResponse<T> failure shape a handled
// API-level error already does, instead of throwing. Callers can then
// always just check `.success`/`.error` — no per-call-site try/catch
// needed, and no risk of a forgotten one leaving a loading/saving flag
// stuck or a failure passing with no user-facing feedback.
function withErrorGuard<T extends Record<string, (...args: any[]) => Promise<ApiResponse<any>>>>(
  api: T
): T {
  const guarded = {} as T
  // `Object.entries`, not `api[key]`: under `noUncheckedIndexedAccess` an
  // index read is `T[k] | undefined` and the call below is unprovable.
  for (const [key, fn] of Object.entries(api) as [keyof T, T[keyof T]][]) {
    guarded[key] = (async (...args: any[]) => {
      try {
        return await fn(...args)
      } catch (e: unknown) {
        // `envelopeError` with no fallback, deliberately.
        //
        // This guard's job is that the method never rejects. It is not in a
        // position to say what went wrong: it wraps every method on the
        // object, so the only message it could offer is a generic one, and a
        // generic message here *shadows* the specific one the call site
        // already has. Every failure path in DoorCheckinView and
        // DoorManagement ends in `r.error || '<what this call was doing>'`,
        // and none of them could fire while this filled `error` in first.
        //
        // The browser tier caught it twice: first as "Network Error" reaching
        // the user, then -- after that was fixed -- as "Door request failed"
        // where the door page's own "Failed to load door" belonged.
        return envelopeError(e)
      }
    }) as T[keyof T]
  }
  return guarded
}

// Doors API
export const doorsApi = withErrorGuard({
  // Member-facing
  info(doorId: string) {
    return apiClient.get<import('@/types').DoorInfo>(`/doors/${doorId}/info`)
  },
  checkin(doorId: string) {
    return apiClient.post<import('@/types').DoorCheckinResult>(`/doors/${doorId}/checkin`)
  },

  // Admin
  list() {
    return apiClient.get<import('@/types').Door[]>('/admin/doors')
  },
  get(doorId: string) {
    return apiClient.get<import('@/types').DoorDetail>(`/admin/doors/${doorId}`)
  },
  create(body: import('@/types').CreateDoorRequest) {
    return apiClient.post<import('@/types').Door>('/admin/doors', body)
  },
  update(doorId: string, body: import('@/types').UpdateDoorRequest) {
    return apiClient.patch<import('@/types').Door>(`/admin/doors/${doorId}`, body)
  },
  remove(doorId: string) {
    return apiClient.delete<void>(`/admin/doors/${doorId}`)
  },
  unlock(doorId: string) {
    return apiClient.post<{ unlocked: boolean }>(`/admin/doors/${doorId}/unlock`)
  },
  republish(doorId: string) {
    return apiClient.post<{ republished: boolean }>(`/admin/doors/${doorId}/republish`)
  },
  qrUrl(doorId: string) {
    return apiClient.get<{ url: string }>(`/admin/doors/${doorId}/qr`)
  },
  events(doorId: string, params?: { limit?: number; offset?: number }) {
    return apiClient.get<import('@/types').DoorAccessEvent[]>(
      `/admin/doors/${doorId}/events`,
      params
    )
  },
  listRules(doorId: string) {
    return apiClient.get<import('@/types').DoorAccessRule[]>(`/admin/doors/${doorId}/rules`)
  },
  addRule(doorId: string, body: import('@/types').AddDoorRuleRequest) {
    return apiClient.post<import('@/types').DoorAccessRule>(`/admin/doors/${doorId}/rules`, body)
  },
  removeRule(doorId: string, ruleId: string) {
    return apiClient.delete<void>(`/admin/doors/${doorId}/rules/${ruleId}`)
  },
})

// MFA API functions
export const mfaApi = {
  status() {
    return apiClient.get<import('@/types').MfaStatus>('/auth/mfa/status')
  },
  totpSetup() {
    return apiClient.post<import('@/types').MfaTotpSetup>('/auth/mfa/totp/setup')
  },
  totpConfirm(code: string) {
    return apiClient.post<import('@/types').MfaRecoveryCodes>('/auth/mfa/totp/confirm', { code })
  },
  totpDisable() {
    return apiClient.delete<void>('/auth/mfa/totp')
  },
  listWebauthn() {
    return apiClient.get<import('@/types').MfaWebauthnCredential[]>('/auth/mfa/webauthn')
  },
  webauthnRegisterBegin(label: string) {
    return apiClient.post<import('@/types').MfaWebauthnRegisterBegin>(
      '/auth/mfa/webauthn/register/begin',
      { label }
    )
  },
  webauthnRegisterFinish(challenge_token: string, response: unknown) {
    return apiClient.post<{ credential_id: string }>('/auth/mfa/webauthn/register/finish', {
      challenge_token,
      response,
    })
  },
  webauthnRemove(id: string) {
    return apiClient.delete<void>(`/auth/mfa/webauthn/${id}`)
  },
  regenerateRecoveryCodes() {
    return apiClient.post<import('@/types').MfaRecoveryCodes>('/auth/mfa/recovery-codes/regenerate')
  },
  verify(body: {
    challenge_token: string
    method: 'totp' | 'webauthn' | 'recovery'
    code?: string
    response?: unknown
  }) {
    return apiClient.post<import('@/types').LoginResponse>('/auth/mfa/verify', body)
  },
}

/**
 * Account recovery and email confirmation.
 *
 * Every call here is unauthenticated -- the token in the body is the
 * credential, and none of these ever carries a session. `mfaApi.verify` is the
 * existing precedent for that shape.
 *
 * Note what is deliberately absent: a `validate(token)` call. Answering "is
 * this token good?" without spending it would be a free oracle on a public
 * endpoint, and the only thing it buys is a slightly better message.
 */
export const accountApi = {
  requestPasswordReset(email: string) {
    return apiClient.post<void>('/auth/password-reset/request', { email })
  },
  consumePasswordReset(token: string, new_password: string) {
    return apiClient.post<void>('/auth/password-reset/consume', { token, new_password })
  },
  verifyEmail(token: string) {
    return apiClient.post<void>('/auth/email/verify', { token })
  },
  resendVerification(email: string) {
    return apiClient.post<void>('/auth/email/resend', { email })
  },
}

// Webhooks API functions (admin only)
export const webhooksApi = {
  // --- Auth headers (reusable write-only credentials) ---
  listAuthHeaders() {
    return apiClient.get<import('@/types').WebhookAuthHeader[]>('/admin/webhooks/auth-headers')
  },
  createAuthHeader(data: import('@/types').CreateAuthHeaderRequest) {
    return apiClient.post<import('@/types').WebhookAuthHeader>('/admin/webhooks/auth-headers', data)
  },
  updateAuthHeader(id: string, data: import('@/types').UpdateAuthHeaderRequest) {
    return apiClient.patch<import('@/types').WebhookAuthHeader>(
      `/admin/webhooks/auth-headers/${id}`,
      data
    )
  },
  deleteAuthHeader(id: string) {
    return apiClient.delete<void>(`/admin/webhooks/auth-headers/${id}`)
  },

  // --- Event type catalog ---
  listEventTypes() {
    return apiClient.get<import('@/types').WebhookEventType[]>('/admin/webhooks/event-types')
  },

  // --- Webhooks ---
  listWebhooks() {
    return apiClient.get<import('@/types').Webhook[]>('/admin/webhooks')
  },
  getWebhook(id: string) {
    return apiClient.get<import('@/types').Webhook>(`/admin/webhooks/${id}`)
  },
  createWebhook(data: import('@/types').CreateWebhookRequest) {
    return apiClient.post<import('@/types').Webhook>('/admin/webhooks', data)
  },
  updateWebhook(id: string, data: import('@/types').UpdateWebhookRequest) {
    return apiClient.patch<import('@/types').Webhook>(`/admin/webhooks/${id}`, data)
  },
  deleteWebhook(id: string) {
    return apiClient.delete<void>(`/admin/webhooks/${id}`)
  },
  testWebhook(id: string) {
    return apiClient.post<{ delivered: boolean; error?: string }>(`/admin/webhooks/${id}/test`)
  },

  // --- Deliveries ---
  listDeliveries(params?: { webhook_id?: string; limit?: number; offset?: number }) {
    return apiClient.get<import('@/types').WebhookDelivery[]>('/admin/webhooks/deliveries', params)
  },
}

// Tools API functions
export const toolsApi = {
  // List tools (staff only)
  getTools(query?: ToolQuery): Promise<ApiResponse<Tool[]>> {
    return apiClient.get<Tool[]>('/tools', query).catch((error) => {
      console.error('Error fetching tools:', error)
      return { ...envelopeError(error, 'Failed to fetch tools'), data: [] }
    })
  },

  // Get a specific tool (staff only)
  getTool(toolId: string): Promise<ApiResponse<Tool>> {
    return apiClient.get<Tool>(`/tools/${toolId}`).catch((error) => {
      console.error('Error fetching tool:', error)
      return envelopeError(error, 'Failed to fetch tool')
    })
  },

  // Create a new tool (staff only)
  createTool(toolData: CreateToolRequest): Promise<ApiResponse<Tool>> {
    return apiClient.post<Tool>('/tools', toolData).catch((error) => {
      console.error('Error creating tool:', error)
      return envelopeError(error, 'Failed to create tool')
    })
  },

  // Update a tool (staff only)
  updateTool(toolId: string, updates: UpdateToolRequest): Promise<ApiResponse<Tool>> {
    return apiClient.put<Tool>(`/tools/${toolId}`, updates).catch((error) => {
      console.error('Error updating tool:', error)
      return envelopeError(error, 'Failed to update tool')
    })
  },

  // Delete a tool (staff only)
  deleteTool(toolId: string): Promise<ApiResponse<void>> {
    return apiClient.delete<void>(`/tools/${toolId}`).catch((error) => {
      console.error('Error deleting tool:', error)
      return envelopeError(error, 'Failed to delete tool')
    })
  },

  // Change tool status (staff only)
  changeToolStatus(
    toolId: string,
    statusData: ChangeToolStatusRequest
  ): Promise<ApiResponse<Tool>> {
    return apiClient.put<Tool>(`/tools/${toolId}/status`, statusData).catch((error) => {
      console.error('Error changing tool status:', error)
      return envelopeError(error, 'Failed to change tool status')
    })
  },

  // Get tool events (staff only)
  getToolEvents(toolId: string): Promise<ApiResponse<ToolEvent[]>> {
    return apiClient.get<ToolEvent[]>(`/tools/${toolId}/events`).catch((error) => {
      console.error('Error fetching tool events:', error)
      return { ...envelopeError(error, 'Failed to fetch tool events'), data: [] }
    })
  },

  // Get available tools (members)
  getAvailableTools(): Promise<ApiResponse<Tool[]>> {
    return apiClient.get<Tool[]>('/tools/available').catch((error) => {
      console.error('Error fetching available tools:', error)
      return { ...envelopeError(error, 'Failed to fetch available tools'), data: [] }
    })
  },

  // Check if user can use a tool (members)
  canUseTool(toolId: string): Promise<ApiResponse<{ can_use: boolean; reason?: string }>> {
    return apiClient
      .get<{ can_use: boolean; reason?: string }>(`/tools/${toolId}/can-use`)
      .catch((error) => {
        console.error('Error checking tool usage:', error)
        return {
          ...envelopeError(error, 'Failed to check tool usage'),
          data: { can_use: false },
        }
      })
  },

  // Update tool status helper (shortcut method)
  //
  // `status` is a ToolStatus, not a string. It was typed as `string` and passed
  // straight into changeToolStatus, whose request type requires the enum -- so
  // any caller could send an arbitrary status the server would reject, and
  // nothing said so until the type-strictness ratchet included this file.
  updateToolStatus(toolId: string, status: ToolStatus, notes?: string): Promise<ApiResponse<Tool>> {
    return this.changeToolStatus(toolId, { status, notes })
  },

  // Get tool training steps (if available)
  getToolTrainingSteps(toolId: string): Promise<ApiResponse<any[]>> {
    return apiClient.get<any[]>(`/training/tools/${toolId}/steps`).catch((error) => {
      // The error is logged rather than discarded. This previously swallowed
      // it entirely and reported success with an empty list, so a 500 or a
      // dropped connection was indistinguishable from "this tool has no
      // training steps" -- on a system that gates machine access on training,
      // that is a failure that reads as an answer.
      //
      // What this does NOT do is distinguish the two: the empty-list fallback
      // is kept because callers render it directly and changing that is a UI
      // decision. A caller that needs to tell them apart cannot, yet.
      console.warn(`Could not load training steps for tool ${toolId}:`, error)
      return { success: true, data: [] }
    })
  },
}

// Training API functions
export const trainingApi = {
  // === Training Steps ===

  // Get training steps
  getTrainingSteps(query?: TrainingQuery): Promise<ApiResponse<TrainingStep[]>> {
    return apiClient.get<TrainingStep[]>('/training/steps', query).catch((error) => {
      console.error('Error fetching training steps:', error)
      return { ...envelopeError(error, 'Failed to fetch training steps'), data: [] }
    })
  },

  // Get training step by ID
  getTrainingStep(stepId: string): Promise<ApiResponse<TrainingStep>> {
    return apiClient.get<TrainingStep>(`/training/steps/${stepId}`).catch((error) => {
      console.error('Error fetching training step:', error)
      return envelopeError(error, 'Failed to fetch training step')
    })
  },

  // Create training step (staff only)
  createTrainingStep(stepData: CreateTrainingStepRequest): Promise<ApiResponse<TrainingStep>> {
    return apiClient.post<TrainingStep>('/training/steps', stepData).catch((error) => {
      console.error('Error creating training step:', error)
      return envelopeError(error, 'Failed to create training step')
    })
  },

  // Update training step (staff only)
  updateTrainingStep(
    stepId: string,
    updates: UpdateTrainingStepRequest
  ): Promise<ApiResponse<TrainingStep>> {
    return apiClient.put<TrainingStep>(`/training/steps/${stepId}`, updates).catch((error) => {
      console.error('Error updating training step:', error)
      return envelopeError(error, 'Failed to update training step')
    })
  },

  // Delete training step (staff only)
  deleteTrainingStep(stepId: string): Promise<ApiResponse<void>> {
    return apiClient.delete<void>(`/training/steps/${stepId}`).catch((error) => {
      console.error('Error deleting training step:', error)
      return envelopeError(error, 'Failed to delete training step')
    })
  },

  // Update training step position/order (staff only)
  updateTrainingStepPosition(stepId: string, newPosition: number): Promise<ApiResponse<void>> {
    return apiClient
      .put<void>(`/training/steps/${stepId}/position`, { step_number: newPosition })
      .catch((error) => {
        console.error('Error updating training step position:', error)
        return envelopeError(error, 'Failed to update training step position')
      })
  },

  // === Prerequisites ===

  // Get training prerequisites
  getTrainingPrerequisites(stepId: string): Promise<ApiResponse<TrainingStep[]>> {
    return apiClient
      .get<TrainingStep[]>(`/training/steps/${stepId}/prerequisites`)
      .catch((error) => {
        console.error('Error fetching training prerequisites:', error)
        return {
          ...envelopeError(error, 'Failed to fetch training prerequisites'),
          data: [],
        }
      })
  },

  // Add training prerequisite (staff only)
  addTrainingPrerequisite(
    data: CreateTrainingPrerequisiteRequest
  ): Promise<ApiResponse<TrainingPrerequisite>> {
    // The route and the body both come from the server, not from a guess.
    // `api/training.rs:130` declares
    // `POST /training/steps/{step_id}/prerequisites` taking a bare `Json<Uuid>`
    // -- this used to post an object to `/training/prerequisites`, which is not
    // a route at all, so adding a prerequisite could not work from anywhere in
    // the UI.
    return apiClient
      .post<TrainingPrerequisite>(
        `/training/steps/${data.training_step_id}/prerequisites`,
        data.prerequisite_step_id
      )
      .catch((error) => {
        console.error('Error adding training prerequisite:', error)
        return envelopeError(error, 'Failed to add training prerequisite')
      })
  },

  // Remove training prerequisite (staff only)
  removeTrainingPrerequisite(prerequisiteId: string): Promise<ApiResponse<void>> {
    return apiClient.delete<void>(`/training/prerequisites/${prerequisiteId}`).catch((error) => {
      console.error('Error removing training prerequisite:', error)
      return envelopeError(error, 'Failed to remove training prerequisite')
    })
  },

  // === User Progress ===

  // Get user training progress
  getUserTrainingProgress(
    userId: string,
    query?: TrainingQuery
  ): Promise<ApiResponse<UserTrainingProgress[]>> {
    return apiClient
      .get<UserTrainingProgress[]>(`/training/progress/${userId}`, query)
      .catch((error) => {
        console.error('Error fetching user training progress:', error)
        return {
          ...envelopeError(error, 'Failed to fetch user training progress'),
          data: [],
        }
      })
  },

  // Start training session
  startTrainingSession(
    userId: string,
    data: StartTrainingRequest
  ): Promise<ApiResponse<UserTrainingProgress>> {
    return apiClient
      .post<UserTrainingProgress>(`/training/progress/${userId}/start`, data)
      .catch((error) => {
        console.error('Error starting training session:', error)
        return envelopeError(error, 'Failed to start training session')
      })
  },

  // Complete training session (instructor only)
  completeTrainingSession(
    userId: string,
    data: CompleteTrainingRequest
  ): Promise<ApiResponse<UserTrainingProgress>> {
    return apiClient
      .post<UserTrainingProgress>(`/training/progress/${userId}/complete`, data)
      .catch((error) => {
        console.error('Error completing training session:', error)
        return envelopeError(error, 'Failed to complete training session')
      })
  },

  // === Tool Training Overview ===

  // Get tool training overview for user
  getToolTrainingOverview(
    toolId: string,
    userId?: string
  ): Promise<ApiResponse<ToolTrainingOverview>> {
    const url = userId
      ? `/training/tools/${toolId}/overview/${userId}`
      : `/training/tools/${toolId}/overview/me`
    return apiClient.get<ToolTrainingOverview>(url).catch((error) => {
      console.error('Error fetching tool training overview:', error)
      return envelopeError(error, 'Failed to fetch tool training overview')
    })
  },

  // Check if user can access tool
  canAccessTool(toolId: string, userId?: string): Promise<ApiResponse<boolean>> {
    // dev's route, which is the one the server declares
    // (api/training.rs:178); ours addressed `/tools/{id}/can-access/{user}`,
    // which is not a route. Our error extraction, because `error.message` is
    // the axios status restated and discards the server's own words.
    const url = userId ? `/training/access/${toolId}/${userId}` : `/training/access/${toolId}`
    return apiClient.get<boolean>(url).catch((error) => {
      console.error('Error checking tool access:', error)
      return { ...envelopeError(error, 'Failed to check tool access'), data: false }
    })
  },

  // === Instructors ===

  // Get training instructors
  getTrainingInstructors(query?: TrainingQuery): Promise<ApiResponse<TrainingInstructor[]>> {
    return apiClient.get<TrainingInstructor[]>('/training/instructors', query).catch((error) => {
      console.error('Error fetching training instructors:', error)
      return {
        ...envelopeError(error, 'Failed to fetch training instructors'),
        data: [],
      }
    })
  },

  // Certify instructor (admin only)
  certifyInstructor(data: CertifyInstructorRequest): Promise<ApiResponse<TrainingInstructor>> {
    return apiClient.post<TrainingInstructor>('/training/instructors', data).catch((error) => {
      console.error('Error certifying instructor:', error)
      return envelopeError(error, 'Failed to certify instructor')
    })
  },

  // Revoke instructor certification (admin only)
  revokeInstructorCertification(instructorId: string): Promise<ApiResponse<void>> {
    return apiClient.delete<void>(`/training/instructors/${instructorId}`).catch((error) => {
      console.error('Error revoking instructor certification:', error)
      return envelopeError(error, 'Failed to revoke instructor certification')
    })
  },
}

// Trainer assignment API
export const trainerApi = {
  // === Tool Trainer Management ===

  // Assign a trainer to a tool (staff only)
  assignToolTrainer(data: AssignTrainerRequest): Promise<ApiResponse<ToolTrainer>> {
    return apiClient
      .post<ToolTrainer>(`/trainers/tools/${data.tool_id}/trainers`, data)
      .catch((error) => {
        console.error('Error assigning tool trainer:', error)
        return envelopeError(error, 'Failed to assign tool trainer')
      })
  },

  // Get trainers for a tool
  getToolTrainers(
    toolId: string,
    includeInactive: boolean = false
  ): Promise<ApiResponse<ToolTrainerWithUser[]>> {
    return apiClient
      .get<ToolTrainerWithUser[]>(`/trainers/tools/${toolId}/trainers`, {
        include_inactive: includeInactive,
      })
      .catch((error) => {
        console.error('Error fetching tool trainers:', error)
        return { ...envelopeError(error, 'Failed to fetch tool trainers'), data: [] }
      })
  },

  // Update trainer assignment (staff only)
  updateToolTrainer(
    toolId: string,
    userId: string,
    data: UpdateTrainerRequest
  ): Promise<ApiResponse<ToolTrainer>> {
    return apiClient
      .put<ToolTrainer>(`/trainers/tools/${toolId}/trainers/${userId}`, data)
      .catch((error) => {
        console.error('Error updating tool trainer:', error)
        return envelopeError(error, 'Failed to update tool trainer')
      })
  },

  // Remove trainer from tool (staff only)
  removeToolTrainer(toolId: string, userId: string): Promise<ApiResponse<void>> {
    return apiClient.delete<void>(`/trainers/tools/${toolId}/trainers/${userId}`).catch((error) => {
      console.error('Error removing tool trainer:', error)
      return envelopeError(error, 'Failed to remove tool trainer')
    })
  },

  // Check if user is authorized trainer for tool
  checkTrainerAuthorization(toolId: string, userId: string): Promise<ApiResponse<boolean>> {
    return apiClient
      .get<boolean>(`/trainers/tools/${toolId}/trainers/check/${userId}`)
      .catch((error) => {
        // Don't log error as this might be expected for non-trainers
        console.debug('Trainer authorization check result:', error.response?.status)
        if (error.response?.status === 401 || error.response?.status === 403) {
          // User is not authorized as trainer, return false instead of error
          return { success: true, data: false }
        }
        console.error('Error checking trainer authorization:', error)
        return {
          ...envelopeError(error, 'Failed to check trainer authorization'),
          data: false,
        }
      })
  },

  // === Training Records ===

  // Create training record (trainers only)
  createTrainingRecord(data: CreateTrainingRecordRequest): Promise<ApiResponse<TrainingRecord>> {
    return apiClient.post<TrainingRecord>('/trainers/training-records', data).catch((error) => {
      console.error('Error creating training record:', error)
      return envelopeError(error, 'Failed to create training record')
    })
  },

  // Get training records with filters
  getTrainingRecords(
    query?: TrainingRecordsQuery
  ): Promise<ApiResponse<TrainingRecordWithUsers[]>> {
    return apiClient
      .get<TrainingRecordWithUsers[]>('/trainers/training-records', query)
      .catch((error) => {
        console.error('Error fetching training records:', error)
        return {
          ...envelopeError(error, 'Failed to fetch training records'),
          data: [],
        }
      })
  },

  // Update training record (trainers and staff)
  updateTrainingRecord(
    recordId: string,
    data: UpdateTrainingRecordRequest
  ): Promise<ApiResponse<TrainingRecord>> {
    return apiClient
      .put<TrainingRecord>(`/trainers/training-records/${recordId}`, data)
      .catch((error) => {
        console.error('Error updating training record:', error)
        return envelopeError(error, 'Failed to update training record')
      })
  },

  // Get training records for a user
  getUserTrainingRecords(
    userId: string,
    asTrainer: boolean = false
  ): Promise<ApiResponse<TrainingRecordWithUsers[]>> {
    return apiClient
      .get<TrainingRecordWithUsers[]>(`/trainers/users/${userId}/training-records`, {
        as_trainer: asTrainer,
      })
      .catch((error) => {
        console.error('Error fetching user training records:', error)
        return {
          ...envelopeError(error, 'Failed to fetch user training records'),
          data: [],
        }
      })
  },
}

// cmi5 training modules. JSON calls go through the typed apiClient; the two
// binary calls (multipart import, zip export) go through apiClient.raw, which
// still carries the Bearer token but lets axios set the right Content-Type /
// responseType.
export const cmi5Api = {
  listCourses() {
    return apiClient.get<import('@/types').Cmi5Course[]>('/cmi5/courses')
  },
  getCourse(id: string) {
    return apiClient.get<import('@/types').Cmi5CourseWithAus>(`/cmi5/courses/${id}`)
  },
  deleteCourse(id: string) {
    return apiClient.delete<{ deleted: string }>(`/cmi5/courses/${id}`)
  },
  assignAu(courseId: string, auId: string, body: import('@/types').Cmi5AssignRequest) {
    return apiClient.post<import('@/types').Cmi5AssignableUnit>(
      `/cmi5/courses/${courseId}/aus/${auId}/assign`,
      body,
    )
  },
  async importCourse(
    file: File,
  ): Promise<ApiResponse<import('@/types').Cmi5CourseWithAus>> {
    const form = new FormData()
    form.append('file', file)
    try {
      const res = await apiClient.raw.post('/cmi5/courses', form)
      return res.data as ApiResponse<import('@/types').Cmi5CourseWithAus>
    } catch (error) {
      return envelopeError<import('@/types').Cmi5CourseWithAus>(error, 'Failed to import package')
    }
  },
  async exportCourse(id: string): Promise<Blob> {
    const res = await apiClient.raw.get(`/cmi5/courses/${id}/export`, {
      responseType: 'blob',
    })
    return res.data as Blob
  },
  listMyModules() {
    return apiClient.get<import('@/types').Cmi5LearnerModule[]>('/cmi5/modules')
  },
  launch(auId: string) {
    return apiClient.post<import('@/types').Cmi5LaunchResponse>(`/cmi5/aus/${auId}/launch`)
  },
}
