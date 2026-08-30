<template>
  <!-- Debug: Component mounted check -->
  <div style="background: red; color: white; padding: 5px; margin: 5px">
    ToolTrainingCard is rendering! Tool: {{ tool.name }}
  </div>
  <div class="training-card">
    <div class="card-header">
      <h3>{{ tool.name }} Training</h3>
      <div class="training-status">
        <span :class="statusClass">{{ formatTrainingStatus() }}</span>
      </div>
    </div>

    <div class="card-body">
      <div v-if="trainingOverview" class="progress-overview">
        <div class="progress-bar-container">
          <div class="progress-bar">
            <div
              class="progress-fill"
              :style="{ width: trainingOverview.overall_progress + '%' }"
            ></div>
          </div>
          <span class="progress-text"
            >{{ Math.round(trainingOverview.overall_progress) }}% Complete</span
          >
        </div>

        <div class="access-status">
          <span v-if="trainingOverview.can_access_tool" class="status-badge status-success">
            ✓ Tool Access Granted
          </span>
          <span v-else class="status-badge status-warning"> ⚠ Training Required </span>
        </div>
      </div>

      <div v-if="trainingOverview?.steps.length" class="training-steps">
        <h4>Training Steps</h4>
        <!-- Debug info -->
        <div
          v-if="trainingOverview"
          style="
            background: #f0f8ff;
            padding: 10px;
            margin-bottom: 10px;
            font-size: 12px;
            border: 1px solid #ccc;
          "
        >
          <strong>Debug Info:</strong><br />
          Overall Progress: {{ trainingOverview.overall_progress }}%<br />
          Can Access Tool: {{ trainingOverview.can_access_tool }}<br />
          Steps Count: {{ trainingOverview.steps?.length || 0 }}<br />
          Raw Steps Data:
          <pre style="font-size: 10px; max-height: 100px; overflow: auto">{{
            JSON.stringify(trainingOverview.steps, null, 2)
          }}</pre>
        </div>
        <div class="steps-list">
          <div
            v-for="stepWithProgress in trainingOverview.steps"
            :key="stepWithProgress.step.id"
            class="step-item"
            :class="getStepStatusClass(stepWithProgress)"
          >
            <div class="step-number" :class="getStepNumberClass(stepWithProgress)">
              <!-- Debug info for each step -->
              <div
                v-if="stepWithProgress.step.step_number === 1"
                style="
                  position: absolute;
                  top: -20px;
                  left: 0;
                  font-size: 10px;
                  background: yellow;
                  padding: 2px;
                "
              >
                Status: {{ stepWithProgress.user_progress?.status || 'null' }}
              </div>
              <span v-if="stepWithProgress.user_progress?.status === 'completed'" class="checkmark"
                >✓</span
              >
              <span v-else>{{ stepWithProgress.step.step_number }}</span>
            </div>
            <div class="step-content">
              <div class="step-title-container">
                <h5>{{ stepWithProgress.step.step_name }}</h5>
                <div
                  v-if="stepWithProgress.user_progress?.status === 'completed'"
                  class="completion-badge"
                >
                  <span class="badge-text">✓ COMPLETED</span>
                  <span v-if="stepWithProgress.user_progress?.completed_at" class="completion-date">
                    {{ formatDate(stepWithProgress.user_progress.completed_at) }}
                  </span>
                </div>
              </div>
              <p>{{ stepWithProgress.step.description }}</p>
              <div class="step-meta">
                <span class="assessment-type">{{
                  formatAssessmentType(stepWithProgress.step.assessment_type)
                }}</span>
                <span v-if="stepWithProgress.step.expires_after_days" class="expiry-info">
                  Expires after {{ stepWithProgress.step.expires_after_days }} days
                </span>
              </div>
            </div>
            <div class="step-actions">
              <!--              <div class="step-status" :class="getStepStatusTextClass(stepWithProgress)">-->
              <!--                {{ f(stepWithProgress) }}-->
              <!--              </div>-->
              <!-- Progress indicator for completed steps -->
              <div
                v-if="stepWithProgress.user_progress?.status === 'completed'"
                class="completion-indicator"
              >
                <div class="progress-circle completed">
                  <span class="progress-icon">✓</span>
                </div>
                <span class="completion-text">Step Complete</span>
              </div>
              <div v-if="canManageTraining" class="action-buttons">
                <button
                  v-if="stepWithProgress.is_available && !stepWithProgress.user_progress"
                  class="btn btn-sm btn-primary"
                  @click="startTraining(stepWithProgress.step)"
                >
                  Start Training
                </button>
                <button
                  v-if="stepWithProgress.user_progress?.status === 'in_progress' && isInstructor"
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
                  Retry
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div v-else class="no-training">
        <p>No training steps configured for this tool.</p>
        <button v-if="canManageTraining" class="btn btn-primary" @click="showSetupModal = true">
          Set Up Training
        </button>
      </div>
    </div>

    <!-- Start Training Modal -->
    <StartTrainingModal
      v-if="showStartModal"
      :step="selectedStep"
      :user="user"
      @close="showStartModal = false"
      @started="onTrainingStarted"
    />

    <!-- Complete Training Modal -->
    <CompleteTrainingModal
      v-if="showCompleteModal"
      :step="selectedStep"
      :user="user"
      @close="showCompleteModal = false"
      @completed="onTrainingCompleted"
    />

    <!-- Training Setup Modal -->
    <ToolTrainingSetupModal
      v-if="showSetupModal"
      :tool="tool"
      @close="showSetupModal = false"
      @created="onTrainingSetupCreated"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useAuthStore } from '../stores/auth'
import { trainingApi } from '../utils/api'
import type {
  Tool,
  TrainingStep,
  ToolTrainingOverview,
  TrainingStepWithProgress,
  AssessmentType,
  User,
} from '../types'
import StartTrainingModal from './StartTrainingModal.vue'
import CompleteTrainingModal from './CompleteTrainingModal.vue'
import ToolTrainingSetupModal from './ToolTrainingSetupModal.vue'

interface Props {
  tool: Tool
  user?: User
}

const props = defineProps<Props>()

const emit = defineEmits<{
  'create-training': []
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
const selectedStep = ref<TrainingStep | null>(null)

// Computed
const canManageTraining = computed(() => {
  const userRole = auth.user?.role?.toLowerCase()
  return userRole === 'staff' || userRole === 'admin'
})

const isInstructor = computed(() => {
  // This would need to check if the current user is certified as an instructor
  // For now, we'll use staff/admin as proxy
  return canManageTraining.value
})

const statusClass = computed(() => {
  if (!trainingOverview.value) return 'status-unknown'

  if (trainingOverview.value.can_access_tool) return 'status-success'
  if (trainingOverview.value.overall_progress > 0) return 'status-progress'
  return 'status-pending'
})

// Methods
const loadTrainingOverview = async () => {
  try {
    loading.value = true
    error.value = ''

    const userId = props.user?.id || 'me'
    console.log('🔍 Loading training overview for tool:', props.tool.id, 'user:', userId)

    const response = await trainingApi.getToolTrainingOverview(props.tool.id, userId)

    if (response.success && response.data) {
      console.log('API Response:', response.data)
      console.log('Overall progress:', response.data.overall_progress)
      trainingOverview.value = response.data

      // Emit training status to parent
      console.log('📊 Steps in response:', response.data.steps?.length)
      console.log('🔍 First step details:', response.data.steps?.[0])

      trainingOverview.value = response.data

      // Emit training status to parent
      emit('training-status-changed', props.tool.id, response.data.can_access_tool)
    } else {
      console.error('❌ API response failed:', response.error)
      error.value = response.error || 'Failed to load training overview'
    }
  } catch (err: any) {
    console.error('💥 Exception loading training overview:', err)
    error.value = err.message || 'Failed to load training overview'
  } finally {
    loading.value = false
  }
}

const formatTrainingStatus = (): string => {
  if (!trainingOverview.value) return 'Loading...'

  if (trainingOverview.value.can_access_tool) {
    return 'Training Complete'
  } else if (trainingOverview.value.overall_progress > 0) {
    return `Training in Progress (${Math.round(trainingOverview.value.overall_progress)}%)`
  } else {
    return 'Training Required'
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

const getStepStatusClass = (stepWithProgress: TrainingStepWithProgress): string => {
  if (!stepWithProgress.user_progress) {
    return stepWithProgress.is_available ? 'step-available' : 'step-locked'
  }

  const status = stepWithProgress.user_progress.status
  return `step-${status.replace('_', '-')}`
}

const getStepNumberClass = (stepWithProgress: TrainingStepWithProgress): string => {
  console.log(
    'Step',
    stepWithProgress.step.step_number,
    'user_progress:',
    stepWithProgress.user_progress
  )
  console.log('Status check:', stepWithProgress.user_progress?.status === 'completed')
  if (stepWithProgress.user_progress?.status === 'completed') {
    return 'step-number-completed'
  }
  return ''
}

const formatDate = (dateString: string): string => {
  const date = new Date(dateString)
  return date.toLocaleDateString()
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

// Lifecycle
onMounted(() => {
  void loadTrainingOverview()
})
</script>

<style scoped>
.training-card {
  background: white;
  border: 1px solid #e1e5e9;
  border-radius: 8px;
  overflow: hidden;
}

.card-header {
  background: #f8f9fa;
  padding: 1rem 1.5rem;
  border-bottom: 1px solid #e1e5e9;
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.card-header h3 {
  margin: 0;
  color: #2c3e50;
}

.training-status {
  font-size: 0.9rem;
}

.status-success {
  color: #28a745;
  font-weight: 500;
}
.status-progress {
  color: #ffc107;
  font-weight: 500;
}
.status-pending {
  color: #6c757d;
  font-weight: 500;
}
.status-unknown {
  color: #dc3545;
  font-weight: 500;
}

.card-body {
  padding: 1.5rem;
}

.progress-overview {
  margin-bottom: 2rem;
}

.progress-bar-container {
  display: flex;
  align-items: center;
  gap: 1rem;
  margin-bottom: 1rem;
}

.progress-bar {
  flex: 1;
  height: 8px;
  background: #e9ecef;
  border-radius: 4px;
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: #28a745;
  transition: width 0.3s ease;
}

.progress-text {
  font-size: 0.9rem;
  color: #6c757d;
  min-width: 80px;
}

.access-status {
  text-align: center;
}

.status-badge {
  display: inline-block;
  padding: 0.5rem 1rem;
  border-radius: 20px;
  font-size: 0.9rem;
  font-weight: 500;
}

.status-success {
  background: #d4edda;
  color: #155724;
}

.status-warning {
  background: #fff3cd;
  color: #856404;
}

.training-steps h4 {
  margin: 0 0 1rem 0;
  color: #2c3e50;
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
  padding: 1rem;
  border: 1px solid #e1e5e9;
  border-radius: 6px;
  transition: border-color 0.2s;
}

.step-item:hover {
  border-color: #007bff;
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
}

.step-completed {
  border-left: 4px solid #28a745;
  background: #f8fff9;
}

.step-failed {
  border-left: 4px solid #dc3545;
  background: #fff8f8;
}

.step-expired {
  border-left: 4px solid #fd7e14;
  background: #fff8f0;
}

.step-number {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  background: #007bff;
  color: white;
  border-radius: 50%;
  font-weight: bold;
  font-size: 0.9rem;
  flex-shrink: 0;
}

.step-number {
  position: relative;
}

.step-number-completed {
  background: #28a745 !important;
  color: white !important;
  border: 2px solid #1e7e34;
  box-shadow: 0 2px 8px rgba(40, 167, 69, 0.3);
}

.checkmark {
  font-size: 1.2rem;
  font-weight: bold;
}

.step-content {
  flex: 1;
}

.step-title-container {
  display: flex;
  align-items: center;
  gap: 1rem;
  margin-bottom: 0.5rem;
}

.step-content h5 {
  margin: 0 0 0.5rem 0;
  color: #2c3e50;
}

.step-content p {
  margin: 0 0 0.5rem 0;
  color: #6c757d;
  font-size: 0.9rem;
}

.completion-badge {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 0.25rem;
}

.badge-text {
  background: #28a745;
  color: white;
  padding: 0.25rem 0.75rem;
  border-radius: 12px;
  font-size: 0.75rem;
  font-weight: bold;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.completion-date {
  font-size: 0.7rem;
  color: #6c757d;
  font-style: italic;
}

.completion-indicator {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.5rem;
  margin-bottom: 0.5rem;
  padding: 0.5rem;
  background: #f8fff9;
  border-radius: 8px;
}

.step-meta {
  display: flex;
  gap: 1rem;
  font-size: 0.8rem;
  color: #6c757d;
}

.step-actions {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 0.5rem;
  min-width: 120px;
}

.step-status {
  font-size: 0.85rem;
  color: #6c757d;
  font-weight: 500;
}

.status-completed-text {
  color: #28a745 !important;
  font-weight: bold !important;
}

.action-buttons {
  display: flex;
  gap: 0.5rem;
}

.btn {
  padding: 0.375rem 0.75rem;
  border: 1px solid transparent;
  border-radius: 4px;
  cursor: pointer;
  font-size: 0.875rem;
  transition: all 0.2s;
  text-decoration: none;
  display: inline-block;
}

.btn-sm {
  padding: 0.25rem 0.5rem;
  font-size: 0.8rem;
}

.btn-primary {
  background: #007bff;
  color: white;
  border-color: #007bff;
}

.btn-primary:hover {
  background: #0056b3;
  border-color: #0056b3;
}

.btn-success {
  background: #28a745;
  color: white;
  border-color: #28a745;
}

.btn-success:hover {
  background: #1e7e34;
  border-color: #1e7e34;
}

.btn-secondary {
  background: #6c757d;
  color: white;
  border-color: #6c757d;
}

.btn-secondary:hover {
  background: #545b62;
  background: #fff8f0;
}

.progress-circle {
  width: 40px;
  height: 40px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  font-weight: bold;
  margin-bottom: 0.25rem;
}

.progress-circle.completed {
  background: #28a745;
  color: white;
  box-shadow: 0 2px 8px rgba(40, 167, 69, 0.3);
}

.progress-icon {
  font-size: 1.5rem;
}

.completion-text {
  font-size: 0.8rem;
  color: #28a745;
  font-weight: 500;
}

.no-training {
  text-align: center;
  padding: 2rem;
  color: #6c757d;
}

.no-training p {
  margin-bottom: 1rem;
}

@media (max-width: 768px) {
  .step-item {
    flex-direction: column;
    align-items: stretch;
  }

  .step-actions {
    align-items: stretch;
  }

  .action-buttons {
    justify-content: stretch;
  }

  .btn {
    flex: 1;
  }
}
</style>
