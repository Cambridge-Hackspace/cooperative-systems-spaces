import axios, { type AxiosResponse, type AxiosError } from 'axios'
import type { ApiResponse } from '@/types'
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

  async delete<T>(url: string): Promise<ApiResponse<T>> {
    const response = await api.delete<ApiResponse<T>>(url)
    return response.data
  },

  // Raw axios instance for direct access if needed
  raw: api,
}

export default apiClient