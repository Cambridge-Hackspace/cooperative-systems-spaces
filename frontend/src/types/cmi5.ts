// cmi5 training-module types, mirroring the server's cmi5 API responses.

export interface Cmi5Course {
  id: string
  course_iri: string
  title?: string | null
  description?: string | null
  content_path: string
  imported_by?: string | null
  created_at: string
  updated_at: string
  deleted_at?: string | null
  // The server also returns the verbatim manifest; the UI does not display it.
  manifest_xml?: string
}

export interface Cmi5AssignableUnit {
  id: string
  course_id: string
  block_id?: string | null
  au_iri: string
  title?: string | null
  launch_url: string
  launch_parameters?: string | null
  launch_method?: string | null
  move_on: string
  mastery_score?: number | null
  position: number
  /** The training step this AU is bound to; null until an admin maps it. */
  training_step_id?: string | null
  created_at: string
  updated_at: string
}

/** A course together with its assignable units, as import/get return it. */
export interface Cmi5CourseWithAus {
  course: Cmi5Course
  aus: Cmi5AssignableUnit[]
}

/** The response to a launch: the URL to open, and the registration it belongs to. */
export interface Cmi5LaunchResponse {
  launch_url: string
  registration: string
}

/** Body of the AU→training-step binding. `null` unbinds. */
export interface Cmi5AssignRequest {
  training_step_id: string | null
}

/** A launchable module as a learner sees it. */
export interface Cmi5LearnerModule {
  au_id: string
  au_title?: string | null
  course_id: string
  course_title?: string | null
  tool_id: string
  training_step_id: string
  completed: boolean
}
