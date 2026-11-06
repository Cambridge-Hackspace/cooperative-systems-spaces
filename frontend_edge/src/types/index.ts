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
  remote_transport?: RemoteTransportStatus
}

/** What the edge knows about its own uplink to the server. */
///
/** Replaces a `mqtt_connected?: boolean` that the edge never sent and nothing */
/** here ever read -- the field existed only in this file, so the one piece of */
/** information an operator needed during #207 (is the link actually up?) was */
/** typed but unobtainable. */
export interface RemoteTransportStatus {
  /** 'websocket' | 'mqtt', or null before a transport has started. */
  kind: string | null
  /** null when the active transport does not report its own liveness, which is */
  /** the honest answer for MQTT -- paho reconnects internally and the edge never */
  /** sees the transitions. Not the same as false. */
  connected: boolean | null
  /** The negotiated TLS key exchange, e.g. 'X25519MLKEM768' for a hybrid */
  /** post-quantum handshake. The edge prefers one; whether it gets one depends */
  /** on what terminates TLS at the server. */
  key_exchange: string | null
  /** Non-zero means the transport's supervisor had to be restarted, which is */
  /** worth seeing even on a link that is currently up. */
  supervisor_restarts: number
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
  /** Hex argon2id(device_pepper, card) (#109) -- not the card itself. */
  profile_field_digest: string
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
