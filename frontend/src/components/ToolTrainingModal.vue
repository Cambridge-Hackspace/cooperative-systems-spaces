<template>
  <div class="modal-overlay" @click="closeModal">
    <div class="modal-content" @click.stop>
      <div class="modal-header bg-gradient-to-br from-primary via-secondary to-primary">
        <div>
          <h3>{{ tool.name }}</h3>
          <p class="subtitle">Complete training to access this tool</p>
        </div>
        <button class="close-btn" @click="closeModal">&times;</button>
      </div>

      <div class="modal-body bg-base-100">
        <!-- Admin Actions -->
        <div
          v-if="canManageTraining && (trainingOverview?.steps?.length || 0) > 0"
          class="admin-section"
        >
          <h4>Training Management</h4>
          <div class="admin-actions">
            <button class="btn btn-primary" @click="addNewStep">Add Training Step</button>
            <!--            <button @click="viewAllProgress" class="btn fondary">-->
            <!--              View All Users' Progress-->
            <!--            </button>-->
            <button class="btn btn-info" @click="showTrainerManagement = true">
              Manage Trainers
            </button>
            <!-- OR embed it inline -->
            <button
              class="btn btn-success"
              @click="showRecordTrainingForm = !showRecordTrainingForm"
            >
              {{ showRecordTrainingForm ? 'Cancel Recording' : 'Record Training Session' }}
            </button>
          </div>
        </div>

        <!-- Trainer Section for Trainers -->
        <div v-if="isTrainer && !canManageTraining" class="trainer-section">
          <h4>Trainer Actions</h4>
          <div class="trainer-actions">
            <button
              class="btn btn-success"
              @click="showRecordTrainingForm = !showRecordTrainingForm"
            >
              {{ showRecordTrainingForm ? 'Cancel Recording' : 'Record Training Session' }}
            </button>
            <button class="btn btn-secondary" @click="viewTrainingHistory">
              View Training Records
            </button>
          </div>
        </div>

        <!-- Training History Modal -->
        <div v-if="showTrainingHistory" class="training-history-modal">
          <div class="modal-content">
            <div class="modal-header">
              <h4>📋 Training History - {{ tool.name }}</h4>
              <button class="close-btn" @click="showTrainingHistory = false">&times;</button>
            </div>

            <div class="modal-body">
              <div v-if="loadingTrainingHistory" class="loading">
                <div class="spinner"></div>
                <p>Loading training history...</p>
              </div>

              <div v-else-if="trainingHistoryError" class="error">
                <p>{{ trainingHistoryError }}</p>
                <button class="btn btn-primary" @click="viewTrainingHistory">Try Again</button>
              </div>

              <div v-else>
                <div v-if="trainingHistory.length === 0" class="no-history">
                  <p>No training records found for this tool.</p>
                </div>

                <div v-else class="history-list">
                  <div class="history-filters">
                    <input type="date" placeholder="Start Date" class="form-control" />
                    <input type="date" placeholder="End Date" class="form-control" />
                    <select class="form-control">
                      <option value="">All Statuses</option>
                      <option value="completed">Completed</option>
                      <option value="partial">Partial</option>
                      <option value="failed">Failed</option>
                    </select>
                  </div>

                  <div class="history-table">
                    <table>
                      <thead>
                        <tr>
                          <th>Date</th>
                          <th>Step</th>
                          <th>Trainee</th>
                          <th>Trainer</th>
                          <th>Status</th>
                          <th>Duration</th>
                          <th>Notes</th>
                        </tr>
                      </thead>
                      <tbody>
                        <tr v-for="record in trainingHistory" :key="record.id">
                          <td>{{ formatDate(record.training_date) }}</td>
                          <td>Step {{ record.step_number }}: {{ record.step_name }}</td>
                          <td>{{ record.trainee_name }}</td>
                          <td>{{ record.trainer_name }}</td>
                          <td>
                            <span :class="'status-' + record.completion_status.toLowerCase()">
                              {{ record.completion_status }}
                            </span>
                          </td>
                          <td>
                            {{ record.minutes_trained ? record.minutes_trained + ' min' : 'N/A' }}
                          </td>
                          <td>{{ record.notes || 'No notes' }}</td>
                        </tr>
                      </tbody>
                    </table>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Inline Record Training Form -->
        <div
          v-if="showRecordTrainingForm"
          class="record-training-form bg-primary text-primary-content"
        >
          <h4>📝 Record Training Session</h4>

          <div v-if="loadingUsersForRecord" class="loading">
            <div class="spinner"></div>
            <p>Loading users...</p>
          </div>

          <form v-else class="record-form" @submit.prevent="submitRecordForm">
            <div class="form-row">
              <div class="form-group">
                <label for="record-step">Training Step *</label>
                <select
                  id="record-step"
                  v-model="recordFormData.training_step_id"
                  class="form-control select select-secondary"
                  required
                >
                  <option value="">Select training step...</option>
                  <option
                    v-for="stepWithProgress in trainingOverview?.steps"
                    :key="stepWithProgress.step.id"
                    :value="stepWithProgress.step.id"
                  >
                    Step {{ stepWithProgress.step.step_number }}

                    : {{ stepWithProgress.step.step_name }}
                  </option>
                </select>
              </div>

              <div class="form-group">
                <label for="record-trainee">Trainee</label>
                <select
                  id="record-trainee"
                  v-model="recordFormData.trainee_user_id"
                  class="form-control select select-secondary"
                  required
                >
                  <option value="">Select trainee...</option>
                  <option v-for="row in usersForRecord" :key="row.id" :value="row.id">
                    {{ row.full_name || row.username }} ({{ row.email }})
                  </option>
                </select>
              </div>

              <div class="form-group">
                <label for="record-date">Training Date</label>
                <input
                  id="record-date"
                  v-model="recordFormData.training_date"
                  type="date"
                  class="form-control input"
                  :max="today"
                  required
                />
              </div>
            </div>

            <div class="form-row">
              <div class="form-group">
                <label for="record-status">Completion Status</label>
                <select
                  id="record-status"
                  v-model="recordFormData.completion_status"
                  class="form-control select"
                  required
                >
                  <option value="completed">✅ Completed</option>
                  <option value="partial">⏳ Partial</option>
                  <option value="failed">❌ Failed</option>
                </select>
              </div>

              <div class="form-group">
                <label for="record-minutes">Duration (Minutes)</label>
                <input
                  id="record-minutes"
                  v-model.number="recordFormData.minutes_trained"
                  type="number"
                  min="1"
                  max="480"
                  class="form-control input"
                  placeholder="e.g. 60"
                />
              </div>
            </div>

            <div class="form-group">
              <label for="record-notes">Training Notes</label>
              <textarea
                id="record-notes"
                v-model="recordFormData.notes"
                class="form-control textarea"
                rows="3"
                placeholder="Notes about the training session..."
              ></textarea>
            </div>

            <div v-if="recordError" class="error">{{ recordError }}</div>

            <div class="form-actions">
              <button type="submit" :disabled="recordSubmitting" class="btn btn-secondary">
                {{ recordSubmitting ? 'Recording...' : 'Record Training' }}
              </button>
            </div>
          </form>
        </div>

        <div v-if="loading" class="loading">
          <div class="spinner"></div>
          <p>Loading training information...</p>
        </div>

        <div v-else-if="error" class="error-message">
          <h4>Unable to Load Training Information</h4>
          <p>{{ error }}</p>
          <div class="error-actions">
            <button class="btn btn-primary" @click="loadTrainingOverview">Try Again</button>
            <button
              v-if="canManageTraining"
              class="btn btn-secondary"
              @click="showSetupModal = true"
            >
              Set Up Training
            </button>
          </div>
        </div>

        <div
          v-else-if="
            !tool.requires_training || !trainingOverview || trainingOverview.steps.length === 0
          "
        >
          <!-- No Training Steps -->
          <div class="no-training">
            <div class="no-training-icon">🛠️</div>
            <h4>No Training Required</h4>
            <p>This tool does not require any special training. You can use it freely.</p>
            <div v-if="canManageTraining" class="admin-actions">
              <p class="admin-note">
                As an administrator, you can set up training requirements for this tool.
              </p>
              <button class="btn btn-primary" @click="showSetupModal = true">
                Set Up Training Requirements
              </button>
            </div>
          </div>
        </div>

        <div v-else>
          <!-- Training Overview -->
          <div class="training-overview">
            <!-- Overall Progress -->
            <div class="progress-section bg-secondary text-secondary-content mt-4">
              <div class="progress-header">
                <h4>Your Training Progress</h4>
                <div class="progress-percentage">
                  {{ Math.round(trainingOverview.overall_progress) }}% Complete
                </div>
              </div>

              <div class="progress-bar-container">
                <div class="progress-bar">
                  <div
                    class="progress-fill"
                    :style="{ width: trainingOverview.overall_progress + '%' }"
                  ></div>
                </div>
              </div>

              <div class="access-status">
                <div v-if="trainingOverview.can_access_tool" class="access-granted rounded-md p-3">
                  <div class="status-icon">✅</div>
                  <div class="status-text">
                    <strong>Training Complete - Tool Access Granted</strong>
                    <p>You have completed all required training and can now use this tool.</p>
                  </div>
                </div>
                <div v-else class="access-pending rounded-md p-3">
                  <div class="status-icon">⏳</div>
                  <div class="status-text">
                    <strong>Training Required</strong>
                    <p>Complete the training steps below to access this tool.</p>
                  </div>
                </div>
              </div>
            </div>

            <!-- Training Steps -->
            <div class="steps-section">
              <h4>Training Steps</h4>
              <div class="steps-list">
                <div
                  v-for="stepWithProgress in trainingOverview.steps"
                  :key="stepWithProgress.step.id"
                  class="step-item bg-secondary text-secondary-content rounded-md"
                  :class="getStepStatusClass(stepWithProgress)"
                >
                  <div class="step-number" :class="getStepNumberClass(stepWithProgress)">
                    <span
                      v-if="stepWithProgress.user_progress?.status === 'completed'"
                      class="checkmark"
                      >✓</span
                    >
                    <span v-else>{{ stepWithProgress.step.step_number }}</span>
                  </div>

                  <div class="step-content">
                    <div class="step-header">
                      <h5>{{ stepWithProgress.step.step_name }}</h5>
                      <!--                      <div class="step-status-badge" :class="getStatusBadgeClass(stepWithProgress)">-->
                      <!--                        {{ formatStepStatus(stepWithProgress) }}-->
                      <!--                      </div>-->
                    </div>

                    <p class="step-description">{{ stepWithProgress.step.description }}</p>

                    <div class="step-meta">
                      <span class="assessment-type">
                        {{ formatAssessmentType(stepWithProgress.step.assessment_type) }}
                      </span>
                      <span v-if="stepWithProgress.step.passing_score" class="passing-score">
                        {{ stepWithProgress.step.passing_score }} % required
                      </span>
                      <span v-if="stepWithProgress.step.expires_after_days" class="expiry-info">
                        Valid for {{ stepWithProgress.step.expires_after_days }} days
                      </span>
                    </div>

                    <!-- Progress Details -->
                    <div v-if="stepWithProgress.user_progress" class="progress-details">
                      <div v-if="stepWithProgress.user_progress.started_at" class="progress-item">
                        <strong>Started:</strong>
                        {{ formatDate(stepWithProgress.user_progress.started_at) }}
                      </div>
                      <div v-if="stepWithProgress.user_progress.completed_at" class="progress-item">
                        <strong>Completed:</strong>
                        {{ formatDate(stepWithProgress.user_progress.completed_at) }}
                      </div>
                      <div v-if="stepWithProgress.user_progress.expires_at" class="progress-item">
                        <strong>Expires:</strong>
                        {{ formatDate(stepWithProgress.user_progress.expires_at) }}
                      </div>
                      <div
                        v-if="stepWithProgress.user_progress.instructor_id"
                        class="progress-item"
                      >
                        <strong>Instructor ID:</strong>
                        {{ stepWithProgress.user_progress.instructor_id }}
                      </div>
                      <div
                        v-if="stepWithProgress.user_progress.assessment_score !== null"
                        class="progress-item"
                      >
                        <strong>Score:</strong>
                        {{ stepWithProgress.user_progress.assessment_score }} %
                      </div>
                      <div v-if="stepWithProgress.user_progress.notes" class="progress-item notes">
                        <strong>Notes:</strong> {{ stepWithProgress.user_progress.notes }}
                      </div>
                    </div>
                  </div>

                  <!-- Action Buttons -->
                  <div class="step-actions">
                    <!-- Admin Edit Button -->
                    <button
                      v-if="canManageTraining"
                      class="btn btn-sm btn-secondary"
                      @click="editStep(stepWithProgress.step)"
                    >
                      Edit Step
                    </button>

                    <!-- User Training Actions -->
                    <button
                      v-if="
                        stepWithProgress.is_available &&
                        !stepWithProgress.user_progress &&
                        canStartTraining
                      "
                      class="btn btn-sm btn-primary"
                      @click="startTraining(stepWithProgress.step)"
                    >
                      Start Training
                    </button>

                    <button
                      v-if="
                        stepWithProgress.user_progress?.status === 'in_progress' && isInstructor
                      "
                      class="btn btn-sm btn-success"
                      @click="completeTraining(stepWithProgress.step)"
                    >
                      Mark Complete
                    </button>

                    <button
                      v-if="stepWithProgress.user_progress?.status === 'failed'"
                      class="btn btn-sm btn-secondary"
                      @click="retryTraining(stepWithProgress.step)"
                    >
                      Retry Training
                    </button>

                    <!--                    <div v-if="!stepWithProgress.is_available && !stepWithProgress.user_progress" class="step-locked">-->
                    <!--                      <span class="lock-icon">🔒</span>-->
                    <!--                      <span class="lock-text">Complete prerequisites first</span>-->
                    <!--                    </div>-->
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
      <div class="modal-footer bg-base-300">
        <button class="btn btn-secondary" @click="closeModal">Close</button>
      </div>
    </div>

    <!-- Start Training Modal -->
    <StartTrainingModal
      v-if="showStartModal"
      :step="selectedStep!"
      :user="user!"
      @close="showStartModal = false"
      @started="onTrainingStarted"
    />
    <!-- Complete Training Modal -->
    <CompleteTrainingModal
      v-if="showCompleteModal"
      :step="selectedStep!"
      :user="user!"
      @close="showCompleteModal = false"
      @completed="onTrainingCompleted"
    />
    />

    <!-- Training Setup Modal -->
    <ToolTrainingSetupModal
      v-if="showSetupModal"
      :tool="tool"
      @close="showSetupModal = false"
      @created="onTrainingSetupCreated"
    />

    <!-- Edit Training Step Modal -->
    <EditTrainingStepModal
      v-if="showEditStepModal && selectedStep"
      :step="selectedStep"
      :tool="tool"
      :existing-steps="trainingOverview?.steps?.map((s) => s.step) || []"
      @close="showEditStepModal = false"
      @step-updated="onStepUpdated"
      @step-deleted="onStepDeleted"
    />

    <!-- Create Training Step Modal -->
    <CreateTrainingStepModal
      v-if="showCreateStepModal"
      :tools="[tool]"
      @close="showCreateStepModal = false"
      @created="onStepCreated"
    />

    <!-- Trainer Management Modal -->
    <TrainerManagement
      v-if="showTrainerManagement"
      :tool="tool"
      @close="showTrainerManagement = false"
      @trainer-updated="onTrainerUpdatedWithAuthCheck"
    />

    <!-- Record Training Modal -->
    <RecordTrainingModal
      v-if="showRecordTraining"
      :tool="tool"
      @close="showRecordTraining = false"
      @recorded="onTrainingRecorded"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useAuthStore } from '../stores/auth'
import { trainingApi } from '../utils/api'
import { trainerApi } from '../utils/api'
import { userApi } from '../utils/api'
import type {
  Tool,
  TrainingStep,
  ToolTrainingOverview,
  TrainingStepWithProgress,
  AssessmentType,
  CreateTrainingRecordRequest,
  TrainingCompletionStatus,
  User,
} from '../types'
import StartTrainingModal from './StartTrainingModal.vue'
import CompleteTrainingModal from './CompleteTrainingModal.vue'
import ToolTrainingSetupModal from './ToolTrainingSetupModal.vue'
import CreateTrainingStepModal from './CreateTrainingStepModal.vue'
import EditTrainingStepModal from './EditTrainingStepModal.vue'
import TrainerManagement from './TrainerManagement.vue'
import RecordTrainingModal from './RecordTrainingModal.vue'
import { localDate } from '@/lib/dates'

interface Props {
  tool: Tool
  user?: User
}

const props = defineProps<Props>()
const emit = defineEmits<{
  close: []
  'training-updated': []
  'training-status-changed': [toolId: string, canAccessTool: boolean]
}>()

const auth = useAuthStore()

// State
const trainingOverview = ref<ToolTrainingOverview | null>(null)
const loading = ref(false)
const error = ref('')
const showStartModal = ref(false)
const showCompleteModal = ref(false)
const showSetupModal = ref(false)
const showEditStepModal = ref(false)
const showCreateStepModal = ref(false)
const showTrainerManagement = ref(false)
const showRecordTraining = ref(false)
const isTrainerForTool = ref(false)
const showRecordTrainingForm = ref(false)
const loadingUsersForRecord = ref(false)
const usersForRecord = ref<User[]>([])
const trainingHistory = ref<any[]>([])
const showTrainingHistory = ref(false)
const loadingTrainingHistory = ref(false)
const trainingHistoryError = ref('')
const recordError = ref('')
const recordSubmitting = ref(false)
const recordFormData = ref<CreateTrainingRecordRequest>({
  tool_id: '',
  training_step_id: '',
  trainee_user_id: '',
  training_date: localDate(),
  completion_status: 'completed' as TrainingCompletionStatus,
  minutes_trained: undefined,
  skills_covered: undefined,
  notes: '',
  next_steps: '',
})
const selectedStep = ref<TrainingStep | null>(null)

// Computed
const canManageTraining = computed(() => {
  const userRole = auth.user?.role?.toLowerCase()
  return userRole === 'staff' || userRole === 'admin'
})

const isInstructor = computed(() => {
  return canManageTraining.value
})

const isTrainer = computed(() => {
  // Checks if user is an authorized trainer for this specific tool
  // This is populated by checkTrainerAuthorization() function
  // Also includes admins/staff who can manage all training
  return isTrainerForTool.value || canManageTraining.value
})

const canStartTraining = computed(() => {
  return !!auth.user // User must be logged in
})

// See RecordTrainingModal, which has the same inline form and had the same
// UTC default and ceiling.
const today = computed(() => localDate())

// Methods
const closeModal = () => {
  emit('close')
}

const loadTrainingOverview = async () => {
  try {
    loading.value = true
    error.value = ''

    const userId = props.user?.id || 'me'
    console.log('Loading training overview for tool:', props.tool.id, 'user:', userId)

    const response = await trainingApi.getToolTrainingOverview(props.tool.id, userId)
    console.log('Training overview response:', response)

    if (response.success && response.data) {
      trainingOverview.value = response.data
      console.log('Training overview loaded:', response.data)
      console.log('Steps count:', response.data.steps?.length || 0)
      emit('training-status-changed', props.tool.id, response.data.can_access_tool)
    } else {
      error.value = response.error || 'Failed to load training overview'
      console.log('Training overview error:', error.value)
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to load training overview'
    console.error('Error loading training overview:', err)
  } finally {
    loading.value = false
  }
}

const getStepStatusClass = (stepWithProgress: TrainingStepWithProgress): string => {
  if (!stepWithProgress.user_progress) {
    return stepWithProgress.is_available ? 'step-available' : 'step-locked'
  }

  const status = stepWithProgress.user_progress.status
  return `step-${status.replace('_', '-')}`
}

const getStepNumberClass = (stepWithProgress: TrainingStepWithProgress): string => {
  if (!stepWithProgress.user_progress) {
    return stepWithProgress.is_available ? 'number-available' : 'number-locked'
  }

  const status = stepWithProgress.user_progress.status
  switch (status) {
    case 'completed':
      return 'number-completed'
    case 'in_progress':
      return 'number-in-progress'
    case 'failed':
      return 'number-failed'
    case 'expired':
      return 'number-expired'
    default:
      return 'number-not-started'
  }
}

const formatAssessmentType = (type: AssessmentType): string => {
  const types = {
    practical: 'Practical Assessment',
    written: 'Written Test',
    both: 'Practical + Written',
    observation_only: 'Observation Only',
  }

  return types[type] || type
}

const formatDate = (dateString: string): string => {
  return new Date(dateString).toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

const startTraining = (step: TrainingStep) => {
  selectedStep.value = step
  showStartModal.value = true
}

const completeTraining = (step: TrainingStep) => {
  selectedStep.value = step
  showCompleteModal.value = true
}

const retryTraining = (step: TrainingStep) => {
  startTraining(step)
}

const onTrainingStarted = () => {
  showStartModal.value = false
  selectedStep.value = null
  void loadTrainingOverview()
  emit('training-updated')
}

const onTrainingCompleted = () => {
  showCompleteModal.value = false
  selectedStep.value = null
  void loadTrainingOverview()
  emit('training-updated')
}

const onTrainingSetupCreated = () => {
  showSetupModal.value = false
  void loadTrainingOverview()
  emit('training-updated')
}

const addNewStep = () => {
  showCreateStepModal.value = true
}

const editStep = (step: TrainingStep) => {
  selectedStep.value = step
  showEditStepModal.value = true
}

const onStepCreated = () => {
  showCreateStepModal.value = false
  void loadTrainingOverview()
  emit('training-updated')
}

const onStepUpdated = () => {
  showEditStepModal.value = false
  selectedStep.value = null
  void loadTrainingOverview()
  emit('training-updated')
}

const onStepDeleted = () => {
  void loadTrainingOverview()
  showEditStepModal.value = false
  selectedStep.value = null
  emit('training-updated')
}

// New event handlers for trainer management

const onTrainingRecorded = () => {
  showRecordTraining.value = false
  // Reload training overview to reflect new training records
  void loadTrainingOverview()
  emit('training-updated')
}

const viewTrainingHistory = async () => {
  try {
    showTrainingHistory.value = true
    loadingTrainingHistory.value = true
    trainingHistoryError.value = ''

    const response = await userApi.getTrainingHistory(props.tool.id)

    if (response.success && response.data) {
      trainingHistory.value = response.data
    } else {
      trainingHistoryError.value = response.error || 'Failed to load training history'
    }
  } catch (error: any) {
    console.error('Error loading training history:', error)
    trainingHistoryError.value = error.message || 'Failed to load training history'
  } finally {
    loadingTrainingHistory.value = false
  }
}

// Check if user is an authorized trainer for this tool
const checkTrainerAuthorization = async () => {
  if (!auth.user?.id) {
    isTrainerForTool.value = false
    return
  }

  try {
    const response = await trainerApi.checkTrainerAuthorization(props.tool.id, auth.user.id)
    if (response.success && response.data) {
      isTrainerForTool.value = !!response.data
    } else {
      isTrainerForTool.value = false
    }
  } catch (err) {
    console.error('Error checking trainer authorization:', err)
    isTrainerForTool.value = false
  }
}

// Update trainer authorization when trainers are updated
const onTrainerUpdatedWithAuthCheck = () => {
  showTrainerManagement.value = false
  // Reload training overview to get updated trainer status
  void loadTrainingOverview()
  // Check trainer authorization again since assignments may have changed
  void checkTrainerAuthorization()
  emit('training-updated')
}

// Record training functionality
const loadUsersForRecord = async () => {
  try {
    loadingUsersForRecord.value = true
    recordError.value = ''

    // Use the new training-specific roster endpoint that includes tool context
    const response = await userApi.getUsersForTraining(props.tool.id)

    if (response.success && response.data?.items) {
      usersForRecord.value = response.data.items.filter((user: User) => user.is_active)
    } else {
      recordError.value = response.error || 'Failed to load users'
    }
  } catch (err: any) {
    recordError.value = err.message || 'Failed to load users'
  } finally {
    loadingUsersForRecord.value = false
  }
}

const submitRecordForm = async () => {
  try {
    recordSubmitting.value = true
    recordError.value = ''

    // Prepare the data
    const requestData: CreateTrainingRecordRequest = {
      ...recordFormData.value,
      tool_id: props.tool.id,
      notes: recordFormData.value.notes || undefined,
      next_steps: recordFormData.value.next_steps || undefined,
    }

    const response = await trainerApi.createTrainingRecord(requestData)

    if (response.success) {
      // Reset form
      recordFormData.value = {
        tool_id: '',
        training_step_id: '',
        trainee_user_id: '',
        training_date: localDate(),
        completion_status: 'completed' as TrainingCompletionStatus,
        minutes_trained: undefined,
        skills_covered: undefined,
        notes: '',
        next_steps: '',
      }

      showRecordTrainingForm.value = false
      // Reload training overview
      void loadTrainingOverview()
      emit('training-updated')
    } else {
      recordError.value = response.error || 'Failed to record training session'
    }
  } catch (err: any) {
    recordError.value = err.message || 'Failed to record training session'
  } finally {
    recordSubmitting.value = false
  }
}

// Watch for record training form visibility
watch(
  () => showRecordTrainingForm.value,
  (newValue) => {
    if (newValue && usersForRecord.value.length === 0) {
      void loadUsersForRecord()
    }
  }
)

// Lifecycle
onMounted(() => {
  // Load training overview and check trainer authorization
  void loadTrainingOverview()
  void checkTrainerAuthorization()
})

// Watch for tool changes
watch(
  () => props.tool.id,
  () => {
    void loadTrainingOverview()
    void checkTrainerAuthorization()
  }
)
</script>

<style scoped>
.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.modal-content {
  background: white;
  border-radius: 12px;
  max-width: 800px;
  width: 90%;
  max-height: 90vh;
  overflow-y: auto;
  box-shadow: 0 20px 40px rgba(0, 0, 0, 0.15);
}

.modal-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  padding: 2rem;
  border-bottom: 1px solid #e1e5e9;
  color: white;
  border-radius: 12px 12px 0 0;
}

.modal-header h3 {
  margin: 0 0 0.25rem 0;
  font-size: 1.5rem;
}

.subtitle {
  margin: 0;
  opacity: 0.9;
  font-size: 0.9rem;
}

.close-btn {
  background: none;
  border: none;
  font-size: 1.5rem;
  cursor: pointer;
  color: white;
  padding: 0;
  width: 30px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  opacity: 0.8;
  transition: opacity 0.2s;
}

.close-btn:hover {
  opacity: 1;
}

.modal-body {
  padding: 2rem;
  min-height: 200px;
}

.loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 3rem;
  color: #6c757d;
}

.spinner {
  width: 40px;
  height: 40px;
  border: 4px solid #f3f3f3;
  border-top: 4px solid #3498db;
  border-radius: 50%;
  animation: spin 1s linear infinite;
  margin-bottom: 1rem;
}

@keyframes spin {
  0% {
    transform: rotate(0deg);
  }

  100% {
    transform: rotate(360deg);
  }
}

.error-message {
  text-align: center;
  padding: 2rem;
  color: #721c24;
  background: #f8d7da;
  border-radius: 8px;
  border: 1px solid #f5c6cb;
}

.error-message h4 {
  margin: 0 0 1rem 0;
  color: #721c24;
}

.error-actions {
  display: flex;
  gap: 1rem;
  justify-content: center;
  margin-top: 1.5rem;
}

.no-training {
  text-align: center;
  padding: 3rem 1rem;
}

.no-training-icon {
  font-size: 3rem;
  margin-bottom: 1rem;
}

.no-training h4 {
  margin: 0 0 1rem 0;
}

.no-training p {
  color: #6c757d;
  margin-bottom: 2rem;
}

.admin-note {
  color: #856404;
  background: #fff3cd;
  padding: 1rem;
  border-radius: 6px;
  margin: 1.5rem 0;
  border: 1px solid #ffeaa7;
}

.training-overview {
  display: flex;
  flex-direction: column;
  gap: 2rem;
}

.progress-section {
  padding: 1.5rem;
  border-radius: 8px;
  border: 1px solid #e9ecef;
}

.progress-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 1rem;
}

.progress-header h4 {
  margin: 0;
}

.progress-percentage {
  font-size: 1.25rem;
  font-weight: bold;
  color: #495057;
}

.progress-bar-container {
  margin-bottom: 1.5rem;
}

.progress-bar {
  width: 100%;
  height: 12px;
  background: #e9ecef;
  border-radius: 6px;
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: linear-gradient(90deg, #28a745, #20c997);
  transition: width 0.3s ease;
}

.access-status {
  display: flex;
  align-items: center;
  gap: 1rem;
  padding: 1rem;
  border-radius: 6px;
}

.access-granted {
  background: #d4edda;
  border: 1px solid #c3e6cb;
  color: #155724;
}

.access-pending {
  background: #fff3cd;
  border: 1px solid #ffeaa7;
  color: #856404;
}

.status-icon {
  font-size: 1.5rem;
  flex-shrink: 0;
}

.status-text strong {
  display: block;
  margin-bottom: 0.25rem;
}

.status-text p {
  margin: 0;
}

.steps-section h4 {
  margin-bottom: 1rem;
}

.steps-list {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.step-item {
  display: flex;
  align-items: flex-start;
  gap: 1rem;
  padding: 1.5rem;
  border: 1px solid #e1e5e9;
  border-radius: 8px;
  transition: all 0.2s;
}

.step-item:hover {
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.1);
}

.step-available {
  border-left: 4px solid #28a745;
}

.step-locked {
  border-left: 4px solid #6c757d;
  opacity: 0.7;
}

.step-in-progress {
  border-left: 4px solid #ffc107;
  background: rgba(255, 193, 7, 0.05);
}

.step-completed {
  border-left: 4px solid #28a745;
  background: rgba(40, 167, 69, 0.05);
}

.step-failed {
  border-left: 4px solid #dc3545;
  background: rgba(220, 53, 69, 0.05);
}

.step-expired {
  border-left: 4px solid #fd7e14;
  background: rgba(253, 126, 20, 0.05);
}

.step-number {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  background: #007bff;
  color: white;
  border-radius: 50%;
  font-weight: bold;
  font-size: 1rem;
  font-weight: bold;
}

.step-number {
  position: relative;
}

.number-completed {
  background: #28a745 !important;
  color: white !important;
  border: 2px solid #1e7e34;
  box-shadow: 0 2px 8px rgba(40, 167, 69, 0.3);
}

.checkmark {
  font-size: 1.2rem;
  font-weight: bold;
}

.step-completed {
  border-left: 4px solid #28a745;
  background: #f8fff9;
}

.step-content {
  flex: 1;
}

.step-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 0.5rem;
}

.step-header h5 {
  margin: 0;
  color: #2c3e50;
  font-size: 1.1rem;
}

.step-status-badge {
  padding: 0.25rem 0.75rem;
  border-radius: 12px;
  font-size: 0.8rem;
  font-weight: 600;
  text-transform: uppercase;
}

.badge-available {
  background: #d4edda;
  color: #155724;
}

.badge-locked {
  background: #e2e3e5;
  color: #6c757d;
}

.badge-in-progress {
  background: #fff3cd;
  color: #856404;
}

.badge-completed {
  background: #d4edda;
  color: #155724;
}

.badge-failed {
  background: #f8d7da;
  color: #721c24;
}

.badge-expired {
  background: #fdf2e9;
  color: #e67e22;
}

.step-description {
  margin: 0 0 1rem 0;
  color: #6c757d;
  line-height: 1.5;
}

.step-meta {
  display: flex;
  gap: 1rem;
  flex-wrap: wrap;
  margin-bottom: 1rem;
  font-size: 0.9rem;
}

.step-meta > span {
  background: #f8f9fa;
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  color: #495057;
}

.progress-details {
  background: #f8f9fa;
  padding: 1rem;
  border-radius: 6px;
  margin-top: 1rem;
}

.progress-item {
  margin-bottom: 0.5rem;
  font-size: 0.9rem;
  color: #495057;
}

.progress-item:last-child {
  margin-bottom: 0;
}

.progress-item.notes {
  padding: 0.5rem;
  border-radius: 4px;
  border: 1px solid #e9ecef;
}

.step-actions {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  align-items: flex-end;
  min-width: 120px;
}

.step-locked {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  color: #6c757d;
  font-size: 0.85rem;
}

.lock-icon {
  font-size: 1rem;
}

.admin-section {
  border-top: 1px solid #e1e5e9;
  padding-top: 2rem;
}

.admin-section h4 {
  margin-bottom: 1rem;
}

.admin-actions {
  display: flex;
  gap: 1rem;
  flex-wrap: wrap;
}

.trainer-section {
  border-top: 1px solid #e1e5e9;
  padding-top: 2rem;
  margin-top: 2rem;
}

.trainer-section h4 {
  margin-bottom: 1rem;
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.trainer-section h4::before {
  content: '👨‍🏫';
  font-size: 1.2rem;
}

.trainer-actions {
  display: flex;
  gap: 1rem;
  flex-wrap: wrap;
}

/* Additional trainer-specific styling */
.trainer-section .btn {
  min-width: 160px;
}

/* Record Training Form Styles */
.record-training-form {
  border: 1px solid #b3d9ff;
  border-radius: 8px;
  padding: 1.5rem;
  margin: 1.5rem 0;
}

.record-training-form h4 {
  margin: 0 0 1.5rem 0;
  color: #2c3e50;
  font-size: 1.1rem;
}

.record-form {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.form-row {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 1rem;
}

.form-group {
  display: flex;
  flex-direction: column;
}

.form-group label {
  font-weight: 500;
  margin-bottom: 0.5rem;
}

.form-control {
  padding: 0.75rem;
  border: 1px solid #ced4da;
  border-radius: 4px;
  font-size: 1rem;
  transition: border-color 0.2s;
}

.form-control:focus {
  outline: none;
  border-color: #007bff;
  box-shadow: 0 0 0 2px rgba(0, 123, 255, 0.25);
}

.form-text {
  font-size: 0.875rem;
  color: #6c757d;
  margin-top: 0.25rem;
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  margin-top: 0.5rem;
}

.spinner {
  width: 32px;
  height: 32px;
  border: 3px solid #f3f3f3;
  border-top: 3px solid #3498db;
  border-radius: 50%;
  animation: spin 1s linear infinite;
  margin: 0 auto;
}

@keyframes spin {
  0% {
    transform: rotate(0deg);
  }

  100% {
    transform: rotate(360deg);
  }
}

.loading {
  text-align: center;
  padding: 2rem;
  color: #6c757d;
}

.loading p {
  margin-top: 1rem;
  margin-bottom: 0;
}

.error {
  background: #f8d7da;
  color: #721c24;
  padding: 0.75rem;
  border-radius: 4px;
  border: 1px solid #f5c6cb;
  margin-bottom: 1rem;
}

select.form-control,
textarea.form-control {
  cursor: pointer;
}

textarea.form-control {
  resize: vertical;
  min-height: 80px;
}

/* Responsive styles for trainer section */

.modal-footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 1.5rem 2rem;
  border-top: 1px solid #e1e5e9;
  border-radius: 0 0 12px 12px;
}

.btn {
  padding: 0.5rem 1rem;
  border: 1px solid transparent;
  border-radius: 6px;
  cursor: pointer;
  font-size: 0.9rem;
  font-weight: 500;
  transition: all 0.2s;
  text-decoration: none;
  display: inline-block;
}

.btn-sm {
  padding: 0.375rem 0.75rem;
  font-size: 0.85rem;
}

.btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

@media (max-width: 768px) {
  .modal-content {
    width: 95%;
    margin: 1rem;
    max-height: 95vh;
  }

  .modal-header,
  .modal-body,
  .modal-footer {
    padding: 1rem;
  }

  .step-item {
    flex-direction: column;
    gap: 1rem;
  }

  .step-actions {
    align-items: stretch;
    min-width: auto;
  }

  .step-header {
    flex-direction: column;
    align-items: flex-start;
    gap: 0.5rem;
  }

  .admin-actions,
  .error-actions {
    flex-direction: column;
  }

  .trainer-actions {
    flex-direction: column;
  }

  .trainer-section .btn {
    min-width: auto;
    width: 100%;
  }

  .form-row {
    grid-template-columns: 1fr;
  }

  .record-training-form .form-row {
    grid-template-columns: 1fr;
  }

  .form-actions {
    margin-top: 1rem;
  }

  .btn {
    width: 100%;
    text-align: center;
  }
}
</style>
