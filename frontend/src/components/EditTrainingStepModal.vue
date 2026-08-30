<template>
  <div class="modal-overlay" @click="$emit('close')">
    <div class="modal-content" @click.stop>
      <div class="modal-header">
        <h3>Edit Training Step</h3>
        <button class="close-btn" @click="$emit('close')">&times;</button>
      </div>

      <div class="modal-body">
        <form @submit.prevent="updateStep">
          <div class="form-group">
            <label for="step_number">Step Number:</label>
            <input
              id="step_number"
              v-model.number="form.step_number"
              type="number"
              class="form-control"
              min="1"
              required
            />
          </div>

          <div class="form-group">
            <label for="step_name">Title:</label>
            <input
              id="step_name"
              v-model="form.step_name"
              type="text"
              class="form-control"
              required
              placeholder="e.g., Safety Orientation"
            />
          </div>

          <div class="form-group">
            <label for="description">Description:</label>
            <textarea
              id="description"
              v-model="form.description"
              class="form-control"
              rows="3"
              required
              placeholder="Detailed description of the training step..."
            ></textarea>
          </div>

          <div class="form-group">
            <label for="assessment_type">Assessment Type:</label>
            <select
              id="assessment_type"
              v-model="form.assessment_type"
              class="form-control"
              required
            >
              <option value="practical">Practical Assessment</option>
              <option value="written">Written Test</option>
              <option value="both">Practical + Written</option>
              <option value="observation_only">Observation Only</option>
            </select>
          </div>

          <div v-if="form.assessment_type !== 'observation_only'" class="form-group">
            <label for="passing_score">Passing Score (%):</label>
            <input
              id="passing_score"
              v-model.number="form.passing_score"
              type="number"
              class="form-control"
              min="0"
              max="100"
              placeholder="e.g., 80"
            />
          </div>

          <div class="form-group">
            <label for="expires_after_days">Expires After (days):</label>
            <input
              id="expires_after_days"
              v-model.number="form.expires_after_days"
              type="number"
              class="form-control"
              min="1"
              placeholder="Leave blank for no expiration"
            />
          </div>

          <div class="form-group">
            <label class="checkbox-label">
              <input v-model="form.is_active" type="checkbox" class="checkbox" />
              <span class="checkbox-text">Active (visible to users)</span>
            </label>
          </div>

          <div class="form-actions">
            <button type="button" :disabled="loading" class="btn btn-danger" @click="deleteStep">
              {{ loading ? 'Deleting...' : 'Delete Step' }}
            </button>
            <button type="button" class="btn btn-secondary" @click="$emit('close')">Cancel</button>
            <button type="submit" :disabled="loading" class="btn btn-primary">
              {{ loading ? 'Updating...' : 'Update Training Step' }}
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
import { reactive, ref, watch } from 'vue'
import { trainingApi } from '../utils/api'
import { AssessmentType, Tool, TrainingStep, UpdateTrainingStepRequest } from '../types'

interface Props {
  step: TrainingStep | null
  tool: Tool
  existingSteps: TrainingStep[]
}

const props = defineProps<Props>()

const emit = defineEmits<{
  close: []
  'step-updated': [step: TrainingStep]
  'step-deleted': [stepId: string]
}>()

// State
const loading = ref(false)
const error = ref('')

const form = reactive<UpdateTrainingStepRequest>({
  step_number: 1,
  step_name: '',
  description: '',
  assessment_type: AssessmentType.Practical,
  passing_score: undefined,
  expires_after_days: undefined,
  is_active: true,
})

// Methods
const populateForm = (step: TrainingStep) => {
  form.step_number = step.step_number
  form.step_name = step.step_name
  form.description = step.description
  form.assessment_type = step.assessment_type
  form.passing_score = step.passing_score
  form.expires_after_days = step.expires_after_days
  form.is_active = step.is_active
}

const updateStep = async () => {
  if (!props.step) return

  try {
    loading.value = true
    error.value = ''

    const response = await trainingApi.updateTrainingStep(props.step.id, form)

    if (response.success && response.data) {
      emit('step-updated', response.data)
      emit('close')
    } else {
      error.value = response.error || 'Failed to update training step'
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to update training step'
  } finally {
    loading.value = false
  }
}

const deleteStep = async () => {
  if (
    !props.step ||
    !confirm('Are you sure you want to delete this training step? This action cannot be undone.')
  ) {
    return
  }

  try {
    loading.value = true
    error.value = ''

    const response = await trainingApi.deleteTrainingStep(props.step.id)

    if (response.success) {
      emit('step-deleted', props.step.id)
      emit('close')
    } else {
      error.value = response.error || 'Failed to delete training step'
    }
  } catch (err: any) {
    error.value = err.message || 'Failed to delete training step'
  } finally {
    loading.value = false
  }
}

// Watch for changes to the step prop
watch(
  () => props.step,
  (newStep) => {
    if (newStep) {
      populateForm(newStep)
    }
  },
  { immediate: true }
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

.btn-danger {
  background: #dc3545;
  color: white;
  border-color: #dc3545;
}

.btn-danger:hover:not(:disabled) {
  background: #c82333;
  border-color: #c82333;
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
