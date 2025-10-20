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
  id: string;
  name: string;
  description?: string;
  category: ToolCategory;
  status: ToolStatus;
  barcode?: string;
  serial_number?: string;
  location?: string;
  purchase_date?: string;
  purchase_price?: number;
  maintenance_notes?: string;
  requires_training: boolean;
  created_by: string;
  created_at: string;
  updated_at: string;
  external_id?: string;
  // Additional fields that may be present
  manufacturer?: string;
  model?: string;
  notes?: string;
}

export interface CreateToolRequest {
  name: string;
  description?: string;
  category: ToolCategory;
  barcode?: string;
  serial_number?: string;
  location?: string;
  purchase_date?: string;
  purchase_price?: number;
  maintenance_notes?: string;
  requires_training?: boolean;
  external_id?: string;
  manufacturer?: string;
  model?: string;
  notes?: string;
  status?: string;
}

export type NewTool = CreateToolRequest;

export interface UpdateToolRequest {
  name?: string;
  description?: string;
  category?: ToolCategory;
  status?: ToolStatus;
  barcode?: string;
  serial_number?: string;
  location?: string;
  purchase_date?: string;
  purchase_price?: number;
  maintenance_notes?: string;
  requires_training?: boolean;
  external_id?: string;
  manufacturer?: string;
  model?: string;
  notes?: string;
}

export interface ChangeToolStatusRequest {
  status: ToolStatus;
  notes?: string;
  scan_data?: any;
}

export interface ToolEvent {
  id: string;
  tool_id: string;
  event_type: string;
  old_status?: ToolStatus;
  new_status?: ToolStatus;
  user_id?: string;
  actor_id?: string;
  notes?: string;
  scan_data?: any;
  created_at: string;
  // Additional fields that may be present
  user_username?: string;
  metadata?: any;
}

export interface ToolQuery {
  category?: ToolCategory;
  status?: ToolStatus;
  requires_training?: boolean;
  page?: number;
  per_page?: number;
}