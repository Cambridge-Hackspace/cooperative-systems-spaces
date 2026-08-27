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
  TrainingRecordsQuery
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
  (error) => {
    return Promise.reject(error)
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
  updateUserProfile(userId: string, data: UpdateProfileRequest): Promise<ApiResponse<ProfileResponse>> {
    return apiClient.put(`/profiles/${userId}`, data)
  },

  // Get profile configuration (admin only)
  getProfileConfig(): Promise<ApiResponse<ProfileConfigResponse>> {
    return apiClient.get('/profiles/config')
  },

  // Update profile configuration (admin only)
  updateProfileConfig(data: UpdateProfileConfigRequest): Promise<ApiResponse<ProfileConfigResponse>> {
    return apiClient.put('/profiles/config', data)
  },
}

// User/Roster API functions  
export const userApi = {
  // Get all users for roster (admin only)
  getAllUsers(): Promise<ApiResponse<any>> {
    // Use the admin roster endpoint instead
    return apiClient.get('/admin/roster')
      .then(response => {
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
              total_pages: 1
            }
          }
        }
        return response
      })
      .catch(error => {
        console.error('Error fetching roster:', error)
        return { success: false, error: error.message || 'Failed to fetch users' }
      })
  },

  // Get users for training purposes (trainers can access)
  getUsersForTraining(toolId?: string): Promise<ApiResponse<any>> {
    // Use the new training-specific roster endpoint that allows trainers to see all users
    const endpoint = toolId ? `/training/roster/${toolId}` : '/training/roster'
    
    return apiClient.get(endpoint)
      .then(response => {
        if (response.success && response.data) {
          const users = Array.isArray(response.data) ? response.data : ((response.data as any).items || [])
          return {
            success: true,
            data: {
              items: users,
              page: 1,
              per_page: users.length,
              total: users.length,
              total_pages: 1
            }
          }
        }
        return response
      })
      .catch(error => {
        // If training roster endpoint fails, try the old trainer-specific endpoint
        if (error.response?.status === 404) {
          console.log('Training roster endpoint not available, trying legacy trainer endpoint')
          return apiClient.get('/trainers/users')
            .then(response => {
              if (response.success && response.data) {
                const users = Array.isArray(response.data) ? response.data : ((response.data as any).items || [])
                return {
                  success: true,
                  data: {
                    items: users,
                    page: 1,
                    per_page: users.length,
                    total: users.length,
                    total_pages: 1
                  }
                }
              }
              return response
            })
            .catch(legacyError => {
              // If both training roster and legacy trainer endpoints fail, try admin roster for admins
              if (legacyError.response?.status === 401 || legacyError.response?.status === 403) {
                console.log('Trainer endpoints not accessible, trying admin roster')
                return this.getAllUsers()
              }
              throw legacyError
            })
        }
        
        console.error('Error fetching users for training:', error)
        return { success: false, error: error.message || 'Failed to fetch users for training' }
      })
  },

  // Get training history for a specific tool (trainers and staff can access)
  getTrainingHistory(toolId: string, queryParams?: any): Promise<ApiResponse<any>> {
    return apiClient.get(`/training/history/${toolId}`, queryParams)
      .then(response => {
        if (response.success && response.data) {
          const records = Array.isArray(response.data) ? response.data : []
          return {
            success: true,
            data: records
          }
        }
        return response
      })
      .catch(error => {
        console.error('Error fetching training history:', error)
        return { success: false, error: error.message || 'Failed to fetch training history' }
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
}

export default apiClient

// Admin API functions
export const adminApi = {
  // Get audit logs (admin only)
  getAuditLogs(page: number = 1, per_page: number = 50, event_type?: string): Promise<ApiResponse<AuditLog[]>> {
    const params: any = { page, per_page }
    if (event_type) params.event_type = event_type

    return apiClient.get<AuditLog[]>('/admin/audit-logs', params)
      .catch(error => {
        console.error('Error fetching audit logs:', error)
        return { success: false, error: error.message || 'Failed to fetch audit logs', data: [] }
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
  api: T,
  fallbackMessage: string
): T {
  const guarded = {} as T
  for (const key of Object.keys(api) as (keyof T)[]) {
    const fn = api[key]
    guarded[key] = (async (...args: any[]) => {
      try {
        return await fn(...args)
      } catch (e: any) {
        return { success: false, error: e?.response?.data?.error || fallbackMessage }
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
    return apiClient.get<import('@/types').DoorAccessEvent[]>(`/admin/doors/${doorId}/events`, params)
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
}, 'Door request failed')

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
    return apiClient.post<import('@/types').MfaWebauthnRegisterBegin>('/auth/mfa/webauthn/register/begin', { label })
  },
  webauthnRegisterFinish(challenge_token: string, response: unknown) {
    return apiClient.post<{ credential_id: string }>('/auth/mfa/webauthn/register/finish', { challenge_token, response })
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
    return apiClient.patch<import('@/types').WebhookAuthHeader>(`/admin/webhooks/auth-headers/${id}`, data)
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
    return apiClient.get<Tool[]>('/tools', query)
      .catch(error => {
        console.error('Error fetching tools:', error)
        return { success: false, error: error.message || 'Failed to fetch tools', data: [] }
      })
  },

  // Get a specific tool (staff only)
  getTool(toolId: string): Promise<ApiResponse<Tool>> {
    return apiClient.get<Tool>(`/tools/${toolId}`)
      .catch(error => {
        console.error('Error fetching tool:', error)
        return { success: false, error: error.message || 'Failed to fetch tool' }
      })
  },

  // Create a new tool (staff only)
  createTool(toolData: CreateToolRequest): Promise<ApiResponse<Tool>> {
    return apiClient.post<Tool>('/tools', toolData)
      .catch(error => {
        console.error('Error creating tool:', error)
        return { success: false, error: error.message || 'Failed to create tool' }
      })
  },

  // Update a tool (staff only)
  updateTool(toolId: string, updates: UpdateToolRequest): Promise<ApiResponse<Tool>> {
    return apiClient.put<Tool>(`/tools/${toolId}`, updates)
      .catch(error => {
        console.error('Error updating tool:', error)
        return { success: false, error: error.message || 'Failed to update tool' }
      })
  },

  // Delete a tool (staff only)
  deleteTool(toolId: string): Promise<ApiResponse<void>> {
    return apiClient.delete<void>(`/tools/${toolId}`)
      .catch(error => {
        console.error('Error deleting tool:', error)
        return { success: false, error: error.message || 'Failed to delete tool' }
      })
  },

  // Change tool status (staff only)
  changeToolStatus(toolId: string, statusData: ChangeToolStatusRequest): Promise<ApiResponse<Tool>> {
    return apiClient.put<Tool>(`/tools/${toolId}/status`, statusData)
      .catch(error => {
        console.error('Error changing tool status:', error)
        return { success: false, error: error.message || 'Failed to change tool status' }
      })
  },

  // Get tool events (staff only)
  getToolEvents(toolId: string): Promise<ApiResponse<ToolEvent[]>> {
    return apiClient.get<ToolEvent[]>(`/tools/${toolId}/events`)
      .catch(error => {
        console.error('Error fetching tool events:', error)
        return { success: false, error: error.message || 'Failed to fetch tool events', data: [] }
      })
  },

  // Get available tools (members)
  getAvailableTools(): Promise<ApiResponse<Tool[]>> {
    return apiClient.get<Tool[]>('/tools/available')
      .catch(error => {
        console.error('Error fetching available tools:', error)
        return { success: false, error: error.message || 'Failed to fetch available tools', data: [] }
      })
  },

  // Check if user can use a tool (members)
  canUseTool(toolId: string): Promise<ApiResponse<{ can_use: boolean; reason?: string }>> {
    return apiClient.get<{ can_use: boolean; reason?: string }>(`/tools/${toolId}/can-use`)
      .catch(error => {
        console.error('Error checking tool usage:', error)
        return { success: false, error: error.message || 'Failed to check tool usage', data: { can_use: false } }
      })
  },

  // Update tool status helper (shortcut method)
  updateToolStatus(toolId: string, status: string, notes?: string): Promise<ApiResponse<Tool>> {
    return this.changeToolStatus(toolId, { status, notes })
  },

  // Get tool training steps (if available)
  getToolTrainingSteps(toolId: string): Promise<ApiResponse<any[]>> {
    return apiClient.get<any[]>(`/training/tools/${toolId}/steps`)
      .catch(error => {
        console.debug('No training steps for tool:', toolId)
        return { success: true, data: [] } // Return empty array if no training
      })
  },
}

// Training API functions
export const trainingApi = {
  // === Training Steps ===
  
  // Get training steps
  getTrainingSteps(query?: TrainingQuery): Promise<ApiResponse<TrainingStep[]>> {
    return apiClient.get<TrainingStep[]>('/training/steps', query)
      .catch(error => {
        console.error('Error fetching training steps:', error)
        return { success: false, error: error.message || 'Failed to fetch training steps', data: [] }
      })
  },

  // Get training step by ID
  getTrainingStep(stepId: string): Promise<ApiResponse<TrainingStep>> {
    return apiClient.get<TrainingStep>(`/training/steps/${stepId}`)
      .catch(error => {
        console.error('Error fetching training step:', error)
        return { success: false, error: error.message || 'Failed to fetch training step' }
      })
  },

  // Create training step (staff only)
  createTrainingStep(stepData: CreateTrainingStepRequest): Promise<ApiResponse<TrainingStep>> {
    return apiClient.post<TrainingStep>('/training/steps', stepData)
      .catch(error => {
        console.error('Error creating training step:', error)
        return { success: false, error: error.message || 'Failed to create training step' }
      })
  },

  // Update training step (staff only)
  updateTrainingStep(stepId: string, updates: UpdateTrainingStepRequest): Promise<ApiResponse<TrainingStep>> {
    return apiClient.put<TrainingStep>(`/training/steps/${stepId}`, updates)
      .catch(error => {
        console.error('Error updating training step:', error)
        return { success: false, error: error.message || 'Failed to update training step' }
      })
  },

  // Delete training step (staff only)
  deleteTrainingStep(stepId: string): Promise<ApiResponse<void>> {
    return apiClient.delete<void>(`/training/steps/${stepId}`)
      .catch(error => {
        console.error('Error deleting training step:', error)
        return { success: false, error: error.message || 'Failed to delete training step' }
      })
  },

  // Update training step position/order (staff only)
  updateTrainingStepPosition(stepId: string, newPosition: number): Promise<ApiResponse<void>> {
    return apiClient.put<void>(`/training/steps/${stepId}/position`, { step_number: newPosition })
      .catch(error => {
        console.error('Error updating training step position:', error)
        return { success: false, error: error.message || 'Failed to update training step position' }
      })
  },

  // === Prerequisites ===

  // Get training prerequisites
  getTrainingPrerequisites(stepId: string): Promise<ApiResponse<TrainingStep[]>> {
    return apiClient.get<TrainingStep[]>(`/training/steps/${stepId}/prerequisites`)
      .catch(error => {
        console.error('Error fetching training prerequisites:', error)
        return { success: false, error: error.message || 'Failed to fetch training prerequisites', data: [] }
      })
  },

  // Add training prerequisite (staff only)
  addTrainingPrerequisite(data: CreateTrainingPrerequisiteRequest): Promise<ApiResponse<TrainingPrerequisite>> {
    return apiClient.post<TrainingPrerequisite>('/training/prerequisites', data)
      .catch(error => {
        console.error('Error adding training prerequisite:', error)
        return { success: false, error: error.message || 'Failed to add training prerequisite' }
      })
  },

  // Remove training prerequisite (staff only)
  removeTrainingPrerequisite(prerequisiteId: string): Promise<ApiResponse<void>> {
    return apiClient.delete<void>(`/training/prerequisites/${prerequisiteId}`)
      .catch(error => {
        console.error('Error removing training prerequisite:', error)
        return { success: false, error: error.message || 'Failed to remove training prerequisite' }
      })
  },

  // === User Progress ===

  // Get user training progress
  getUserTrainingProgress(userId: string, query?: TrainingQuery): Promise<ApiResponse<UserTrainingProgress[]>> {
    return apiClient.get<UserTrainingProgress[]>(`/training/progress/${userId}`, query)
      .catch(error => {
        console.error('Error fetching user training progress:', error)
        return { success: false, error: error.message || 'Failed to fetch user training progress', data: [] }
      })
  },

  // Start training session
  startTrainingSession(userId: string, data: StartTrainingRequest): Promise<ApiResponse<UserTrainingProgress>> {
    return apiClient.post<UserTrainingProgress>(`/training/progress/${userId}/start`, data)
      .catch(error => {
        console.error('Error starting training session:', error)
        return { success: false, error: error.message || 'Failed to start training session' }
      })
  },

  // Complete training session (instructor only)
  completeTrainingSession(userId: string, data: CompleteTrainingRequest): Promise<ApiResponse<UserTrainingProgress>> {
    return apiClient.post<UserTrainingProgress>(`/training/progress/${userId}/complete`, data)
      .catch(error => {
        console.error('Error completing training session:', error)
        return { success: false, error: error.message || 'Failed to complete training session' }
      })
  },

  // === Tool Training Overview ===

  // Get tool training overview for user
  getToolTrainingOverview(toolId: string, userId?: string): Promise<ApiResponse<ToolTrainingOverview>> {
    const url = userId ? `/training/tools/${toolId}/overview/${userId}` : `/training/tools/${toolId}/overview/me`
    return apiClient.get<ToolTrainingOverview>(url)
      .catch(error => {
        console.error('Error fetching tool training overview:', error)
        return { success: false, error: error.message || 'Failed to fetch tool training overview' }
      })
  },

  // Check if user can access tool
  canAccessTool(toolId: string, userId?: string): Promise<ApiResponse<boolean>> {
    const url = userId ? `/training/access/${toolId}/${userId}` : `/training/access/${toolId}`
    return apiClient.get<boolean>(url)
      .catch(error => {
        console.error('Error checking tool access:', error)
        return { success: false, error: error.message || 'Failed to check tool access', data: false }
      })
  },

  // === Instructors ===

  // Get training instructors
  getTrainingInstructors(query?: TrainingQuery): Promise<ApiResponse<TrainingInstructor[]>> {
    return apiClient.get<TrainingInstructor[]>('/training/instructors', query)
      .catch(error => {
        console.error('Error fetching training instructors:', error)
        return { success: false, error: error.message || 'Failed to fetch training instructors', data: [] }
      })
  },

  // Certify instructor (admin only)
  certifyInstructor(data: CertifyInstructorRequest): Promise<ApiResponse<TrainingInstructor>> {
    return apiClient.post<TrainingInstructor>('/training/instructors', data)
      .catch(error => {
        console.error('Error certifying instructor:', error)
        return { success: false, error: error.message || 'Failed to certify instructor' }
      })
  },

  // Revoke instructor certification (admin only)
  revokeInstructorCertification(instructorId: string): Promise<ApiResponse<void>> {
    return apiClient.delete<void>(`/training/instructors/${instructorId}`)
      .catch(error => {
        console.error('Error revoking instructor certification:', error)
        return { success: false, error: error.message || 'Failed to revoke instructor certification' }
      })
  },
}

// Trainer assignment API
export const trainerApi = {
  // === Tool Trainer Management ===

  // Assign a trainer to a tool (staff only)
  assignToolTrainer(data: AssignTrainerRequest): Promise<ApiResponse<ToolTrainer>> {
    return apiClient.post<ToolTrainer>(`/trainers/tools/${data.tool_id}/trainers`, data)
      .catch(error => {
        console.error('Error assigning tool trainer:', error)
        return { success: false, error: error.message || 'Failed to assign tool trainer' }
      })
  },

  // Get trainers for a tool
  getToolTrainers(toolId: string, includeInactive: boolean = false): Promise<ApiResponse<ToolTrainerWithUser[]>> {
    return apiClient.get<ToolTrainerWithUser[]>(`/trainers/tools/${toolId}/trainers`, { include_inactive: includeInactive })
      .catch(error => {
        console.error('Error fetching tool trainers:', error)
        return { success: false, error: error.message || 'Failed to fetch tool trainers', data: [] }
      })
  },

  // Update trainer assignment (staff only)
  updateToolTrainer(toolId: string, userId: string, data: UpdateTrainerRequest): Promise<ApiResponse<ToolTrainer>> {
    return apiClient.put<ToolTrainer>(`/trainers/tools/${toolId}/trainers/${userId}`, data)
      .catch(error => {
        console.error('Error updating tool trainer:', error)
        return { success: false, error: error.message || 'Failed to update tool trainer' }
      })
  },

  // Remove trainer from tool (staff only)
  removeToolTrainer(toolId: string, userId: string): Promise<ApiResponse<void>> {
    return apiClient.delete<void>(`/trainers/tools/${toolId}/trainers/${userId}`)
      .catch(error => {
        console.error('Error removing tool trainer:', error)
        return { success: false, error: error.message || 'Failed to remove tool trainer' }
      })
  },

  // Check if user is authorized trainer for tool
  checkTrainerAuthorization(toolId: string, userId: string): Promise<ApiResponse<boolean>> {
    return apiClient.get<boolean>(`/trainers/tools/${toolId}/trainers/check/${userId}`)
      .catch(error => {
        // Don't log error as this might be expected for non-trainers
        console.debug('Trainer authorization check result:', error.response?.status)
        if (error.response?.status === 401 || error.response?.status === 403) {
          // User is not authorized as trainer, return false instead of error
          return { success: true, data: false }
        }
        console.error('Error checking trainer authorization:', error)
        return { success: false, error: error.message || 'Failed to check trainer authorization', data: false }
      })
  },

  // === Training Records ===

  // Create training record (trainers only)
  createTrainingRecord(data: CreateTrainingRecordRequest): Promise<ApiResponse<TrainingRecord>> {
    return apiClient.post<TrainingRecord>('/trainers/training-records', data)
      .catch(error => {
        console.error('Error creating training record:', error)
        return { success: false, error: error.message || 'Failed to create training record' }
      })
  },

  // Get training records with filters
  getTrainingRecords(query?: TrainingRecordsQuery): Promise<ApiResponse<TrainingRecordWithUsers[]>> {
    return apiClient.get<TrainingRecordWithUsers[]>('/trainers/training-records', query)
      .catch(error => {
        console.error('Error fetching training records:', error)
        return { success: false, error: error.message || 'Failed to fetch training records', data: [] }
      })
  },

  // Update training record (trainers and staff)
  updateTrainingRecord(recordId: string, data: UpdateTrainingRecordRequest): Promise<ApiResponse<TrainingRecord>> {
    return apiClient.put<TrainingRecord>(`/trainers/training-records/${recordId}`, data)
      .catch(error => {
        console.error('Error updating training record:', error)
        return { success: false, error: error.message || 'Failed to update training record' }
      })
  },

  // Get training records for a user
  getUserTrainingRecords(userId: string, asTrainer: boolean = false): Promise<ApiResponse<TrainingRecordWithUsers[]>> {
    return apiClient.get<TrainingRecordWithUsers[]>(`/trainers/users/${userId}/training-records`, { as_trainer: asTrainer })
      .catch(error => {
        console.error('Error fetching user training records:', error)
        return { success: false, error: error.message || 'Failed to fetch user training records', data: [] }
      })
  },
}
