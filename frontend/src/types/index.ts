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
  profile: Record<string, any>
  meta: Record<string, any>
}

// Profile types
export enum ProfileFieldType {
  Text = 'Text',
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
