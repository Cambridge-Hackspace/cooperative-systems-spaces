// Tool and training types for the frontend
export * from './tools'
export * from './training'

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
  Admin = 'Admin',
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
  /**
   * Whether the address has been confirmed.
   *
   * Optional because a server older than the email work does not send it, and
   * because `undefined` and `false` must not be treated alike: the first means
   * "this deployment does not track confirmation", the second means "not
   * confirmed".
   */
  email_verified?: boolean
  profile: Record<string, unknown>
  meta: Record<string, unknown>
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
  Select = 'Select',
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
  profile: Record<string, unknown>
}

export interface UpdateProfileRequest {
  profile: Record<string, unknown>
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
  return !!x && typeof x === 'object' && (x as Record<string, unknown>).mfa_required === true
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

// ===== Doors =====

export type DoorRuleKind = 'role' | 'user' | 'card' | 'open_access'
export type DoorRuleEffect = 'allow' | 'deny'
export type DoorAccessMethod = 'rfid' | 'qr_checkin' | 'admin_remote'

export interface Door {
  id: string
  name: string
  location: string | null
  description: string | null
  edge_device_id: string | null
  unlock_duration_ms: number
  enabled: boolean
  created_at: string
  updated_at: string
  place_id_from?: string | null
  place_id_to?: string | null
}

export interface DoorAccessRule {
  id: string
  door_id: string
  kind: string
  value: string
  effect: string
  created_at: string
  /** Optional reusable schedule. `null` = the rule applies 24/7. */
  schedule_id?: string | null
}

export interface DoorDetail extends Door {
  rules: DoorAccessRule[]
}

export interface DoorAccessEvent {
  id: string
  door_id: string
  user_id: string | null
  method: string
  card_id_attempted: string | null
  granted: boolean
  reason: string | null
  ip_address: string | null
  occurred_at: string
  created_at: string
}

export interface CreateDoorRequest {
  name: string
  location?: string | null
  description?: string | null
  edge_device_id?: string | null
  unlock_duration_ms?: number
  enabled?: boolean
  /** Required. Use a special place (e.g. `Outside`) for exterior doors. */
  place_id_from: string
  /** Required. */
  place_id_to: string
}

export interface UpdateDoorRequest {
  name?: string
  /** Pass `null` to clear; omit to leave unchanged. */
  location?: string | null
  description?: string | null
  edge_device_id?: string | null
  unlock_duration_ms?: number
  enabled?: boolean
  /** PATCH-style: set to a real place ID (special places included).
      Doors can no longer be set to `null` — model exterior with a
      special place like `Outside`. */
  place_id_from?: string
  place_id_to?: string
}

export interface AddDoorRuleRequest {
  kind: DoorRuleKind
  value: string
  effect?: DoorRuleEffect
  /** Optional reusable schedule. Omit or pass `null` for "always". */
  schedule_id?: string | null
}

export interface DoorInfo {
  id: string
  name: string
  location: string | null
  enabled: boolean
  you_are_authorized: boolean
  reason: string | null
}

export interface DoorCheckinResult {
  unlocked: boolean
  reason: string | null
}

// ===== Places (configurable hierarchy) =====

export interface Place {
  id: string
  parent_id: string | null
  place_type: string
  name: string
  description: string | null
  external_id: string | null
  created_at: string
  updated_at: string
  /** Marked by operator as a "special" place (Outside, Common Area, Parking,
      …). Special places ignore the configured type-ordering rules and must
      be roots. */
  is_special: boolean
}

export interface PlaceConfig {
  enabled: boolean
  /** Ordered list, top → leaf. Index = depth. */
  types: string[]
}

export interface PlaceAttachedCounts {
  doors: number
  tools: number
  devices: number
}

export interface PlaceDetail extends Place {
  /** Top-down: `[root, ..., immediate_parent]`. Does not include this place. */
  ancestors: Place[]
  children: Place[]
  attached: PlaceAttachedCounts
}

// ===== Home links (admin-curated, audience-gated) =====

export type HomeLinkAudience = 'everyone' | 'anonymous' | 'logged_in' | 'member' | 'staff'

export interface HomeLink {
  id: string
  label: string
  url: string
  description: string | null
  icon: string | null
  audience: HomeLinkAudience
  sort_order: number
  enabled: boolean
  created_at: string
  updated_at: string
  /** RFC-3339; when set and in the past, the public endpoint hides this link. */
  expires_at?: string | null
}

export interface CreateHomeLinkRequest {
  label: string
  url: string
  description?: string | null
  icon?: string | null
  audience: HomeLinkAudience
  sort_order?: number
  enabled?: boolean
  /** RFC-3339 (`new Date(...).toISOString()`); pass `null` for no expiry. */
  expires_at?: string | null
}

export interface UpdateHomeLinkRequest {
  label?: string
  url?: string
  description?: string | null
  icon?: string | null
  audience?: HomeLinkAudience
  sort_order?: number
  enabled?: boolean
  /** Pass `null` to clear an existing expiry; omit to leave unchanged. */
  expires_at?: string | null
}

// ===== Schedules (weekly windows attached to access rules) =====

export type DayOfWeek = 'mon' | 'tue' | 'wed' | 'thu' | 'fri' | 'sat' | 'sun'

export interface ScheduleInterval {
  day: DayOfWeek
  /** HH:MM 24-hour */
  start: string
  /** HH:MM 24-hour; must be strictly greater than `start`. */
  end: string
}

export interface Schedule {
  id: string
  name: string
  description: string | null
  intervals: ScheduleInterval[]
  created_at: string
  updated_at: string
  is_public: boolean
}

export interface CreateScheduleRequest {
  name: string
  description?: string | null
  intervals: ScheduleInterval[]
  is_public?: boolean
}

export interface UpdateScheduleRequest {
  name?: string
  description?: string | null
  intervals?: ScheduleInterval[]
  is_public?: boolean
}

export interface CreatePlaceRequest {
  name: string
  place_type: string
  parent_id?: string | null
  description?: string | null
  external_id?: string | null
  is_special?: boolean
}

export interface UpdatePlaceRequest {
  name?: string
  place_type?: string
  /** Pass `null` to promote to a root; omit to leave unchanged. */
  parent_id?: string | null
  description?: string | null
  external_id?: string | null
  is_special?: boolean
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
  icon?: unknown
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
  event_data: Record<string, unknown>
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
