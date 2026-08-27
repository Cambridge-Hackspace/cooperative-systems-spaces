<template>
  <div class="modal-overlay" @click="$emit('close')">
    <div class="modal-content" @click.stop>
      <div class="modal-header">
        <h3>Start Training Session</h3>
        <button @click="$emit('close')" class="close-btn">&times;</button>
      </div>

      <div class="modal-body">
        <div class="step-info">
          <h4>{{ step.step_name }}</h4>
          <p>{{ step.description }}</p>
          <div class="step-meta">
            <span class="assessment-type">
              Assessment Type: {{ formatAssessmentType(step.assessment_type) }}
            </span>
            <span v-if="step.passing_score" class="passing-score">
              Passing Score: {{ step.passing_score }}%
            </span>
          </div>
        </div>

        <form @submit.prevent="startTraining">
          <div class="form-group">
            <label for="instructor">Select Instructor (Optional):</label>
            <select 
              id="instructor" 
              v-model="form.instructor_id"
              class="form-control"
            >
            <option value="">Self-study (No instructor)</option>
            <option 
              v-for="instructor in availableInstructors" 
              :key="instructor.id"
              :value="instructor.id"
            >
              {{ instructor.full_name || instructor.username }}
            </option>
            </select>
          </div>

          <div class="form-group">
            <label for="notes">Notes (Optional):</label>
            <textarea 
              id="notes"
              v-model="form.notes"
              class="form-control"
              rows="3"
              placeholder="Any additional notes about this training session..."
            ></textarea>
          </div>

          <div class="form-actions">
            <button type="button" @click="$emit('close')" class="btn btn-secondary">
              Cancel
            </button>
            <button type="submit" :disabled="loading" class="btn btn-primary">
              {{ loading ? 'Starting...' : 'Start Training' }}
            </button>
          </div>
        </form>

        <div v-if="error" class="error-message">
          {{ error }}
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { trainingApi, userApi } from '../utils/api'
import type {
  TrainingStep,
  StartTrainingRequest,
  AssessmentType,
  User
} from '../types'

interface Props {
  step: TrainingStep
  user: User
}

const props = defineProps<Props>()

const emit = defineEmits<{
  close: []
  started: []
}>()

// State
const loading = ref(false)
const error = ref('')
const availableInstructors = ref<User[]>([])

const form = ref<StartTrainingRequest>({
  training_step_id: props.step.id,
  instructor_id: undefined,
  notes: ''
})

// Methods
const loadInstructors = async () => {
  try {
    // In a real implementation, you'd have an endpoint to get certified instructors
    // For now, we'll get staff/admin users as potential instructors
    const response = await userApi.getAllUsers()
    
    if (response.success && response.data?.items) {
      availableInstructors.value = response.data.items.filter(
        (user: User) => {
          const role = user.role?.toLowerCase()
          return role === 'staff' || role === 'admin'
        }
      )
    }
  } catch (err) {
    console.error('Error loading instructors:', err)
  }
}

const formatAssessmentType = (type: AssessmentType): string => {
  const types = {
    practical: 'Practical Assessment',
    written: 'Written Test', 
    both: 'Practical + Written',
    observation_only: 'Observation Only'
  }
  return types[type] || type
}

const startTraining = async () => {
  try {
    loading.value = true
    error.value = ''

    const response = await trainingApi.startTrainingSession(props.user.id, form.value)
    
    if (response.success) {
      emit('started')
    } else {
      error.value = response.error || 'Failed to start training session'
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to start training session'
  } finally {
    loading.value = false
  }
}

// Lifecycle
onMounted(() => {
  loadInstructors()
})
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
  background: var(--fallback-b1,oklch(var(--b1)/1));
  border-radius: 8px;
  max-width: 500px;
  width: 90%;
  max-height: 90vh;
  overflow-y: auto;
}

.modal-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 1.5rem;
  border-bottom: 1px solid var(--fallback-b3,oklch(var(--b3)/1));
}

.modal-header h3 {
  margin: 0;
  color: var(--fallback-bc,oklch(var(--bc)/1));
}

.close-btn {
  background: none;
  border: none;
  font-size: 1.5rem;
  cursor: pointer;
  color: #6c757d;
  padding: 0;
  width: 30px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.close-btn:hover {
  color: var(--fallback-bc,oklch(var(--bc)/1));
}

.modal-body {
  padding: 1.5rem;
}

.step-info {
  background: var(--fallback-b2,oklch(var(--b2)/1));
  padding: 1rem;
  border-radius: 6px;
  margin-bottom: 1.5rem;
}

.step-info h4 {
  margin: 0 0 0.5rem 0;
  color: var(--fallback-bc,oklch(var(--bc)/1));
}

.step-info p {
  margin: 0 0 1rem 0;
  color: #6c757d;
}

.step-meta {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  font-size: 0.9rem;
  color: #6c757d;
}

.form-group {
  margin-bottom: 1rem;
}

.form-group label {
  display: block;
  margin-bottom: 0.5rem;
  color: var(--fallback-bc,oklch(var(--bc)/1));
  font-weight: 500;
}

.form-control {
  width: 100%;
  padding: 0.5rem;
  border: 1px solid var(--fallback-b3,oklch(var(--b3)/1));
  border-radius: 4px;
  font-size: 0.9rem;
}

.form-control:focus {
  outline: none;
  border-color: #007bff;
  box-shadow: 0 0 0 2px rgba(0, 123, 255, 0.25);
}

.form-actions {
  display: flex;
  gap: 1rem;
  justify-content: flex-end;
  margin-top: 1.5rem;
}

.btn {
  padding: 0.5rem 1rem;
  border: 1px solid transparent;
  border-radius: 4px;
  cursor: pointer;
  font-size: 0.9rem;
  transition: all 0.2s;
}

.btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.btn-primary {
  background: #007bff;
  color: white;
  border-color: #007bff;
}

.btn-primary:hover:not(:disabled) {
  background: #0056b3;
  border-color: #0056b3;
}

.btn-secondary {
  background: #6c757d;
  color: white;
  border-color: #6c757d;
}

.btn-secondary:hover {
  background: #545b62;
  border-color: #545b62;
}

.error-message {
  background: #f8d7da;
  color: #721c24;
  padding: 0.75rem;
  border-radius: 4px;
  margin-top: 1rem;
  border: 1px solid #f5c6cb;
}

@media (max-width: 768px) {
  .modal-content {
    width: 95%;
    margin: 1rem;
  }
  
  .form-actions {
    flex-direction: column;
  }
  
  .btn {
    width: 100%;
  }
}
</style>
