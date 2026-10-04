// Tool types for the frontend
export enum ToolStatus {
  Idle = 'idle',
  InUse = 'in_use',
  Maintenance = 'maintenance',
  Broken = 'broken',
  Repair = 'repair',
  Retired = 'retired',
}

export enum ToolCategory {
  Saw = 'saw',
  PowerTool = 'powertool',
  HandTools = 'hand_tools',
  Measuring = 'measuring',
  Safety = 'safety',
  Electronics = 'electronics',
  Woodworking = 'woodworking',
  Metalworking = 'metalworking',
  ThreeDPrinting = '3d_printing',
  LaserCutting = 'laser_cutting',
  Welding = 'welding',
  Other = 'other',
}

export interface Tool {
  id: string
  name: string
  description?: string
  category: ToolCategory
  status: ToolStatus
  barcode?: string
  serial_number?: string
  location?: string
  purchase_date?: string
  purchase_price?: number
  maintenance_notes?: string
  requires_training: boolean
  created_by: string
  created_at: string
  updated_at: string
  external_id?: string
  // Metered pay-per-use billing (Phase 2). A tool is "metered" iff a flat fee
  // or a per-minute rate is set. Decimals travel as strings.
  usage_flat_fee?: string | null
  usage_rate_per_min?: string | null
  usage_max_session_minutes?: number | null
  // The power receptacle this tool plugs into (#42), or null for a battery /
  // unmapped tool. Assigned via the Facility > Power tab.
  receptacle_id?: string | null
}

export interface CreateToolRequest {
  name: string
  description?: string
  category: ToolCategory
  barcode?: string
  serial_number?: string
  location?: string
  purchase_date?: string
  purchase_price?: number
  maintenance_notes?: string
  requires_training?: boolean
  external_id?: string
  usage_flat_fee?: string | null
  usage_rate_per_min?: string | null
  usage_max_session_minutes?: number | null
  // The state the tool is catalogued in; the server defaults to idle and
  // refuses in_use / retired.
  status?: ToolStatus
}

export type NewTool = CreateToolRequest

export interface UpdateToolRequest {
  name?: string
  // `null` clears the column; omitting the key leaves it alone. The server
  // distinguishes the two (`Option<Option<T>>`), so blanking a field in the UI
  // has to travel as an explicit null rather than as an absent key.
  description?: string | null
  category?: ToolCategory
  status?: ToolStatus
  barcode?: string | null
  serial_number?: string | null
  location?: string | null
  purchase_date?: string | null
  purchase_price?: number | null
  maintenance_notes?: string | null
  requires_training?: boolean
  external_id?: string | null
  usage_flat_fee?: string | null
  usage_rate_per_min?: string | null
  usage_max_session_minutes?: number | null
}

export interface ChangeToolStatusRequest {
  status: ToolStatus
  notes?: string
  scan_data?: Record<string, unknown>
}

export interface ToolEvent {
  id: string
  tool_id: string
  event_type: string
  old_status?: ToolStatus
  new_status?: ToolStatus
  user_id?: string
  actor_id?: string
  notes?: string
  scan_data?: Record<string, unknown>
  created_at: string
  // Additional fields that may be present
  user_username?: string
  metadata?: Record<string, unknown>
}

export interface ToolQuery {
  category?: ToolCategory
  status?: ToolStatus
  requires_training?: boolean
  page?: number
  per_page?: number
}

// Per-member rate tiers (#34). Decimals travel as strings.
export interface ToolRateTier {
  id: string
  tool_id: string
  name: string
  flat_fee: string | null
  rate_per_min: string | null
  max_session_minutes: number | null
  active: boolean
  created_at: string
  updated_at: string
}

export interface ToolTierAssignment {
  id: string
  user_id: string
  tool_id: string
  tier_id: string
  assigned_by: string | null
  created_at: string
  updated_at: string
}

export interface CreateTierRequest {
  name: string
  flat_fee?: string | null
  rate_per_min?: string | null
  max_session_minutes?: number | null
}

export interface UpdateTierRequest {
  name?: string
  flat_fee?: string | null
  rate_per_min?: string | null
  max_session_minutes?: number | null
  active?: boolean
}
