<template>
  <div class="modal-overlay" @click="$emit('close')">
    <div class="modal-content" @click.stop>
      <div class="modal-header">
        <h3>Complete Training Session</h3>
        <button @click="$emit('close')" class="close-btn">&times;</button>
      </div>

      <div class="modal-body">
        <div class="step-info">
          <h4>{{ step.step_name }}</h4>
          <p>{{ step.description }}</p>
          <div class="trainee-info">
            <strong>Trainee:</strong> {{ user.full_name }} ({{ user.username }})
          </div>
        </div>

        <form @submit.prevent="completeTraining">
          <div class="form-group">
            <label class="checkbox-label">
              <input 
                type="checkbox" 
                v-model="form.passed"
                class="checkbox"
              >
              <span class="checkbox-text">Training completed successfully</span>
            </label>
          </div>

          <div class="form-group" v-if="step.passing_score">
            <label for="score">Assessment Score (%):</label>
            <input 
              id="score"
              type="number"
              v-model.number="form.assessment_score"
              class="form-control"
              min="0"
              max="100"
              :placeholder="`Minimum passing score: ${step.passing_score}%`"
            >
            <div v-if="form.assessment_score && step.passing_score" class="score-feedback">
              <span v-if="form.assessment_score >= step.passing_score" class="score-pass">
                ✓ Meets passing requirements
              </span>
              <span v-else class="score-fail">
                ✗ Below passing score ({{ step.passing_score }}%)
              </span>
            </div>
          </div>

          <div class="form-group">
            <label for="notes">Training Notes:</label>
            <textarea 
              id="notes"
              v-model="form.notes"
              class="form-control"
              rows="4"
              placeholder="Detailed notes about the training session, performance, areas for improvement, etc..."
              required
            ></textarea>
          </div>

          <div class="training-summary" v-if="form.passed">
            <h5>Training Completion Summary</h5>
            <ul>
              <li>Trainee has successfully completed the training requirements</li>
              <li v-if="step.expiry_days">
                Certification will expire in {{ step.expiry_days }} days
              </li>
              <li v-if="form.assessment_score">
                Final score: {{ form.assessment_score }}%
              </li>
              <li>This will unlock access to the next training step (if applicable)</li>
            </ul>
          </div>

          <div class="training-failed" v-else-if="form.passed === false">
            <h5>Training Not Completed</h5>
            <p>Please provide detailed notes about what needs improvement before the trainee can retry.</p>
          </div>

          <div class="form-actions">
            <button type="button" @click="$emit('close')" class="btn btn-secondary">
              Cancel
            </button>
            <button 
              type="submit" 
              :disabled="loading || !canSubmit"
              :class="form.passed ? 'btn btn-success' : 'btn btn-warning'"
            >
              {{ loading ? 'Saving...' : (form.passed ? 'Mark Complete' : 'Mark Failed') }}
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
import { ref, computed } from 'vue'
import { trainingApi } from '../utils/api'
import type {
  TrainingStep,
  CompleteTrainingRequest,
  User
} from '../types'

interface Props {
  step: TrainingStep
  user: User
}

const props = defineProps<Props>()

const emit = defineEmits<{
  close: []
  completed: []
}>()

// State
const loading = ref(false)
const error = ref('')

const form = ref<CompleteTrainingRequest & { assessment_score?: number }>({
  training_step_id: props.step.id,
  passed: true,
  assessment_score: undefined,
  notes: ''
})

// Computed
const canSubmit = computed(() => {
  // Must have notes
  if (!form.value.notes.trim()) return false
  
  // If there's a passing score requirement and they passed, must meet minimum score
  if (props.step.passing_score && form.value.passed) {
    if (!form.value.assessment_score || form.value.assessment_score < props.step.passing_score) {
      return false
    }
  }
  
  return true
})

// Methods
const completeTraining = async () => {
  try {
    loading.value = true
    error.value = ''

    // Validate score requirements
    if (props.step.passing_score && form.value.passed) {
      if (!form.value.assessment_score || form.value.assessment_score < props.step.passing_score) {
        form.value.passed = false
      }
    }

    const requestData: CompleteTrainingRequest = {
      training_step_id: form.value.training_step_id,
      passed: form.value.passed,
      assessment_score: form.value.assessment_score,
      notes: form.value.notes
    }

    const response = await trainingApi.completeTrainingSession(props.user.id, requestData)
    
    if (response.success) {
      emit('completed')
    } else {
      error.value = response.error || 'Failed to complete training session'
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to complete training session'
  } finally {
    loading.value = false
  }
}
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
  border-radius: 8px;
  max-width: 600px;
  width: 90%;
  max-height: 90vh;
  overflow-y: auto;
}

.modal-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 1.5rem;
  border-bottom: 1px solid #e1e5e9;
}

.modal-header h3 {
  margin: 0;
  color: #2c3e50;
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
  color: #2c3e50;
}

.modal-body {
  padding: 1.5rem;
}

.step-info {
  background: #f8f9fa;
  padding: 1rem;
  border-radius: 6px;
  margin-bottom: 1.5rem;
}

.step-info h4 {
  margin: 0 0 0.5rem 0;
  color: #2c3e50;
}

.step-info p {
  margin: 0 0 1rem 0;
  color: #6c757d;
}

.trainee-info {
  font-size: 0.9rem;
  color: #6c757d;
}

.form-group {
  margin-bottom: 1rem;
}

.form-group label {
  display: block;
  margin-bottom: 0.5rem;
  color: #2c3e50;
  font-weight: 500;
}

.checkbox-label {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  cursor: pointer;
  font-weight: normal;
}

.checkbox {
  width: 18px;
  height: 18px;
}

.checkbox-text {
  color: #2c3e50;
}

.form-control {
  width: 100%;
  padding: 0.5rem;
  border: 1px solid #ced4da;
  border-radius: 4px;
  font-size: 0.9rem;
}

.form-control:focus {
  outline: none;
  border-color: #007bff;
  box-shadow: 0 0 0 2px rgba(0, 123, 255, 0.25);
}

.score-feedback {
  margin-top: 0.5rem;
  font-size: 0.9rem;
}

.score-pass {
  color: #28a745;
  font-weight: 500;
}

.score-fail {
  color: #dc3545;
  font-weight: 500;
}

.training-summary {
  background: #d4edda;
  border: 1px solid #c3e6cb;
  padding: 1rem;
  border-radius: 6px;
  margin: 1rem 0;
}

.training-summary h5 {
  margin: 0 0 0.5rem 0;
  color: #155724;
}

.training-summary ul {
  margin: 0;
  padding-left: 1.5rem;
}

.training-summary li {
  color: #155724;
  margin-bottom: 0.25rem;
}

.training-failed {
  background: #fff3cd;
  border: 1px solid #ffeaa7;
  padding: 1rem;
  border-radius: 6px;
  margin: 1rem 0;
}

.training-failed h5 {
  margin: 0 0 0.5rem 0;
  color: #856404;
}

.training-failed p {
  margin: 0;
  color: #856404;
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

.btn-success {
  background: #28a745;
  color: white;
  border-color: #28a745;
}

.btn-success:hover:not(:disabled) {
  background: #1e7e34;
  border-color: #1e7e34;
}

.btn-warning {
  background: #ffc107;
  color: #212529;
  border-color: #ffc107;
}

.btn-warning:hover:not(:disabled) {
  background: #e0a800;
  border-color: #d39e00;
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
