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
  Unknown = 'unknown',
  Newbie = 'newbie',
  Member = 'member',
  Staff = 'staff',
  Admin = 'admin'
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