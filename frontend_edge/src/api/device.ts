import axios from 'axios'
import type { ApiResponse, DeviceStatus, RegistrationRequest, SyncPayload } from '@/types'

const api = axios.create({
  baseURL: '/api',
  headers: {
    'Content-Type': 'application/json',
  },
})

export const deviceApi = {
  async getStatus(): Promise<ApiResponse<DeviceStatus>> {
    const response = await api.get<ApiResponse<DeviceStatus>>('/status')
    return response.data
  },

  async register(data: RegistrationRequest): Promise<ApiResponse<string>> {
    const response = await api.post<ApiResponse<string>>('/register', data)
    return response.data
  },

  async getToolguardState(): Promise<ApiResponse<SyncPayload>> {
    const response = await api.get<ApiResponse<SyncPayload>>('/toolguard/state')
    return response.data
  },
}
