import type { User } from './index'
// Training system types for the frontend

export enum TrainingStatus {
  NotStarted = 'not_started',
  InProgress = 'in_progress',
  Completed = 'completed',
  Failed = 'failed',
  Expired = 'expired',
}

export enum AssessmentType {
  Practical = 'practical',
  Written = 'written',
  Both = 'both',
  ObservationOnly = 'observation_only',
}

export enum TrainingCompletionStatus {
  Completed = 'completed',
  Partial = 'partial',
  Failed = 'failed',
}

export interface TrainingStep {
  id: string
  tool_id: string
  step_number: number
  step_name: string // Changed from 'title' to match backend
  description: string
  assessment_type: AssessmentType
  passing_score?: number
  expiry_days?: number
  is_active: boolean
  created_at: string
  updated_at: string
}

export interface CreateTrainingStepRequest {
  tool_id: string
  step_number: number
  step_name: string // Changed from 'title' to match backend
  description: string
  assessment_type: AssessmentType
  passing_score?: number
  expiry_days?: number
  is_active?: boolean
}

export interface UpdateTrainingStepRequest {
  step_number?: number
  step_name?: string // Changed from 'title' to match backend
  description?: string
  assessment_type?: AssessmentType
  passing_score?: number
  expiry_days?: number
  is_active?: boolean
}

export interface TrainingPrerequisite {
  id: string
  training_step_id: string
  prerequisite_step_id: string
  created_at: string
}

export interface CreateTrainingPrerequisiteRequest {
  training_step_id: string
  prerequisite_step_id: string
}

export interface UserTrainingProgress {
  id: string
  user_id: string
  training_step_id: string
  status: TrainingStatus
  instructor_id?: string
  started_at?: string
  completed_at?: string
  expires_at?: string
  assessment_score?: number
  notes?: string
  created_at: string
  updated_at: string
  // Additional fields for compatibility
  user?: User // populated when the server joins the user row
  instructor?: User // populated when the server joins the instructor row
}

export interface StartTrainingRequest {
  training_step_id: string
  instructor_id?: string
  notes?: string
}

export interface CompleteTrainingRequest {
  training_step_id: string
  passed: boolean
  assessment_score?: number
  notes?: string
}

export interface TrainingInstructor {
  id: string
  user_id: string
  training_step_id: string
  certified_by: string
  certified_at: string
  expires_at?: string
  created_at: string
  updated_at: string
}

export interface CertifyInstructorRequest {
  user_id: string
  training_step_id: string
  expires_at?: string
}

export interface TrainingStepWithProgress {
  step: TrainingStep
  user_progress?: UserTrainingProgress
  prerequisites: TrainingStep[]
  is_available?: boolean
  instructor_required?: boolean
  // No aliases here, deliberately. `progress` and `can_start` used to sit in
  // this interface labelled "Alias for user_progress" and "Alias for
  // is_available" -- and nothing populated an alias. The server serialises its
  // own field names (models/training.rs:280), so both arrived `undefined` on
  // every response, and ToolTrainingModal read them: Start, Mark Complete and
  // Retry Training keyed off fields that were never set, so none of the three
  // rendered for anyone, on any step. An alias that nothing assigns is a
  // second name for `undefined`.
  //
  // `has_users_with_progress` was the same thing without a reader.
}

export interface ToolTrainingOverview {
  tool_id: string
  tool_name: string
  steps: TrainingStepWithProgress[]
  overall_progress: number
  can_access_tool: boolean
  next_step?: TrainingStep
}

export interface TrainingQuery {
  tool_id?: string
  user_id?: string
  instructor_id?: string
  status?: TrainingStatus
  page?: number
  per_page?: number
}

// Extended tool interface with training information
export interface ToolWithTraining {
  id: string
  name: string
  description?: string
  category: string
  status: string
  requires_training: boolean
  training_overview?: ToolTrainingOverview
  can_access: boolean
  training_steps_count: number
  completed_steps_count: number
  created_at: string
  updated_at: string
}

// ==================== TRAINER ASSIGNMENT TYPES ====================

export interface ToolTrainer {
  id: string
  user_id: string
  tool_id: string
  authorized_by: string
  authorized_at: string
  notes?: string
  expires_at?: string
  is_active: boolean
  created_at: string
  updated_at: string
}

export interface ToolTrainerWithUser {
  trainer: ToolTrainer
  user_name: string
  user_email: string
  user_full_name?: string
}

export interface AssignTrainerRequest {
  user_id: string
  tool_id: string
  notes?: string
  expires_at?: string
}

export interface UpdateTrainerRequest {
  notes?: string
  expires_at?: string
  is_active?: boolean
}

// Training record types
export interface TrainingRecord {
  id: string
  tool_id: string
  training_step_id?: string
  trainee_user_id: string
  trainer_user_id: string
  training_date: string
  completion_status: TrainingCompletionStatus
  minutes_trained?: number
  skills_covered?: string[]
  notes?: string
  next_steps?: string
  created_at: string
  updated_at: string
}

export interface TrainingRecordWithUsers {
  record: TrainingRecord
  trainee_name: string
  trainer_name: string
  tool_name: string
}

export interface CreateTrainingRecordRequest {
  tool_id: string
  training_step_id?: string
  trainee_user_id: string
  training_date: string
  completion_status: TrainingCompletionStatus
  minutes_trained?: number
  skills_covered?: string[]
  notes?: string
  next_steps?: string
}

export interface UpdateTrainingRecordRequest {
  completion_status?: TrainingCompletionStatus
  minutes_trained?: number
  training_step_id?: string
  skills_covered?: string[]
  notes?: string
  next_steps?: string
}

export interface TrainingRecordsQuery {
  tool_id?: string
  trainer_id?: string
  trainee_id?: string
  limit?: number
  offset?: number
}
