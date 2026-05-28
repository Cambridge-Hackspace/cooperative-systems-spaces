// Tool and training types for the frontend
export * from './tools';
export * from './training';

// API Response types
export interface ApiResponse<T> {
  success: boolean
  data?: T
  message?: string
  error?: string
}

export interface PaginatedResponse<T> {
  items: T[]
  page: number
  per_page: number
  total: number
  total_pages: number
}

// User types
export enum UserRole {
  Unknown = 'Unknown',
  Newbie = 'Newbie',
  Member = 'Member',
  Staff = 'Staff',
  Admin = 'Admin'
}

export interface User {
  id: string
  username: string
  email: string
  full_name: string
  is_active: boolean
  role: UserRole
  created_at: string
  updated_at: string
  /** Set when the user has at least one confirmed MFA method. */
  mfa_enrolled_at?: string | null
  profile: Record<string, any>
  meta: Record<string, any>
}

// Profile types
export enum ProfileFieldType {
  Text = 'Text',
  /** Ordered list of free-form strings, edited as chips. */
  TextArray = 'TextArray',
  Email = 'Email',
  Phone = 'Phone',
  Number = 'Number',
  Date = 'Date',
  Boolean = 'Boolean',
  Select = 'Select'
}

export interface ProfileFieldSelectOptions {
  options: string[]
}

export interface ProfileField {
  key: string
  label: string
  field_type: ProfileFieldType | { Select: ProfileFieldSelectOptions }
  required: boolean
  help_text?: string
}

export interface UserConfig {
  profile_fields: ProfileField[]
  profiles_enabled: boolean
}

export interface ProfileResponse {
  user_id: string
  profile: Record<string, any>
}

export interface UpdateProfileRequest {
  profile: Record<string, any>
}

export interface ProfileConfigResponse {
  profile_fields: ProfileField[]
  profiles_enabled: boolean
}

export interface UpdateProfileConfigRequest {
  profile_fields: ProfileField[]
  profiles_enabled: boolean
}

// Auth types
export interface LoginRequest {
  username_or_email: string
  password: string
}

export interface LoginResponse {
  token: string
  user: User
  expires_in: number
  /** Set by the server when MFA enforcement requires this user to enroll. */
  must_enroll_mfa?: boolean
}

/** Returned by `/auth/login` when the user has MFA enrolled. */
export interface MfaChallenge {
  mfa_required: true
  challenge_token: string
  methods: Array<'totp' | 'webauthn' | 'recovery'>
  /** A serialized PublicKeyCredentialRequestOptionsJSON for navigator.credentials.get(). */
  webauthn_options: unknown | null
}

export type LoginOutcome = LoginResponse | MfaChallenge

export function isMfaChallenge(x: unknown): x is MfaChallenge {
  return !!x && typeof x === 'object' && (x as any).mfa_required === true
}

// ===== MFA =====

export interface MfaStatus {
  enabled: boolean
  totp_enrolled: boolean
  webauthn_count: number
  recovery_codes_remaining: number
  must_enroll: boolean
}

export interface MfaTotpSetup {
  secret_base32: string
  otpauth_uri: string
}

export interface MfaRecoveryCodes {
  recovery_codes: string[]
}

export interface MfaWebauthnCredential {
  id: string
  label: string
  created_at: string
  last_used_at: string | null
}

export interface MfaWebauthnRegisterBegin {
  challenge_token: string
  /** Server-formatted PublicKeyCredentialCreationOptionsJSON. */
  options: unknown
}

export interface RegisterRequest {
  username: string
  email: string
  password: string
  full_name: string
  challenge_phrase?: string
  terms_of_service_accepted?: boolean
  recaptcha_token?: string
}

export interface UpdateUserRequest {
  username?: string
  email?: string
  full_name?: string
  password?: string
  is_active?: boolean
  role?: UserRole
}

// Navigation types
export interface NavigationItem {
  name: string
  href: string
  icon?: any
  current?: boolean
  requiresAuth?: boolean
  requiredRole?: UserRole
  children?: NavigationItem[]
}

// Audit Log types
export interface AuditLog {
  id: string
  event_type: string
  user_id: string | null
  actor_id: string | null
  event_data: any
  ip_address: string | null
  user_agent: string | null
  created_at: string
}

// Theme types
export type Theme = 'css-light' | 'css-dark' | 'light' | 'dark' | 'cupcake' | 'corporate'

// Notification types
export interface Notification {
  id: string
  type: 'success' | 'error' | 'warning' | 'info'
  title: string
  message: string
  duration?: number
}

// ===== Webhooks =====

/** A reusable, write-only auth credential. The secret value is never returned. */
export interface WebhookAuthHeader {
  id: string
  name: string
  header_name: string
  has_value: boolean
  created_at: string
  updated_at: string
}

export interface CreateAuthHeaderRequest {
  name: string
  header_name: string
  header_value: string
}

export interface UpdateAuthHeaderRequest {
  name?: string
  header_name?: string
  header_value?: string
}

export interface Webhook {
  id: string
  name: string
  url: string
  enabled: boolean
  signing_secret: string
  event_types: string[]
  auth_header_ids: string[]
  created_at: string
  updated_at: string
}

export interface CreateWebhookRequest {
  name: string
  url: string
  enabled?: boolean
  event_types: string[]
  auth_header_ids: string[]
}

export interface UpdateWebhookRequest {
  name?: string
  url?: string
  enabled?: boolean
  event_types?: string[]
  auth_header_ids?: string[]
}

export interface WebhookEventType {
  value: string
  label: string
}

export interface WebhookDelivery {
  id: string
  webhook_id: string
  audit_log_id: string | null
  event_type: string
  attempt: number
  success: boolean
  status_code: number | null
  response_body: string | null
  error: string | null
  request_payload: unknown
  created_at: string
}
