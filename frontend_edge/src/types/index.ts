export interface SystemInfo {
  platform: string
  software_version: string
  mac_address: string
  ipv4_address?: string
  ipv6_address?: string
  uptime_seconds?: number
}

export interface DeviceStatus {
  device_name: string
  is_registered: boolean
  auth_status: 'unauthenticated' | 'pending' | 'approved' | 'denied'
  system_info?: SystemInfo
  mqtt_connected?: boolean
}

export interface RegistrationRequest {
  instance_url: string
  device_code: string
}

export interface ApiResponse<T> {
  success: boolean
  data?: T
  error?: string
}

// ── ToolGuard state types (mirrors edge SyncPayload) ─────────────────────────

export type ToolStatus = 'idle' | 'in_use' | 'maintenance' | 'repair' | 'broken' | 'retired'

export interface SyncTool {
  id: string
  external_id: string | null
  name: string
  status: ToolStatus
}

export interface SyncUser {
  profile_field_value: string
  full_name: string
  is_active: boolean
  authorized_tool_ids: string[]
}

export interface SyncPayload {
  device_id: string
  profile_field: string
  tools: SyncTool[]
  users: SyncUser[]
}
